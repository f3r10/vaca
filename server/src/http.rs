use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::config::{
    Config, CASHU_TS_COMMIT, CASHU_TS_VERSION, CDK_GIT_REV, FEE_RESERVE_RECIPIENT, SIGFLAG_V1,
    SIGFLAG_V2,
};
use crate::db::{self, NewContribution};
use crate::error::ApiError;
use crate::lock;
use crate::mint::{self, MintBackend};
use crate::pot::{self, CreatePotRequest, PotView};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Arc<Mutex<Connection>>,
    pub mint: MintBackend,
}

#[derive(Serialize)]
struct Health {
    ok: bool,
    experimental: bool,
    mint_url_set: bool,
    max_pot_sats: u64,
    grace_seconds: u64,
    fee_reserve_recipient: &'static str,
    sigflag_v1: &'static str,
    sigflag_v2: &'static str,
    server_key_path_set: bool,
    server_key_file_present: bool,
    /// The public key from the environment. The key file is not read.
    server_pubkey_set: bool,
    /// A checked proof is stored. Signing, melt, and refund are not.
    counts_contributions: bool,
    cdk_rev: &'static str,
    cdk_linked: bool,
    cashu_ts: &'static str,
    cashu_ts_commit: &'static str,
}

#[derive(Serialize)]
struct PotList {
    pots: Vec<PotView>,
}

pub fn router(state: AppState) -> Router {
    let origin = state
        .config
        .cors_origin
        .parse::<HeaderValue>()
        .expect("cors origin was checked at startup");
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(origin))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([axum::http::header::CONTENT_TYPE]);
    Router::new()
        .route("/api/health", get(health))
        .route("/api/pots", get(list_pots).post(create_pot))
        .route("/api/pots/{id}", get(get_pot))
        .route("/api/pots/{id}/contributions", post(post_contribution))
        .route("/api/pots/{id}/payout", post(payout_unimplemented))
        .route("/api/pots/{id}/refunds", post(refunds_unimplemented))
        .fallback(not_found)
        .layer(cors)
        .with_state(state)
}

pub fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    let key = state.config.server_key_status();
    Json(Health {
        ok: true,
        experimental: state.config.experimental,
        mint_url_set: !state.config.mint_url.is_empty(),
        max_pot_sats: state.config.max_pot_sats,
        grace_seconds: state.config.grace_seconds,
        fee_reserve_recipient: FEE_RESERVE_RECIPIENT,
        sigflag_v1: SIGFLAG_V1,
        sigflag_v2: SIGFLAG_V2,
        server_key_path_set: key.path_set,
        server_key_file_present: key.file_present,
        server_pubkey_set: state.config.server_pubkey.is_some(),
        counts_contributions: true,
        cdk_rev: CDK_GIT_REV,
        cdk_linked: false,
        cashu_ts: CASHU_TS_VERSION,
        cashu_ts_commit: CASHU_TS_COMMIT,
    })
}

async fn list_pots(State(state): State<AppState>) -> Result<Json<PotList>, ApiError> {
    let pots = with_db(&state, db::list_pots)?
        .into_iter()
        .map(|pot| pot.view())
        .collect();
    Ok(Json(PotList { pots }))
}

async fn get_pot(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PotView>, ApiError> {
    let pot = require_pot(&state, &id)?;
    Ok(Json(pot.view()))
}

async fn create_pot(
    State(state): State<AppState>,
    Json(request): Json<CreatePotRequest>,
) -> Result<(StatusCode, Json<PotView>), ApiError> {
    let new_pot = pot::validate_new_pot(request, now_unix(), state.config.max_pot_sats)
        .map_err(ApiError::BadRequest)?;
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = now_unix();
    let stored = with_db(&state, |connection| {
        db::insert_pot(connection, &id, &new_pot, created_at)
    })?;
    Ok((StatusCode::CREATED, Json(stored.view())))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContributionBody {
    refund_pubkey: String,
    proofs: Vec<IncomingProof>,
}

/// The posted proof. A `witness` field is rejected here, before the cashu type
/// parses it. `C` and `dleq` stay out of the response.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IncomingProof {
    amount: u64,
    id: String,
    secret: String,
    #[serde(rename = "C")]
    c: String,
    dleq: IncomingDleq,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IncomingDleq {
    e: String,
    s: String,
    r: String,
}

async fn post_contribution(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ContributionBody>,
) -> Result<Json<PotView>, ApiError> {
    if body.proofs.is_empty() {
        return Err(reject(&id, "There is no proof."));
    }
    let pot = require_pot(&state, &id)?;
    if matches!(state.mint, MintBackend::Live) && state.config.mint_url.is_empty() {
        return Err(reject(&id, "Mint URL is not set."));
    }
    if now_unix() > pot.deadline_unix {
        return Err(reject(&id, "The deadline has passed."));
    }

    let locktime = pot.view().locktime_unix;
    let mut seen = HashSet::new();
    let mut sum = 0u64;
    for proof in &body.proofs {
        if !lock::contribution_lock_ok(
            state.config.server_pubkey.as_deref(),
            locktime,
            &body.refund_pubkey,
            &proof.secret,
        ) {
            return Err(reject(&id, "The secret is not this pot's lock."));
        }
        if proof.amount == 0 {
            return Err(reject(&id, "The proof amount is empty."));
        }
        if !seen.insert(proof.secret.clone()) {
            return Err(reject(&id, "This proof is already counted."));
        }
        sum = sum
            .checked_add(proof.amount)
            .ok_or_else(|| reject(&id, "That amount passes the target."))?;
    }
    if pot
        .locked_sats
        .checked_add(sum)
        .map(|total| total > pot.target_sats)
        .unwrap_or(true)
    {
        return Err(reject(&id, "That amount passes the target."));
    }
    for proof in &body.proofs {
        if with_db(&state, |connection| {
            db::secret_already_counted(connection, &proof.secret)
        })? {
            return Err(reject(&id, "This proof is already counted."));
        }
    }

    let proofs = parse_proofs(&body.proofs).map_err(|err| log_reject(&id, err))?;
    mint::check_proofs(&state.mint, &state.config.mint_url, &proofs)
        .await
        .map_err(|err| log_reject(&id, err))?;

    let mut rows = Vec::with_capacity(proofs.len());
    for (incoming, proof) in body.proofs.iter().zip(&proofs) {
        let stored = serde_json::to_string(proof).map_err(|_| {
            tracing::info!(pot_id = %id, "contribution proof could not be stored");
            ApiError::BadRequest(mint::SIGNED.to_string())
        })?;
        rows.push(NewContribution {
            amount_sats: incoming.amount,
            secret: incoming.secret.clone(),
            proof: stored,
        });
    }
    let stored = with_db_mut(&state, |connection| {
        db::insert_contributions(connection, &id, pot.target_sats, &rows, now_unix())?;
        db::get_pot(connection, &id)?.ok_or(ApiError::Database)
    })?;
    tracing::info!(
        pot_id = %id,
        proofs = rows.len(),
        sats = sum,
        "counted proofs"
    );
    Ok(Json(stored.view()))
}

fn reject(pot_id: &str, message: &str) -> ApiError {
    log_reject(pot_id, ApiError::BadRequest(message.to_string()))
}

fn log_reject(pot_id: &str, err: ApiError) -> ApiError {
    if let ApiError::BadRequest(message) = &err {
        tracing::info!(pot_id, error = %message, "contribution rejected");
    }
    err
}

fn parse_proofs(incoming: &[IncomingProof]) -> Result<Vec<cashu::Proof>, ApiError> {
    let mut proofs = Vec::with_capacity(incoming.len());
    for proof in incoming {
        let value = serde_json::json!({
            "amount": proof.amount,
            "id": proof.id,
            "secret": proof.secret,
            "C": proof.c,
            "dleq": {
                "e": proof.dleq.e,
                "s": proof.dleq.s,
                "r": proof.dleq.r,
            }
        });
        let parsed: cashu::Proof = serde_json::from_value(value).map_err(|_| {
            tracing::info!("contribution proof did not parse");
            ApiError::BadRequest(mint::SIGNED.to_string())
        })?;
        if parsed.amount.to_u64() != proof.amount {
            return Err(ApiError::BadRequest(mint::SIGNED.to_string()));
        }
        proofs.push(parsed);
    }
    Ok(proofs)
}

async fn payout_unimplemented(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(), ApiError> {
    refuse_money_route(&state, &id, "payout")
}

async fn refunds_unimplemented(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(), ApiError> {
    refuse_money_route(&state, &id, "refunds")
}

fn refuse_money_route(state: &AppState, id: &str, route: &'static str) -> Result<(), ApiError> {
    require_pot(state, id)?;
    Err(ApiError::NotImplemented(route))
}

async fn not_found() -> ApiError {
    ApiError::NotFound
}

fn require_pot(state: &AppState, id: &str) -> Result<crate::pot::StoredPot, ApiError> {
    with_db(state, |connection| {
        db::get_pot(connection, id)?.ok_or(ApiError::NotFound)
    })
}

fn with_db<T>(
    state: &AppState,
    f: impl FnOnce(&Connection) -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    let guard = state.db.lock().map_err(|_| ApiError::Database)?;
    f(&guard)
}

fn with_db_mut<T>(
    state: &AppState,
    f: impl FnOnce(&mut Connection) -> Result<T, ApiError>,
) -> Result<T, ApiError> {
    let mut guard = state.db.lock().map_err(|_| ApiError::Database)?;
    f(&mut guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mint::MintBackend;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_app() -> (Router, std::path::PathBuf) {
        test_app_with(None)
    }

    fn test_app_with(server_pubkey: Option<String>) -> (Router, std::path::PathBuf) {
        open_app(server_pubkey, MintBackend::Live, String::new())
    }

    fn open_app(
        server_pubkey: Option<String>,
        mint: MintBackend,
        mint_url: String,
    ) -> (Router, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("vaca-http-{}.sqlite", uuid::Uuid::new_v4()));
        let config = Config {
            bind: "127.0.0.1:9".parse().unwrap(),
            mint_url,
            db_path: path.clone(),
            max_pot_sats: 100_000,
            grace_seconds: 600,
            cors_origin: "http://127.0.0.1:5173".to_string(),
            experimental: true,
            server_key_path: None,
            server_pubkey,
        };
        let connection = db::open(&path).expect("db");
        let state = AppState {
            config: Arc::new(config),
            db: Arc::new(Mutex::new(connection)),
            mint,
        };
        (router(state), path)
    }

    async fn json_body(response: axum::response::Response) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn post_json(uri: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    #[tokio::test]
    async fn health_reports_policy_and_counts_contributions() {
        let (app, path) = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["ok"], true);
        assert_eq!(body["experimental"], true);
        assert_eq!(body["mint_url_set"], false);
        assert_eq!(body["max_pot_sats"], 100_000);
        assert_eq!(body["grace_seconds"], 600);
        assert_eq!(body["fee_reserve_recipient"], "organizer");
        assert_eq!(body["sigflag_v1"], "SIG_INPUTS");
        assert_eq!(body["sigflag_v2"], "SIG_ALL");
        assert_eq!(body["counts_contributions"], true);
        assert_eq!(body["server_pubkey_set"], false);
        assert_eq!(body["cdk_linked"], false);
        assert_eq!(body["cashu_ts"], "5.0.0-rc.12");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn create_list_and_get_leave_status_open() {
        let (app, path) = test_app();
        let created = app
            .clone()
            .oneshot(post_json(
                "/api/pots",
                serde_json::json!({
                    "title": "Shared pan",
                    "kind": "split_bill",
                    "target_sats": 5_000,
                    "deadline_unix": now_unix() + 7_200,
                    "destination": "lnbc50n1example",
                    "organizer_pubkey": ""
                }),
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let pot = json_body(created).await;
        assert_eq!(pot["status"], "open");
        assert_eq!(pot["contribution_count"], 0);
        assert_eq!(pot["locked_sats"], 0);
        assert_eq!(
            pot["locktime_unix"],
            pot["deadline_unix"].as_i64().unwrap() + 600
        );
        assert!(pot["organizer_pubkey"].is_null());
        let id = pot["id"].as_str().unwrap().to_string();

        let list = json_body(
            app.clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/pots")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(list["pots"].as_array().unwrap().len(), 1);

        let fetched = json_body(
            app.clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/pots/{id}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(fetched["title"], "Shared pan");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn money_routes_refuse_and_do_not_store_a_proof() {
        let (app, path) = test_app();
        let created = json_body(
            app.clone()
                .oneshot(post_json(
                    "/api/pots",
                    serde_json::json!({
                        "title": "Cause",
                        "kind": "cause",
                        "target_sats": 1,
                        "deadline_unix": now_unix() + 86_400,
                        "destination": "lno1qq"
                    }),
                ))
                .await
                .unwrap(),
        )
        .await;
        let id = created["id"].as_str().unwrap();
        let proofs = serde_json::json!({
            "proofs": [{"secret": "not-a-real-secret", "witness": {"signatures": ["aa"]}}]
        });
        for route in ["payout", "refunds"] {
            let response = app
                .clone()
                .oneshot(post_json(
                    &format!("/api/pots/{id}/{route}"),
                    proofs.clone(),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
            let body = json_body(response).await;
            assert_eq!(body["error"], "not_implemented");
            assert_eq!(body["route"], route);
            assert_eq!(body["owner"], "Y");
        }
        let rejected = app
            .clone()
            .oneshot(post_json(&format!("/api/pots/{id}/contributions"), proofs))
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let fetched = json_body(
            app.oneshot(
                Request::builder()
                    .uri(format!("/api/pots/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(fetched["contribution_count"], 0);
        assert_eq!(fetched["locked_sats"], 0);
        assert_eq!(fetched["status"], "open");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn contribution_rejects_a_bad_lock_and_stores_nothing() {
        let server = compressed("02", "11");
        let refund = compressed("03", "22");
        let (app, path) = open_app(
            Some(server.clone()),
            MintBackend::Scripted(crate::mint::ScriptedMint::Accept),
            "http://127.0.0.1:9".to_string(),
        );
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap();
        let locktime = created["locktime_unix"].as_i64().unwrap();
        let other = v1_secret(&compressed("02", "44"), &refund, locktime);
        let bad = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [loose_proof(&other, 4)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        let bad_body = json_body(bad).await;
        assert_eq!(bad_body["message"], "The secret is not this pot's lock.");
        assert!(!bad_body.to_string().contains(&other));

        let shape = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "secret": other,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(shape.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let missing = app
            .clone()
            .oneshot(post_json(
                "/api/pots/missing-pot/contributions",
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [loose_proof(&v1_secret(&server, &refund, locktime), 4)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let fetched = fetch_pot(&app, id).await;
        assert_eq!(fetched["contribution_count"], 0);
        assert_eq!(fetched["locked_sats"], 0);
        assert_eq!(fetched["status"], "open");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn contribution_counts_three_signed_proofs_and_stays_open() {
        let server = compressed("02", "11");
        let refund = compressed("03", "22");
        let (app, path) = open_app(
            Some(server.clone()),
            MintBackend::Scripted(crate::mint::ScriptedMint::Accept),
            "http://127.0.0.1:9".to_string(),
        );
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap();
        let locktime = created["locktime_unix"].as_i64().unwrap();
        let mut proofs = Vec::new();
        let mut secrets = Vec::new();
        for (nonce, amount) in [("64", 64u64), ("32", 32), ("04", 4)] {
            let secret = v1_secret_nonce(&server, &refund, locktime, nonce);
            proofs.push(signed_proof(&secret, amount));
            secrets.push(secret);
        }
        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": proofs,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        let text = body.to_string();
        assert_eq!(body["locked_sats"], 100);
        assert_eq!(body["contribution_count"], 3);
        assert_eq!(body["status"], "open");
        assert!(body.get("proofs").is_none());
        for secret in &secrets {
            assert!(!text.contains(secret));
        }
        for proof in &proofs {
            let c = proof["C"].as_str().unwrap();
            assert!(!text.contains(c));
            let r = proof["dleq"]["r"].as_str().unwrap();
            assert!(!text.contains(r));
        }

        let again = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": proofs,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(again.status(), StatusCode::BAD_REQUEST);
        let again_body = json_body(again).await;
        assert_eq!(again_body["message"], "This proof is already counted.");
        let fetched = fetch_pot(&app, id).await;
        assert_eq!(fetched["locked_sats"], 100);
        assert_eq!(fetched["contribution_count"], 3);
        assert_eq!(fetched["status"], "open");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn contribution_refuses_an_empty_mint_url_a_past_deadline_and_the_target() {
        let server = compressed("02", "ab");
        let refund = compressed("03", "cd");
        let (app, path) = test_app_with(Some(server.clone()));
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap();
        let locktime = created["locktime_unix"].as_i64().unwrap();
        let secret = v1_secret(&server, &refund, locktime);
        let empty_mint = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [loose_proof(&secret, 4)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(empty_mint.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(empty_mint).await["message"],
            "Mint URL is not set."
        );

        let (app, path2) = open_app(
            Some(server.clone()),
            MintBackend::Live,
            "http://127.0.0.1:9".to_string(),
        );
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap();
        let connection = rusqlite::Connection::open(&path2).unwrap();
        let changed = connection
            .execute(
                "UPDATE pots SET deadline_unix = ?1 WHERE id = ?2",
                rusqlite::params![1_700_000_000_i64, id],
            )
            .unwrap();
        assert_eq!(changed, 1);
        drop(connection);
        let late = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [loose_proof(&secret, 4)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(late.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(late).await["message"], "The deadline has passed.");

        let (app, path3) = open_app(
            Some(server.clone()),
            MintBackend::Scripted(crate::mint::ScriptedMint::Accept),
            "http://127.0.0.1:9".to_string(),
        );
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap();
        let locktime = created["locktime_unix"].as_i64().unwrap();
        let secret = v1_secret(&server, &refund, locktime);
        let over = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [loose_proof(&secret, 1_001)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(over.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(over).await["message"],
            "That amount passes the target."
        );
        let empty = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(empty).await["message"], "There is no proof.");
        let zero = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [loose_proof(&secret, 0)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(zero.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(zero).await["message"],
            "The proof amount is empty."
        );
        let fetched = fetch_pot(&app, id).await;
        assert_eq!(fetched["locked_sats"], 0);
        assert_eq!(fetched["contribution_count"], 0);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path2);
        let _ = std::fs::remove_file(path3);
    }

    #[tokio::test]
    async fn contribution_stores_nothing_when_the_mint_refuses() {
        let server = compressed("02", "11");
        let refund = compressed("03", "22");
        for scripted in [
            crate::mint::ScriptedMint::BadSignature,
            crate::mint::ScriptedMint::NotSpendable,
        ] {
            let (app, path) = open_app(
                Some(server.clone()),
                MintBackend::Scripted(scripted),
                "http://127.0.0.1:9".to_string(),
            );
            let created = create_cause(&app, 1_000).await;
            let id = created["id"].as_str().unwrap();
            let locktime = created["locktime_unix"].as_i64().unwrap();
            let secret = v1_secret_nonce(&server, &refund, locktime, "mint");
            let response = app
                .clone()
                .oneshot(post_json(
                    &format!("/api/pots/{id}/contributions"),
                    serde_json::json!({
                        "refund_pubkey": refund,
                        "proofs": [signed_proof(&secret, 4)],
                    }),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let message = json_body(response).await["message"]
                .as_str()
                .unwrap()
                .to_string();
            let expected = match scripted {
                crate::mint::ScriptedMint::BadSignature => crate::mint::SIGNED,
                crate::mint::ScriptedMint::NotSpendable => crate::mint::SPENDABLE,
                crate::mint::ScriptedMint::Accept => unreachable!(),
            };
            assert_eq!(message, expected);
            assert!(!message.contains(&secret));
            let fetched = fetch_pot(&app, id).await;
            assert_eq!(fetched["locked_sats"], 0);
            assert_eq!(fetched["contribution_count"], 0);
            let _ = std::fs::remove_file(path);
        }
    }

    #[tokio::test]
    async fn contribution_rejects_a_witness() {
        let (app, path) = test_app();
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap();
        let response = app
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": compressed("03", "22"),
                    "proofs": [{
                        "amount": 4,
                        "id": "0011111111111111",
                        "secret": "[]",
                        "C": "02",
                        "dleq": {"e": "aa", "s": "bb", "r": "cc"},
                        "witness": "not-accepted"
                    }],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let _ = std::fs::remove_file(path);
    }

    async fn create_cause(app: &Router, target_sats: u64) -> serde_json::Value {
        json_body(
            app.clone()
                .oneshot(post_json(
                    "/api/pots",
                    serde_json::json!({
                        "title": "Cause",
                        "kind": "cause",
                        "target_sats": target_sats,
                        "deadline_unix": now_unix() + 86_400,
                        "destination": "lno1qq"
                    }),
                ))
                .await
                .unwrap(),
        )
        .await
    }

    async fn fetch_pot(app: &Router, id: &str) -> serde_json::Value {
        json_body(
            app.clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/pots/{id}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await
    }

    fn compressed(prefix: &str, byte: &str) -> String {
        format!("{prefix}{}", byte.repeat(32))
    }

    fn v1_secret(server: &str, refund: &str, locktime: i64) -> String {
        v1_secret_nonce(server, refund, locktime, "aa")
    }

    fn v1_secret_nonce(server: &str, refund: &str, locktime: i64, nonce: &str) -> String {
        serde_json::json!([
            "P2PK",
            {
                "nonce": nonce,
                "data": server,
                "tags": [
                    ["locktime", locktime.to_string()],
                    ["refund", refund]
                ]
            }
        ])
        .to_string()
    }

    fn loose_proof(secret: &str, amount: u64) -> serde_json::Value {
        serde_json::json!({
            "amount": amount,
            "id": "0011111111111111",
            "secret": secret,
            "C": "02",
            "dleq": {"e": "aa", "s": "bb", "r": "cc"}
        })
    }

    fn signed_proof(secret: &str, amount: u64) -> serde_json::Value {
        let proof = crate::mint::sign_proof(secret, amount);
        serde_json::to_value(&proof).expect("proof json")
    }

    #[tokio::test]
    async fn rejects_a_pot_over_the_cap_and_an_unknown_field() {
        let (app, path) = test_app();
        let over = app
            .clone()
            .oneshot(post_json(
                "/api/pots",
                serde_json::json!({
                    "title": "Too big",
                    "kind": "cause",
                    "target_sats": 100_001,
                    "deadline_unix": now_unix() + 60,
                    "destination": "lno1qq"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(over.status(), StatusCode::BAD_REQUEST);

        let extra = app
            .oneshot(post_json(
                "/api/pots",
                serde_json::json!({
                    "title": "Sneaky",
                    "kind": "cause",
                    "target_sats": 10,
                    "deadline_unix": now_unix() + 60,
                    "destination": "lno1qq",
                    "proofs": []
                }),
            ))
            .await
            .unwrap();
        assert_eq!(extra.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let _ = std::fs::remove_file(path);
    }
}

use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::Connection;
use serde::Serialize;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::config::{
    Config, CASHU_TS_COMMIT, CASHU_TS_VERSION, CDK_GIT_REV, FEE_RESERVE_RECIPIENT, SIGFLAG_V1,
    SIGFLAG_V2,
};
use crate::db;
use crate::error::ApiError;
use crate::pot::{self, CreatePotRequest, PotView};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Arc<Mutex<Connection>>,
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
    /// Proofs are not counted. The contributions table stays empty.
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
        .route(
            "/api/pots/{id}/contributions",
            post(contributions_unimplemented),
        )
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
        counts_contributions: false,
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

async fn contributions_unimplemented(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(), ApiError> {
    refuse_money_route(&state, &id, "contributions")
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_app() -> (Router, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("vaca-http-{}.sqlite", uuid::Uuid::new_v4()));
        let config = Config {
            bind: "127.0.0.1:9".parse().unwrap(),
            mint_url: String::new(),
            db_path: path.clone(),
            max_pot_sats: 100_000,
            grace_seconds: 600,
            cors_origin: "http://127.0.0.1:5173".to_string(),
            experimental: true,
            server_key_path: None,
            server_pubkey: None,
        };
        let connection = db::open(&path).expect("db");
        let state = AppState {
            config: Arc::new(config),
            db: Arc::new(Mutex::new(connection)),
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
    async fn health_reports_policy_and_does_not_count_money() {
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
        assert_eq!(body["counts_contributions"], false);
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
        for route in ["contributions", "payout", "refunds"] {
            let response = app
                .clone()
                .oneshot(post_json(
                    &format!("/api/pots/{id}/{route}"),
                    serde_json::json!({
                        "proofs": [{"secret": "not-a-real-secret", "witness": {"signatures": ["aa"]}}]
                    }),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
            let body = json_body(response).await;
            assert_eq!(body["error"], "not_implemented");
            assert_eq!(body["route"], route);
            assert_eq!(body["owner"], "Y");
        }
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
        assert_eq!(fetched["status"], "open");
        let _ = std::fs::remove_file(path);
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

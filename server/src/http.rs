use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use axum::extract::{Path, State};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use cashu::SecretKey;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::config::{
    Config, CASHU_TS_COMMIT, CASHU_TS_VERSION, CDK_GIT_REV, FEE_RESERVE_RECIPIENT, SIGFLAG_V1,
    SIGFLAG_V2,
};
use crate::db::{self, NewContribution, StoredProof};
use crate::error::ApiError;
use crate::lock;
use crate::mint::{self, melt_quote, MeltQuote, MintBackend};
use crate::pot::{self, CreatePotRequest, PotView};

#[derive(Clone)]
pub struct SimpleCache {
    store: Arc<RwLock<HashMap<(String, String), MeltQuote>>>,
}

impl SimpleCache {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn set(&self, key: (String, String), value: MeltQuote) {
        self.store
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .insert(key, value);
    }

    pub fn get_fresh(&self, key: &(String, String), now_unix: u64) -> Option<MeltQuote> {
        let mut store = self.store.write().unwrap_or_else(|err| err.into_inner());
        match store.get(key).copied() {
            Some(quote) if quote.expiry_unix > now_unix => Some(quote),
            Some(_) => {
                store.remove(key);
                None
            }
            None => None,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Arc<Mutex<Connection>>,
    pub mint: MintBackend,
    pub quote_cache: SimpleCache,
    /// How many times payout asked the mint to pay. A paid pot does not add to it.
    pub melt_attempts: Arc<AtomicUsize>,
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
    /// A checked proof is stored. A refund returns that contributor's proofs after the lock time.
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
        .route("/api/pots/{id}/payout", post(payout))
        .route("/api/pots/{id}/refunds", post(post_refund))
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
    let mut pots: Vec<PotView> = vec![];
    for pot in with_db(&state, db::list_pots)?.into_iter() {
        let melt_quote_lookup = match state.quote_cache.get_fresh(
            &(pot.mint_url.clone(), pot.destination.clone()),
            now_unix() as u64,
        ) {
            Some(quoted) => mint::MeltQuoteLookup::Quoted(quoted),
            None => {
                melt_quote(
                    &state.mint,
                    &pot.mint_url,
                    &pot.destination,
                    pot.target_sats,
                    state.quote_cache.clone(),
                )
                .await
            }
        };
        let (view, _) = pot.view(&melt_quote_lookup);
        pots.push(show_server_pubkey(&state, view))
    }
    Ok(Json(PotList { pots }))
}

async fn get_pot(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PotView>, ApiError> {
    let pot = require_pot(&state, &id)?;
    let melt_quote_lookup = match state.quote_cache.get_fresh(
        &(pot.mint_url.clone(), pot.destination.clone()),
        now_unix() as u64,
    ) {
        Some(quoted) => mint::MeltQuoteLookup::Quoted(quoted),
        None => {
            melt_quote(
                &state.mint,
                &pot.mint_url,
                &pot.destination,
                pot.target_sats,
                state.quote_cache.clone(),
            )
            .await
        }
    };
    let (view, _) = pot.view(&melt_quote_lookup);
    Ok(Json(show_server_pubkey(&state, view)))
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
        db::insert_pot(
            connection,
            &id,
            &new_pot,
            &state.config.mint_url,
            created_at,
        )
    })?;
    let melt_quote_lookup = match state.quote_cache.get_fresh(
        &(stored.mint_url.clone(), stored.destination.clone()),
        now_unix() as u64,
    ) {
        Some(quoted) => mint::MeltQuoteLookup::Quoted(quoted),
        None => {
            melt_quote(
                &state.mint,
                &stored.mint_url,
                &stored.destination,
                stored.target_sats,
                state.quote_cache.clone(),
            )
            .await
        }
    };
    let (view, _) = stored.view(&melt_quote_lookup);
    Ok((StatusCode::CREATED, Json(show_server_pubkey(&state, view))))
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
    if pot.status == "paid" {
        return Err(reject(&id, "This pot is already paid."));
    }
    let melt_quote_lookup = match state.quote_cache.get_fresh(
        &(pot.mint_url.clone(), pot.destination.clone()),
        now_unix() as u64,
    ) {
        Some(quoted) => mint::MeltQuoteLookup::Quoted(quoted),
        None => {
            melt_quote(
                &state.mint,
                &pot.mint_url,
                &pot.destination,
                pot.target_sats,
                state.quote_cache.clone(),
            )
            .await
        }
    };
    if matches!(state.mint, MintBackend::Live) && pot.mint_url.is_empty() {
        return Err(reject(&id, "Mint URL is not set."));
    }
    if now_unix() > pot.deadline_unix {
        return Err(reject(&id, "The deadline has passed."));
    }

    let (preview, ceiling) = pot.view(&melt_quote_lookup);
    let locktime = preview.locktime_unix;
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
            .ok_or_else(|| reject(&id, "That amount passes the target plus the fee reserve."))?;
    }
    if pot
        .locked_sats
        .checked_add(sum)
        .map(|total| total > ceiling)
        .unwrap_or(true)
    {
        return Err(reject(
            &id,
            "That amount passes the target plus the fee reserve.",
        ));
    }
    for proof in &body.proofs {
        if with_db(&state, |connection| {
            db::secret_already_counted(connection, &proof.secret)
        })? {
            return Err(reject(&id, "This proof is already counted."));
        }
    }

    let proofs = parse_proofs(&body.proofs).map_err(|err| log_reject(&id, err))?;
    mint::check_proofs(&state.mint, &pot.mint_url, &proofs)
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
        db::insert_contributions(connection, &id, ceiling, &rows, now_unix())?;
        db::get_pot(connection, &id)?.ok_or(ApiError::Database)
    })?;
    tracing::info!(
        pot_id = %id,
        proofs = rows.len(),
        sats = sum,
        "counted proofs"
    );
    let (view, _) = stored.view(&melt_quote_lookup);
    Ok(Json(show_server_pubkey(&state, view)))
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

struct KeyPair {
    pubkey: String,
    privkey: String,
}

fn read_keys(path: &PathBuf) -> Result<KeyPair, ApiError> {
    let content = fs::read_to_string(path)
        .map_err(|_err| ApiError::BadRequest("The server key is not set.".to_string()))?;
    let mut pubkey = None;
    let mut privkey = None;

    for line in content.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
            match key {
                "pubkey" => pubkey = Some(value.to_string()),
                "privkey" => privkey = Some(value.to_string()),
                _ => {} // Ignore unknown keys or handle them
            }
        }
    }
    Ok(KeyPair {
        pubkey: pubkey
            .ok_or_else(|| ApiError::BadRequest("The server key is not set.".to_string()))?,
        privkey: privkey
            .ok_or_else(|| ApiError::BadRequest("The server key is not set.".to_string()))?,
    })
}

async fn payout(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PotView>, ApiError> {
    let pot = require_pot(&state, &id)?;
    if pot.status == "paid" {
        let (view, _) = pot.view(&mint::MeltQuoteLookup::Unavailable);
        return Ok(Json(show_server_pubkey(&state, view)));
    }
    let melt_quote_lookup = match state.quote_cache.get_fresh(
        &(pot.mint_url.clone(), pot.destination.clone()),
        now_unix() as u64,
    ) {
        Some(quoted) => mint::MeltQuoteLookup::Quoted(quoted),
        None => {
            melt_quote(
                &state.mint,
                &pot.mint_url,
                &pot.destination,
                pot.target_sats,
                state.quote_cache.clone(),
            )
            .await
        }
    };

    let (preview, _) = pot.view(&melt_quote_lookup);
    if !preview.payout_allowed {
        tracing::info!(pot_id = %id, "The pot has not reached the target plus the fee reserve.");
        return Err(ApiError::BadRequest(
            "The pot has not reached the target plus the fee reserve.".to_string(),
        ));
    }

    if now_unix() > pot.deadline_unix {
        return Err(reject(&id, "The deadline has passed."));
    }

    let Some(key_path) = &state.config.server_key_path else {
        return Err(ApiError::BadRequest(
            "The server key is not set.".to_string(),
        ));
    };
    let keys = read_keys(key_path)?;
    let parsed_privkey = SecretKey::from_str(&keys.privkey)
        .map_err(|_err| ApiError::BadRequest("The server key is not set.".to_string()))?;

    if Some(parsed_privkey.public_key().to_string()) != state.config.server_pubkey {
        return Err(ApiError::BadRequest(
            "The server key is not set.".to_string(),
        ));
    }
    if state.config.server_pubkey != Some(keys.pubkey) {
        return Err(ApiError::BadRequest(
            "The server key is not set.".to_string(),
        ));
    }
    let mut proofs: Vec<(StoredProof, cashu::Proof)> = vec![];
    for proof in with_db_mut(&state, |connection| db::proofs_for_pot(connection, &id))?.into_iter()
    {
        if proof.proof.is_empty() {
            return Err(ApiError::BadRequest(mint::SIGNED.to_string()));
        }
        let mut parsed: cashu::Proof = match serde_json::from_str::<cashu::Proof>(&proof.proof) {
            Ok(parsed_proof) => parsed_proof,
            Err(_) => return Err(ApiError::BadRequest(mint::SIGNED.to_string())),
        };
        match parsed.verify_p2pk() {
            Ok(_) => {
                proofs.push((proof, parsed));
            }
            Err(_) => {
                parsed
                    .sign_p2pk(parsed_privkey.clone())
                    .map_err(|_err| ApiError::BadRequest(mint::SIGNED.to_string()))?;
                parsed.verify_p2pk().map_err(|_err| {
                    ApiError::BadRequest(
                        "The server key does not match this pot's lock.".to_string(),
                    )
                })?;
                proofs.push((proof, parsed));
            }
        };
    }
    // Sign in memory first. The database changes only after every proof is signed,
    // and one transaction commits those rows together.
    let signed = proofs
        .iter()
        .map(|(stored, parsed)| {
            let proof_json = serde_json::to_string(parsed)
                .map_err(|_err| ApiError::BadRequest(mint::SIGNED.to_string()))?;
            Ok((stored.id.clone(), proof_json))
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    with_db_mut(&state, |connection| {
        db::update_proofs_for_pot(connection, &pot.id, &signed)
    })?;
    // The signatures are committed before the mint is asked. A failed payment
    // leaves them in place so a later payout can try again without signing twice.
    let parsed_proofs: Vec<cashu::Proof> = proofs.into_iter().map(|(_, parsed)| parsed).collect();
    state.melt_attempts.fetch_add(1, Ordering::SeqCst);
    let paid = mint::melt_proofs(
        &state.mint,
        &pot.mint_url,
        &pot.destination,
        pot.target_sats,
        pot.locked_sats,
        &parsed_proofs,
    )
    .await?;
    let change_rows = paid
        .change
        .iter()
        .map(|proof| {
            let amount: u64 = proof.amount.into();
            let proof_json = serde_json::to_string(proof)
                .map_err(|_| ApiError::BadRequest(mint::UNPAID.to_string()))?;
            Ok((amount, proof_json))
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    with_db_mut(&state, |connection| {
        db::finish_payout(connection, &pot.id, &change_rows)
    })?;
    let stored = require_pot(&state, &id)?;
    let (view, _) = stored.view(&mint::MeltQuoteLookup::Quoted(mint::MeltQuote {
        amount_sats: paid.amount_sats,
        fee_reserve_sats: paid.fee_reserve_sats,
        expiry_unix: u64::MAX,
    }));
    Ok(Json(show_server_pubkey(&state, view)))
}

/// Puts the configured spend key on a pot response. Health stays a boolean.
fn show_server_pubkey(state: &AppState, mut view: PotView) -> PotView {
    view.server_pubkey = state.config.server_pubkey.clone();
    view
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RefundBody {
    refund_pubkey: String,
}

#[derive(Serialize)]
struct RefundResponse {
    proofs: Vec<serde_json::Value>,
}

/// Returns the contributor's proofs after the mint lock time.
/// The server does not sign them and does not spend them. A witness already
/// stored from a payout attempt is left in the database and removed here,
/// because a refund signature cannot share a witness with the pot key.
async fn post_refund(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RefundBody>,
) -> Result<Json<RefundResponse>, ApiError> {
    let pot = require_pot(&state, &id)?;
    if pot.status == "paid" {
        return Err(reject(&id, "This pot is already paid."));
    }
    let locktime = pot
        .deadline_unix
        .saturating_add(crate::config::GRACE_SECONDS as i64);
    if now_unix() <= locktime {
        return Err(reject(&id, "The refund lock has not expired."));
    }
    if !lock::refund_pubkey_ok(&body.refund_pubkey) {
        return Err(reject(
            &id,
            "The refund key is not a compressed public key.",
        ));
    }
    if matches!(state.mint, MintBackend::Live) && pot.mint_url.is_empty() {
        return Err(reject(&id, "Mint URL is not set."));
    }
    let stored = with_db(&state, |connection| db::proofs_for_pot(connection, &pot.id))?;
    let mut proofs = Vec::new();
    for row in stored {
        let proof: cashu::Proof = serde_json::from_str(&row.proof)
            .map_err(|_| reject(&id, "The stored proof could not be read."))?;
        if lock::secret_refund_matches(&proof.secret.to_string(), &body.refund_pubkey) {
            proofs.push(proof);
        }
    }
    if proofs.is_empty() {
        return Err(reject(&id, "No proof is locked to that refund key."));
    }
    mint::check_proofs(&state.mint, &pot.mint_url, &proofs).await?;
    let mut returned = Vec::with_capacity(proofs.len());
    for mut proof in proofs {
        proof.witness = None;
        let value = serde_json::to_value(&proof)
            .map_err(|_| reject(&id, "The stored proof could not be read."))?;
        returned.push(value);
    }
    Ok(Json(RefundResponse { proofs: returned }))
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

    #[test]
    fn fresh_quote_is_reused_until_it_expires() {
        let cache = SimpleCache::new();
        let key = (
            "http://127.0.0.1:8085".to_string(),
            "lnbc1quote".to_string(),
        );
        cache.set(
            key.clone(),
            MeltQuote {
                amount_sats: 1_000,
                fee_reserve_sats: 20,
                expiry_unix: 100,
            },
        );
        let fresh = cache.get_fresh(&key, 50).expect("in date");
        assert_eq!(fresh.fee_reserve_sats, 20);
        assert_eq!(fresh.amount_sats, 1_000);
        assert!(cache.get_fresh(&key, 100).is_none());
        assert!(cache.get_fresh(&key, 50).is_none());
    }

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
        let quote_cache = SimpleCache::new();
        let state = AppState {
            config: Arc::new(config),
            db: Arc::new(Mutex::new(connection)),
            mint,
            quote_cache,
            melt_attempts: Arc::new(AtomicUsize::new(0)),
        };
        (router(state), path)
    }

    fn open_app_with_server_key(
        server_pubkey: String,
        key_path: PathBuf,
        mint: MintBackend,
        mint_url: String,
    ) -> (Router, std::path::PathBuf, Arc<AtomicUsize>) {
        let path = std::env::temp_dir().join(format!("vaca-http-{}.sqlite", uuid::Uuid::new_v4()));
        let config = Config {
            bind: "127.0.0.1:9".parse().unwrap(),
            mint_url,
            db_path: path.clone(),
            max_pot_sats: 100_000,
            grace_seconds: 600,
            cors_origin: "http://127.0.0.1:5173".to_string(),
            experimental: true,
            server_key_path: Some(key_path),
            server_pubkey: Some(server_pubkey),
        };
        let connection = db::open(&path).expect("db");
        let melt_attempts = Arc::new(AtomicUsize::new(0));
        let state = AppState {
            config: Arc::new(config),
            db: Arc::new(Mutex::new(connection)),
            mint,
            quote_cache: SimpleCache::new(),
            melt_attempts: melt_attempts.clone(),
        };
        (router(state), path, melt_attempts)
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
        assert!(body.get("server_pubkey").is_none());
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn pot_view_includes_the_server_pubkey_and_health_omits_it() {
        let (unset, unset_path) = test_app();
        let created = json_body(
            unset
                .clone()
                .oneshot(post_json(
                    "/api/pots",
                    serde_json::json!({
                        "title": "No key",
                        "kind": "cause",
                        "target_sats": 1_000,
                        "deadline_unix": now_unix() + 86_400,
                        "destination": "lnbc50n1example"
                    }),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(created.get("server_pubkey"), Some(&serde_json::Value::Null));
        let unset_health = json_body(
            unset
                .oneshot(
                    Request::builder()
                        .uri("/api/health")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(unset_health.get("server_pubkey").is_none());
        assert_eq!(unset_health["server_pubkey_set"], false);
        let _ = std::fs::remove_file(unset_path);

        let pubkey = format!("02{}", "ab".repeat(32));
        let (set, set_path) = test_app_with(Some(pubkey.clone()));
        let created = json_body(
            set.clone()
                .oneshot(post_json(
                    "/api/pots",
                    serde_json::json!({
                        "title": "With key",
                        "kind": "cause",
                        "target_sats": 1_000,
                        "deadline_unix": now_unix() + 86_400,
                        "destination": "lnbc50n1example"
                    }),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(created["server_pubkey"].as_str(), Some(pubkey.as_str()));
        let id = created["id"].as_str().unwrap();
        let fetched = json_body(
            set.clone()
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
        assert_eq!(fetched["server_pubkey"].as_str(), Some(pubkey.as_str()));
        let list = json_body(
            set.clone()
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
        assert_eq!(
            list["pots"][0]["server_pubkey"].as_str(),
            Some(pubkey.as_str())
        );
        let set_health = json_body(
            set.oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        )
        .await;
        assert!(set_health.get("server_pubkey").is_none());
        assert_eq!(set_health["server_pubkey_set"], true);
        let _ = std::fs::remove_file(set_path);
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
        assert_eq!(pot["mint_url"], "");
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
        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/refunds"),
                serde_json::json!({ "refund_pubkey": compressed("03", "22") }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json_body(response).await;
        assert_eq!(body["message"], "The refund lock has not expired.");
        assert!(body.get("proofs").is_none());

        let response = app
            .clone()
            .oneshot(post_json(&format!("/api/pots/{id}/payout"), proofs.clone()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json_body(response).await;
        assert_eq!(
            body["message"],
            "The pot has not reached the target plus the fee reserve."
        );
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
    async fn contribution_uses_the_mint_stored_on_the_pot() {
        let (app, path) = open_app(None, MintBackend::Live, "http://127.0.0.1:9".to_string());
        let created = create_cause(&app, 1_000).await;
        assert_eq!(created["mint_url"], "http://127.0.0.1:9");
        let id = created["id"].as_str().unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        let changed = connection
            .execute("UPDATE pots SET mint_url = '' WHERE id = ?1", [id])
            .unwrap();
        assert_eq!(changed, 1);
        drop(connection);
        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": "03",
                    "proofs": [loose_proof("not-this-pot", 1)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["message"], "Mint URL is not set.");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn contribution_requires_the_fee_reserve_and_does_not_pay_out() {
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
        let secret = v1_secret_nonce(&server, &refund, locktime, "1000");
        proofs.push(signed_proof(&secret, 1000));
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
        assert_eq!(body["locked_sats"], 1000);
        assert_eq!(body["fee_reserve_sats"], 1);
        assert_eq!(body["sats_still_needed"], 1);
        assert_eq!(body["payout_allowed"], false);
        assert_eq!(body["status"], "open");

        let again = create_cause(&app, 1_000).await;
        let id = again["id"].as_str().unwrap();
        let locktime = again["locktime_unix"].as_i64().unwrap();
        let mut proofs = Vec::new();
        let secret = v1_secret_nonce(&server, &refund, locktime, "1001");
        proofs.push(signed_proof(&secret, 1001));
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
        assert_eq!(body["locked_sats"], 1001);
        assert_eq!(body["sats_still_needed"], 0);
        assert_eq!(body["payout_allowed"], true);
        assert_eq!(body["status"], "open");

        let proofs = serde_json::json!({
            "proofs": [{"secret": "not-a-real-secret", "witness": {"signatures": ["aa"]}}]
        });
        let response = app
            .clone()
            .oneshot(post_json(&format!("/api/pots/{id}/payout"), proofs.clone()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json_body(response).await;
        assert_eq!(body["error"], "bad_request");
        assert_eq!(body["message"], "The server key is not set.");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn contribution_needs_the_quoted_fee_and_does_not_pay_out() {
        let server = compressed("02", "11");
        let refund = compressed("03", "22");
        let (app, path) = open_app(
            Some(server.clone()),
            MintBackend::Scripted(crate::mint::ScriptedMint::Fee(20)),
            "http://127.0.0.1:9".to_string(),
        );
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap();
        let locktime = created["locktime_unix"].as_i64().unwrap();
        let secret = v1_secret_nonce(&server, &refund, locktime, "1001");
        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [signed_proof(&secret, 1001)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["locked_sats"], 1001);
        assert_eq!(body["fee_reserve_sats"], 20);
        assert_eq!(body["quote_amount_sats"], 1000);
        assert_eq!(body["fee_quoted"], true);
        assert_eq!(body["sats_still_needed"], 19);
        assert_eq!(body["payout_allowed"], false);
        assert_eq!(body["status"], "open");

        let again = create_cause(&app, 1_000).await;
        let id = again["id"].as_str().unwrap();
        let locktime = again["locktime_unix"].as_i64().unwrap();
        let secret = v1_secret_nonce(&server, &refund, locktime, "1020");
        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [signed_proof(&secret, 1020)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["locked_sats"], 1020);
        assert_eq!(body["sats_still_needed"], 0);
        assert_eq!(body["payout_allowed"], true);
        assert_eq!(body["status"], "open");

        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/payout"),
                serde_json::json!({
                    "proofs": [{"secret": "not-a-real-secret", "witness": {"signatures": ["aa"]}}]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json_body(response).await;
        assert_eq!(body["message"], "The server key is not set.");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn payout_signs_each_stored_proof_once() {
        let key = SecretKey::generate();
        let server = key.public_key().to_string();
        let refund = compressed("03", "22");
        let key_path =
            std::env::temp_dir().join(format!("vaca-server-key-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(
            &key_path,
            format!("pubkey {server}\nprivkey {}\n", key.to_secret_hex()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let (app, path, melts) = open_app_with_server_key(
            server.clone(),
            key_path.clone(),
            MintBackend::Scripted(crate::mint::ScriptedMint::Accept),
            "http://127.0.0.1:9".to_string(),
        );
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap().to_string();
        let locktime = created["locktime_unix"].as_i64().unwrap();
        let secret = v1_secret_nonce(&server, &refund, locktime, "sign-1001");
        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [signed_proof(&secret, 1001)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/payout"),
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["status"], "paid");
        assert_eq!(body["payout_allowed"], false);
        assert_eq!(body["fee_reserve_sats"], 1);
        assert_eq!(body["change_sats"], 1);
        assert!(body.get("signatures").is_none());
        assert!(body.get("secret").is_none());
        assert!(body.get("proof").is_none());
        assert_eq!(signature_count(&path, &id), 1);
        assert_eq!(stored_change(&path, &id), (1, 1));
        assert_eq!(melts.load(Ordering::SeqCst), 1);
        let stored = fetch_pot(&app, &id).await;
        assert_eq!(stored["status"], "paid");
        assert_eq!(stored["payout_allowed"], false);

        let again = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/payout"),
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        assert_eq!(again.status(), StatusCode::OK);
        let again_body = json_body(again).await;
        assert_eq!(again_body["status"], "paid");
        assert_eq!(again_body["payout_allowed"], false);
        assert_eq!(again_body["change_sats"], 1);
        assert_eq!(signature_count(&path, &id), 1);
        assert_eq!(stored_change(&path, &id), (1, 1));
        assert_eq!(melts.load(Ordering::SeqCst), 1);

        let extra_secret = v1_secret_nonce(&server, &refund, locktime, "after-paid");
        let extra = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [signed_proof(&extra_secret, 1)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(extra.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(extra).await["message"],
            "This pot is already paid."
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&key_path);
    }

    #[tokio::test]
    async fn payout_refuses_a_funded_pot_after_the_deadline() {
        let key = SecretKey::generate();
        let server = key.public_key().to_string();
        let refund = compressed("03", "22");
        let key_path =
            std::env::temp_dir().join(format!("vaca-server-key-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(
            &key_path,
            format!("pubkey {server}\nprivkey {}\n", key.to_secret_hex()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let (app, path, _melts) = open_app_with_server_key(
            server.clone(),
            key_path.clone(),
            MintBackend::Scripted(crate::mint::ScriptedMint::Accept),
            "http://127.0.0.1:9".to_string(),
        );
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap().to_string();
        let locktime = created["locktime_unix"].as_i64().unwrap();
        let secret = v1_secret_nonce(&server, &refund, locktime, "late-1001");
        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [signed_proof(&secret, 1001)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let connection = rusqlite::Connection::open(&path).unwrap();
        let changed = connection
            .execute(
                "UPDATE pots SET deadline_unix = ?1 WHERE id = ?2",
                rusqlite::params![1_700_000_000_i64, id],
            )
            .unwrap();
        assert_eq!(changed, 1);
        drop(connection);

        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/payout"),
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(response).await["message"],
            "The deadline has passed."
        );
        assert_eq!(signature_count_without_verifying(&path, &id), 0);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&key_path);
    }

    #[tokio::test]
    async fn payout_stays_open_when_the_mint_does_not_pay() {
        let key = SecretKey::generate();
        let server = key.public_key().to_string();
        let refund = compressed("03", "22");
        let key_path =
            std::env::temp_dir().join(format!("vaca-server-key-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(
            &key_path,
            format!("pubkey {server}\nprivkey {}\n", key.to_secret_hex()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let (app, path, melts) = open_app_with_server_key(
            server.clone(),
            key_path.clone(),
            MintBackend::Scripted(crate::mint::ScriptedMint::MeltRejected),
            "http://127.0.0.1:9".to_string(),
        );
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap().to_string();
        let locktime = created["locktime_unix"].as_i64().unwrap();
        let secret = v1_secret_nonce(&server, &refund, locktime, "reject-1001");
        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [signed_proof(&secret, 1001)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/payout"),
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(response).await["message"],
            "The mint did not pay this pot."
        );
        assert_eq!(signature_count(&path, &id), 1);
        assert_eq!(melts.load(Ordering::SeqCst), 1);
        let stored = fetch_pot(&app, &id).await;
        assert_eq!(stored["status"], "open");
        assert_eq!(stored["payout_allowed"], true);
        assert_eq!(stored["change_sats"], 0);
        assert_eq!(stored_change(&path, &id), (0, 0));

        let again = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/payout"),
                serde_json::json!({}),
            ))
            .await
            .unwrap();
        assert_eq!(again.status(), StatusCode::BAD_REQUEST);
        assert_eq!(signature_count(&path, &id), 1);
        assert_eq!(melts.load(Ordering::SeqCst), 2);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&key_path);
    }

    fn stored_change(path: &std::path::Path, pot_id: &str) -> (i64, i64) {
        let connection = rusqlite::Connection::open(path).unwrap();
        connection
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(amount_sats), 0) FROM change_proofs WHERE pot_id = ?1",
                [pot_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }

    fn signature_count(path: &std::path::Path, pot_id: &str) -> usize {
        let connection = rusqlite::Connection::open(path).unwrap();
        let stored: String = connection
            .query_row(
                "SELECT proof FROM contributions WHERE pot_id = ?1",
                [pot_id],
                |row| row.get(0),
            )
            .unwrap();
        let proof: cashu::Proof = serde_json::from_str(&stored).unwrap();
        proof.verify_p2pk().expect("stored proof verifies");
        proof
            .witness
            .as_ref()
            .and_then(|witness| witness.signatures())
            .map(|signatures| signatures.len())
            .unwrap_or(0)
    }

    fn signature_count_without_verifying(path: &std::path::Path, pot_id: &str) -> usize {
        let connection = rusqlite::Connection::open(path).unwrap();
        let stored: String = connection
            .query_row(
                "SELECT proof FROM contributions WHERE pot_id = ?1",
                [pot_id],
                |row| row.get(0),
            )
            .unwrap();
        let proof: cashu::Proof = serde_json::from_str(&stored).unwrap();
        proof
            .witness
            .as_ref()
            .and_then(|witness| witness.signatures())
            .map(|signatures| signatures.len())
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn contribution_rejects_a_quote_whose_amount_is_not_the_target() {
        let server = compressed("02", "11");
        let refund = compressed("03", "22");
        let (app, path) = open_app(
            Some(server.clone()),
            MintBackend::Scripted(crate::mint::ScriptedMint::MismatchedAmount),
            "http://127.0.0.1:9".to_string(),
        );
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap();
        let locktime = created["locktime_unix"].as_i64().unwrap();
        let secret = v1_secret_nonce(&server, &refund, locktime, "match-1000");
        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [signed_proof(&secret, 1000)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["locked_sats"], 1000);
        assert_eq!(body["sats_still_needed"], 0);
        assert_eq!(body["payout_allowed"], false);
        assert_eq!(body["fee_quoted"], true);
        assert_eq!(body["fee_reserve_sats"], 1);
        assert_eq!(body["quote_amount_sats"], 1001);
        assert_eq!(body["status"], "open");

        let again = create_cause(&app, 1_000).await;
        let id = again["id"].as_str().unwrap();
        let locktime = again["locktime_unix"].as_i64().unwrap();
        let secret = v1_secret_nonce(&server, &refund, locktime, "match-1001");
        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/contributions"),
                serde_json::json!({
                    "refund_pubkey": refund,
                    "proofs": [signed_proof(&secret, 1001)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(response).await["message"],
            "That amount passes the target plus the fee reserve."
        );
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
                    "proofs": [loose_proof(&secret, 1_002)],
                }),
            ))
            .await
            .unwrap();
        assert_eq!(over.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(over).await["message"],
            "That amount passes the target plus the fee reserve."
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
                crate::mint::ScriptedMint::Fee(_) => unreachable!(),
                crate::mint::ScriptedMint::MismatchedAmount => unreachable!(),
                crate::mint::ScriptedMint::Accept | crate::mint::ScriptedMint::MeltRejected => {
                    unreachable!()
                }
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

    #[tokio::test]
    async fn refund_returns_the_matching_proof_after_the_lock() {
        let server = compressed("02", "11");
        let refund = compressed("03", "22");
        let other_refund = compressed("03", "44");
        let (app, path) = open_app(
            Some(server.clone()),
            MintBackend::Scripted(crate::mint::ScriptedMint::Accept),
            "http://127.0.0.1:9".to_string(),
        );
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap().to_string();
        let locktime = created["locktime_unix"].as_i64().unwrap();
        let secret = v1_secret_nonce(&server, &refund, locktime, "refund-4");
        let other = v1_secret_nonce(&server, &other_refund, locktime, "other-4");
        for (key, proof_secret, amount) in [
            (refund.as_str(), secret.as_str(), 4u64),
            (other_refund.as_str(), other.as_str(), 5u64),
        ] {
            let response = app
                .clone()
                .oneshot(post_json(
                    &format!("/api/pots/{id}/contributions"),
                    serde_json::json!({
                        "refund_pubkey": key,
                        "proofs": [signed_proof(proof_secret, amount)],
                    }),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let early = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/refunds"),
                serde_json::json!({ "refund_pubkey": refund }),
            ))
            .await
            .unwrap();
        assert_eq!(early.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(early).await["message"],
            "The refund lock has not expired."
        );

        let connection = rusqlite::Connection::open(&path).unwrap();
        let changed = connection
            .execute(
                "UPDATE pots SET deadline_unix = ?1 WHERE id = ?2",
                rusqlite::params![1_700_000_000_i64, id],
            )
            .unwrap();
        assert_eq!(changed, 1);
        let stored: String = connection
            .query_row(
                "SELECT proof FROM contributions WHERE secret = ?1",
                [secret.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        let mut signed: cashu::Proof = serde_json::from_str(&stored).unwrap();
        signed.sign_p2pk(SecretKey::generate()).unwrap();
        let signed_json = serde_json::to_string(&signed).unwrap();
        let updated = connection
            .execute(
                "UPDATE contributions SET proof = ?1 WHERE secret = ?2",
                rusqlite::params![signed_json, secret.as_str()],
            )
            .unwrap();
        assert_eq!(updated, 1);
        drop(connection);
        assert_eq!(witness_len(&path, &secret), 1);

        let flipped = format!("02{}", "22".repeat(32));
        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/refunds"),
                serde_json::json!({ "refund_pubkey": flipped }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        let proofs = body["proofs"].as_array().unwrap();
        assert_eq!(proofs.len(), 1);
        assert_eq!(proofs[0]["amount"], 4);
        assert_eq!(proofs[0]["secret"].as_str(), Some(secret.as_str()));
        assert!(proofs[0].get("witness").is_none() || proofs[0]["witness"].is_null());
        assert_eq!(witness_len(&path, &secret), 1);

        let missing = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/refunds"),
                serde_json::json!({ "refund_pubkey": compressed("03", "55") }),
            ))
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(missing).await["message"],
            "No proof is locked to that refund key."
        );

        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE pots SET status = 'paid' WHERE id = ?1",
                [id.as_str()],
            )
            .unwrap();
        drop(connection);
        let paid = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/refunds"),
                serde_json::json!({ "refund_pubkey": refund }),
            ))
            .await
            .unwrap();
        assert_eq!(paid.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(paid).await["message"],
            "This pot is already paid."
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn refund_refuses_a_proof_the_mint_will_not_spend() {
        let server = compressed("02", "11");
        let refund = compressed("03", "22");
        let (app, path) = open_app(
            Some(server.clone()),
            MintBackend::Scripted(crate::mint::ScriptedMint::NotSpendable),
            "http://127.0.0.1:9".to_string(),
        );
        let created = create_cause(&app, 1_000).await;
        let id = created["id"].as_str().unwrap().to_string();
        let locktime = created["locktime_unix"].as_i64().unwrap();
        let secret = v1_secret_nonce(&server, &refund, locktime, "spent-4");
        // The contribution route would refuse this mint, so the row is inserted
        // directly. The refund route is what asks the mint.
        let proof = signed_proof(&secret, 4);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute(
                "INSERT INTO contributions (
                    id, pot_id, created_at_unix, amount_sats, secret, proof
                ) VALUES (?1, ?2, ?3, 4, ?4, ?5)",
                rusqlite::params![
                    uuid::Uuid::new_v4().to_string(),
                    id.as_str(),
                    now_unix(),
                    secret.as_str(),
                    proof.to_string(),
                ],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE pots SET deadline_unix = ?1 WHERE id = ?2",
                rusqlite::params![1_700_000_000_i64, id],
            )
            .unwrap();
        drop(connection);
        let response = app
            .clone()
            .oneshot(post_json(
                &format!("/api/pots/{id}/refunds"),
                serde_json::json!({ "refund_pubkey": refund }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json_body(response).await;
        assert_eq!(body["message"], crate::mint::SPENDABLE);
        assert!(body.get("proofs").is_none());
        assert!(!body["message"].as_str().unwrap().contains(&secret));
        let _ = std::fs::remove_file(path);
    }

    fn witness_len(path: &std::path::Path, secret: &str) -> usize {
        let connection = rusqlite::Connection::open(path).unwrap();
        let stored: String = connection
            .query_row(
                "SELECT proof FROM contributions WHERE secret = ?1",
                [secret],
                |row| row.get(0),
            )
            .unwrap();
        let proof: cashu::Proof = serde_json::from_str(&stored).unwrap();
        proof
            .witness
            .as_ref()
            .and_then(|witness| witness.signatures())
            .map(|signatures| signatures.len())
            .unwrap_or(0)
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

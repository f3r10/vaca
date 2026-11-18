use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use cashu::nuts::{
    CheckStateRequest, CheckStateResponse, CurrencyUnit, KeySetInfo, KeysResponse, KeysetResponse,
    State,
};
use cashu::Proof;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::http::SimpleCache;

pub(crate) const SIGNED: &str = "The mint did not sign this proof.";
pub(crate) const SPENDABLE: &str = "The proof is not spendable.";
pub(crate) const UNPAID: &str = "The mint did not pay this pot.";
pub(crate) const PENDING: &str = "The mint has not finished this payment.";
pub(crate) const UNKNOWN_INPUT_FEE: &str = "The mint did not say what a proof costs.";
const UNREACHABLE: &str = "The mint could not be reached.";

/// Successful `input_fee_ppk` reads, keyed by the mint URL with no trailing slash.
/// A failed read is not stored, so the next request tries again.
#[derive(Clone)]
pub struct KeysetFeeCache {
    store: Arc<RwLock<HashMap<String, u64>>>,
}

impl KeysetFeeCache {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn get(&self, mint_url: &str) -> Option<u64> {
        self.store
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .get(mint_url)
            .copied()
    }

    fn set(&self, mint_url: &str, input_fee_ppk: u64) {
        self.store
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .insert(mint_url.to_string(), input_fee_ppk);
    }
}

/// NUT-02: ceil(proof_count * input_fee_ppk / 1000). Zero proofs cost nothing.
pub(crate) fn input_fee_sats(input_fee_ppk: u64, proof_count: u64) -> u64 {
    let product = u128::from(proof_count).saturating_mul(u128::from(input_fee_ppk));
    let fee = product.div_ceil(1000);
    u64::try_from(fee).unwrap_or(u64::MAX)
}

#[derive(Clone)]
pub enum MintBackend {
    Live,
    /// Offline stand-in. Tests use it so `cargo test` does not open a socket.
    Scripted(ScriptedMint),
}

#[derive(Clone, Copy)]
pub enum ScriptedMint {
    Accept,
    Fee(u64),
    MismatchedAmount,
    BadSignature,
    NotSpendable,
    /// The fee gate can open, and the melt then refuses. Tests use it.
    MeltRejected,
}

#[derive(Clone, Copy)]
pub struct MeltQuote {
    pub amount_sats: u64,
    pub fee_reserve_sats: u64,
    pub expiry_unix: u64,
}

pub enum MeltQuoteLookup {
    Quoted(MeltQuote),
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MeltQuoteRequest {
    request: String,
    unit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MeltQuoteResponse {
    amount: cashu::Amount,
    fee_reserve: cashu::Amount,
    state: cashu::nuts::MeltQuoteState,
    expiry: u64,
}

pub async fn check_proofs(
    backend: &MintBackend,
    mint_url: &str,
    proofs: &[Proof],
) -> Result<(), ApiError> {
    match backend {
        MintBackend::Scripted(ScriptedMint::Accept) => Ok(()),
        MintBackend::Scripted(ScriptedMint::Fee(_)) => Ok(()),
        MintBackend::Scripted(ScriptedMint::MismatchedAmount) => Ok(()),
        MintBackend::Scripted(ScriptedMint::BadSignature) => {
            Err(ApiError::BadRequest(SIGNED.to_string()))
        }
        MintBackend::Scripted(ScriptedMint::NotSpendable) => {
            Err(ApiError::BadRequest(SPENDABLE.to_string()))
        }
        MintBackend::Scripted(ScriptedMint::MeltRejected) => Ok(()),
        MintBackend::Live => live_check(mint_url, proofs).await,
    }
}

pub async fn melt_quote(
    backend: &MintBackend,
    mint_url: &str,
    destination: &str,
    target_sats: u64,
    quote_store: SimpleCache,
) -> MeltQuoteLookup {
    match backend {
        MintBackend::Scripted(kind) => MeltQuoteLookup::Quoted(scripted_quote(*kind, target_sats)),
        MintBackend::Live => {
            let Some(method) = lightning_method(destination) else {
                return MeltQuoteLookup::Unavailable;
            };
            if mint_url.is_empty() {
                return MeltQuoteLookup::Unavailable;
            }
            let client = match reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|err| {
                    tracing::error!(error = %err, "mint client");
                    ApiError::BadRequest(UNREACHABLE.to_string())
                }) {
                Ok(client) => client,
                Err(_) => return MeltQuoteLookup::Unavailable,
            };
            let base = mint_url.trim_end_matches('/');
            let url = format!("{base}/v1/melt/quote/{method}");
            let melt_quote_response: MeltQuoteResponse = match post_json(
                &client,
                &url,
                &MeltQuoteRequest {
                    request: destination.to_string(),
                    unit: "sat".to_string(),
                },
            )
            .await
            {
                Ok(result) => result,
                Err(_) => return MeltQuoteLookup::Unavailable,
            };
            match melt_quote_response.state {
                cashu::MeltQuoteState::Unpaid => {
                    let quote = MeltQuote {
                        amount_sats: melt_quote_response.amount.into(),
                        fee_reserve_sats: melt_quote_response.fee_reserve.into(),
                        expiry_unix: melt_quote_response.expiry,
                    };
                    quote_store.set((mint_url.to_string(), destination.to_string()), quote);
                    return MeltQuoteLookup::Quoted(quote);
                }
                cashu::MeltQuoteState::Paid => return MeltQuoteLookup::Unavailable,
                cashu::MeltQuoteState::Pending => return MeltQuoteLookup::Unavailable,
                cashu::MeltQuoteState::Unknown => return MeltQuoteLookup::Unavailable,
                cashu::MeltQuoteState::Failed => return MeltQuoteLookup::Unavailable,
            }
        }
    }
}

/// The quote the spend used, plus any NUT-08 change proofs.
/// The fee-gate cache does not keep the mint's quote id.
#[derive(Debug)]
pub struct PaidMelt {
    pub amount_sats: u64,
    pub fee_reserve_sats: u64,
    /// Unblinded change. Empty when the mint kept the whole reserve.
    pub change: Vec<Proof>,
}

/// Spends the signed proofs at the pot's mint. `Ok` means the mint reported `Paid`.
/// This asks for a new quote and does not read the fee-gate cache. Blank outputs
/// cover the sats locked above the invoice amount. The mint assigns the change.
pub async fn melt_proofs(
    backend: &MintBackend,
    mint_url: &str,
    destination: &str,
    target_sats: u64,
    locked_sats: u64,
    proofs: &[Proof],
) -> Result<PaidMelt, ApiError> {
    if proofs.is_empty() {
        return Err(ApiError::BadRequest(UNPAID.to_string()));
    }
    match backend {
        MintBackend::Scripted(ScriptedMint::MeltRejected) => {
            Err(ApiError::BadRequest(UNPAID.to_string()))
        }
        MintBackend::Scripted(kind) => {
            let quote = scripted_quote(*kind, target_sats);
            // The stand-in keyset charges no input fee. Live melts add the real one.
            covered(&quote, target_sats, locked_sats, 0)?;
            let change = scripted_change(locked_sats, quote.amount_sats)?;
            Ok(PaidMelt {
                amount_sats: quote.amount_sats,
                fee_reserve_sats: quote.fee_reserve_sats,
                change,
            })
        }
        MintBackend::Live => {
            live_melt(mint_url, destination, target_sats, locked_sats, proofs).await
        }
    }
}

fn scripted_quote(kind: ScriptedMint, target_sats: u64) -> MeltQuote {
    match kind {
        ScriptedMint::Accept
        | ScriptedMint::BadSignature
        | ScriptedMint::NotSpendable
        | ScriptedMint::MeltRejected => MeltQuote {
            amount_sats: target_sats,
            fee_reserve_sats: 1,
            expiry_unix: u64::MAX,
        },
        ScriptedMint::Fee(fee) => MeltQuote {
            amount_sats: target_sats,
            fee_reserve_sats: fee,
            expiry_unix: u64::MAX,
        },
        ScriptedMint::MismatchedAmount => MeltQuote {
            amount_sats: target_sats.saturating_add(1),
            fee_reserve_sats: 1,
            expiry_unix: u64::MAX,
        },
    }
}

fn lightning_method(destination: &str) -> Option<&'static str> {
    let destination_lower = destination.to_lowercase();
    if ["lnbc", "lntb", "lnbcrt"]
        .iter()
        .any(|prefix| destination_lower.starts_with(prefix))
    {
        Some("bolt11")
    } else if ["lno1", "lni1"]
        .iter()
        .any(|prefix| destination_lower.starts_with(prefix))
    {
        Some("bolt12")
    } else {
        None
    }
}

fn covered(
    quote: &MeltQuote,
    target_sats: u64,
    locked_sats: u64,
    input_fee_sats: u64,
) -> Result<(), ApiError> {
    let Some(needed) = quote
        .amount_sats
        .checked_add(quote.fee_reserve_sats)
        .and_then(|sum| sum.checked_add(input_fee_sats))
    else {
        return Err(ApiError::BadRequest(UNPAID.to_string()));
    };
    if quote.amount_sats == target_sats && locked_sats >= needed {
        Ok(())
    } else {
        Err(ApiError::BadRequest(UNPAID.to_string()))
    }
}

fn finished(state: cashu::MeltQuoteState) -> Result<(), ApiError> {
    match state {
        cashu::MeltQuoteState::Paid => Ok(()),
        cashu::MeltQuoteState::Pending => Err(ApiError::BadRequest(PENDING.to_string())),
        cashu::MeltQuoteState::Unpaid
        | cashu::MeltQuoteState::Unknown
        | cashu::MeltQuoteState::Failed => Err(ApiError::BadRequest(UNPAID.to_string())),
    }
}

#[derive(Debug, Deserialize)]
struct SpendQuoteResponse {
    quote: String,
    amount: cashu::Amount,
    fee_reserve: cashu::Amount,
    state: cashu::MeltQuoteState,
}

#[derive(Debug, Deserialize)]
struct MeltResponse {
    state: cashu::MeltQuoteState,
    #[serde(default)]
    change: Option<Vec<cashu::BlindSignature>>,
}

async fn live_melt(
    mint_url: &str,
    destination: &str,
    target_sats: u64,
    locked_sats: u64,
    proofs: &[Proof],
) -> Result<PaidMelt, ApiError> {
    let Some(method) = lightning_method(destination) else {
        return Err(ApiError::BadRequest(UNPAID.to_string()));
    };
    if mint_url.is_empty() {
        return Err(ApiError::BadRequest(UNPAID.to_string()));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|err| {
            tracing::error!(error = %err, "mint client");
            ApiError::BadRequest(UNPAID.to_string())
        })?;
    let base = mint_url.trim_end_matches('/');
    let quoted: SpendQuoteResponse = post_json(
        &client,
        &format!("{base}/v1/melt/quote/{method}"),
        &MeltQuoteRequest {
            request: destination.to_string(),
            unit: "sat".to_string(),
        },
    )
    .await
    .map_err(|_| ApiError::BadRequest(UNPAID.to_string()))?;
    if quoted.state == cashu::MeltQuoteState::Pending {
        return Err(ApiError::BadRequest(PENDING.to_string()));
    }
    if quoted.state != cashu::MeltQuoteState::Unpaid {
        return Err(ApiError::BadRequest(UNPAID.to_string()));
    }
    let quote = MeltQuote {
        amount_sats: quoted.amount.into(),
        fee_reserve_sats: quoted.fee_reserve.into(),
        expiry_unix: 0,
    };
    // Price the proofs before the mint is asked to pay. A short pot stays open
    // and the inputs are not posted. The fee is each proof's own keyset rate.
    let keysets = sat_keysets(&client, base).await?;
    let input_fee = proofs_input_fee(&keysets, proofs)?;
    covered(&quote, target_sats, locked_sats, input_fee)?;
    // Lightning may spend the whole fee reserve. The blank outputs cover the
    // maximum that can come back: the locked total minus the invoice amount.
    // The input fee is taken from the inputs and is not subtracted again.
    // Each blank has amount 0. The mint assigns the real amounts on the way back.
    let surplus = locked_sats
        .checked_sub(quote.amount_sats)
        .ok_or_else(|| ApiError::BadRequest(UNPAID.to_string()))?;
    let blanks = if surplus == 0 {
        None
    } else {
        let keyset_id = active_sat_keyset_id(&keysets)?;
        match cashu::nuts::PreMintSecrets::blank(keyset_id, cashu::Amount::from(surplus)) {
            Ok(premint) => Some(premint),
            Err(_) => return Err(ApiError::BadRequest(UNPAID.to_string())),
        }
    };
    // `MeltRequest::new` drops the DLEQ. cashu 0.18 already writes the P2PK
    // witness as a string of JSON, which is the shape CDK accepts.
    let request = cashu::MeltRequest::new(
        quoted.quote,
        proofs.to_vec(),
        blanks.as_ref().map(|premint| premint.blinded_messages()),
    );
    let paid: MeltResponse = post_json(&client, &format!("{base}/v1/melt/{method}"), &request)
        .await
        .map_err(|_| ApiError::BadRequest(UNPAID.to_string()))?;
    finished(paid.state)?;
    let change = match blanks {
        None => Vec::new(),
        Some(premint) => {
            unblind_change(&client, base, &premint, paid.change.unwrap_or_default()).await?
        }
    };
    Ok(PaidMelt {
        amount_sats: quote.amount_sats,
        fee_reserve_sats: quote.fee_reserve_sats,
        change,
    })
}

/// Active sat keyset rate. Scripted mints report 0 and do not open a socket.
/// A live failure is `None`: the caller must not invent a fee.
pub async fn active_sat_input_fee_ppk(
    backend: &MintBackend,
    mint_url: &str,
    cache: &KeysetFeeCache,
) -> Option<u64> {
    match backend {
        MintBackend::Scripted(_) => Some(0),
        MintBackend::Live => {
            let base = mint_url.trim_end_matches('/');
            if base.is_empty() {
                return None;
            }
            if let Some(ppk) = cache.get(base) {
                return Some(ppk);
            }
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .ok()?;
            let keysets: KeysetResponse = get_json(&client, &format!("{base}/v1/keysets"))
                .await
                .ok()?;
            let ppk = keysets
                .keysets
                .into_iter()
                .find(|keyset| keyset.active && keyset.unit == CurrencyUnit::Sat)?
                .input_fee_ppk;
            cache.set(base, ppk);
            Some(ppk)
        }
    }
}

async fn sat_keysets(client: &reqwest::Client, base: &str) -> Result<Vec<KeySetInfo>, ApiError> {
    let keysets: KeysetResponse = get_json(client, &format!("{base}/v1/keysets"))
        .await
        .map_err(|_| ApiError::BadRequest(UNPAID.to_string()))?;
    Ok(keysets.keysets)
}

fn active_sat_keyset_id(keysets: &[KeySetInfo]) -> Result<cashu::nuts::Id, ApiError> {
    keysets
        .iter()
        .find(|keyset| keyset.active && keyset.unit == CurrencyUnit::Sat)
        .map(|keyset| keyset.id)
        .ok_or_else(|| ApiError::BadRequest(UNPAID.to_string()))
}

/// Sum of each proof's keyset rate, rounded up. A proof whose keyset is not
/// listed fails the melt rather than being treated as free.
fn proofs_input_fee(keysets: &[KeySetInfo], proofs: &[Proof]) -> Result<u64, ApiError> {
    let mut sum = 0u128;
    for proof in proofs {
        let ppk = keysets
            .iter()
            .find(|keyset| keyset.id == proof.keyset_id)
            .map(|keyset| keyset.input_fee_ppk)
            .ok_or_else(|| ApiError::BadRequest(UNPAID.to_string()))?;
        sum = sum.saturating_add(u128::from(ppk));
    }
    u64::try_from(sum.div_ceil(1000)).map_err(|_| ApiError::BadRequest(UNPAID.to_string()))
}

async fn sat_keyset_keys(
    client: &reqwest::Client,
    base: &str,
    keyset_id: &cashu::nuts::Id,
) -> Result<cashu::nuts::Keys, ApiError> {
    let keys_body: KeysResponse = get_json(client, &format!("{base}/v1/keys/{keyset_id}"))
        .await
        .map_err(|_| ApiError::BadRequest(UNPAID.to_string()))?;
    keys_body
        .keysets
        .into_iter()
        .find(|keyset| &keyset.id == keyset_id)
        .map(|keyset| keyset.keys)
        .ok_or_else(|| ApiError::BadRequest(UNPAID.to_string()))
}

/// Keeps signatures the mint filled in. A zero amount is not spendable.
/// A longer list, or a signature from another keyset, fails the unblind.
fn keep_change(
    premint: &cashu::nuts::PreMintSecrets,
    signatures: Vec<cashu::BlindSignature>,
) -> Result<
    Vec<(
        cashu::BlindSignature,
        cashu::secret::Secret,
        cashu::nuts::SecretKey,
    )>,
    ApiError,
> {
    if signatures.is_empty() {
        return Ok(Vec::new());
    }
    let secrets = premint.secrets();
    let rs = premint.rs();
    if signatures.len() > secrets.len() || signatures.len() > rs.len() {
        return Err(ApiError::BadRequest(UNPAID.to_string()));
    }
    let mut kept = Vec::new();
    for (index, signature) in signatures.into_iter().enumerate() {
        if signature.keyset_id != premint.keyset_id {
            return Err(ApiError::BadRequest(UNPAID.to_string()));
        }
        if signature.amount == cashu::Amount::ZERO {
            continue;
        }
        kept.push((signature, secrets[index].clone(), rs[index].clone()));
    }
    Ok(kept)
}

async fn unblind_change(
    client: &reqwest::Client,
    base: &str,
    premint: &cashu::nuts::PreMintSecrets,
    signatures: Vec<cashu::BlindSignature>,
) -> Result<Vec<Proof>, ApiError> {
    let kept = keep_change(premint, signatures)?;
    if kept.is_empty() {
        return Ok(Vec::new());
    }
    let keys = sat_keyset_keys(client, base, &premint.keyset_id).await?;
    let mut promises = Vec::with_capacity(kept.len());
    let mut secrets = Vec::with_capacity(kept.len());
    let mut rs = Vec::with_capacity(kept.len());
    for (promise, secret, r) in kept {
        promises.push(promise);
        secrets.push(secret);
        rs.push(r);
    }
    cashu::dhke::construct_proofs(promises, rs, secrets, &keys).map_err(|_| {
        tracing::error!("change unblind failed");
        ApiError::BadRequest(UNPAID.to_string())
    })
}

/// The scripted mint pretends Lightning spent nothing, so the change is the
/// whole surplus. It does not open a socket. A zero surplus sends no outputs.
fn scripted_change(locked_sats: u64, amount_sats: u64) -> Result<Vec<Proof>, ApiError> {
    use cashu::dhke::{construct_proofs, sign_message};
    use cashu::nuts::{Id, Keys, PreMintSecrets, SecretKey};
    use cashu::{Amount, BlindSignature};
    use std::collections::BTreeMap;
    use std::str::FromStr;

    let surplus = locked_sats
        .checked_sub(amount_sats)
        .ok_or_else(|| ApiError::BadRequest(UNPAID.to_string()))?;
    if surplus == 0 {
        return Ok(Vec::new());
    }
    let keyset_id =
        Id::from_str("0011111111111111").map_err(|_| ApiError::BadRequest(UNPAID.to_string()))?;
    let premint = PreMintSecrets::blank(keyset_id, Amount::from(surplus))
        .map_err(|_| ApiError::BadRequest(UNPAID.to_string()))?;
    let amounts = change_denominations(surplus);
    if amounts.len() > premint.secrets().len() {
        return Err(ApiError::BadRequest(UNPAID.to_string()));
    }
    let mint_key = SecretKey::generate();
    let mint_pubkey = mint_key.public_key();
    let mut keys = BTreeMap::new();
    let mut promises = Vec::new();
    for (pre, amount) in premint.secrets.iter().zip(amounts) {
        let amount = Amount::from(amount);
        keys.insert(amount, mint_pubkey);
        let signed = sign_message(&mint_key, &pre.blinded_message.blinded_secret)
            .map_err(|_| ApiError::BadRequest(UNPAID.to_string()))?;
        let promise = BlindSignature::new(
            amount,
            signed,
            keyset_id,
            &pre.blinded_message.blinded_secret,
            &mint_key,
        )
        .map_err(|_| ApiError::BadRequest(UNPAID.to_string()))?;
        promises.push(promise);
    }
    let count = promises.len();
    let secrets = premint.secrets().into_iter().take(count).collect();
    let rs = premint.rs().into_iter().take(count).collect();
    let proofs = construct_proofs(promises, rs, secrets, &Keys::new(keys))
        .map_err(|_| ApiError::BadRequest(UNPAID.to_string()))?;
    for proof in &proofs {
        proof
            .verify_dleq(mint_pubkey)
            .map_err(|_| ApiError::BadRequest(UNPAID.to_string()))?;
    }
    Ok(proofs)
}

/// Powers of two that sum to `surplus`. The blank count is large enough to hold them.
fn change_denominations(surplus: u64) -> Vec<u64> {
    let mut amounts = Vec::new();
    let mut rest = surplus;
    let mut power = 1u64;
    while rest > 0 {
        if rest & 1 == 1 {
            amounts.push(power);
        }
        rest >>= 1;
        if rest > 0 {
            power = power.saturating_mul(2);
        }
    }
    amounts
}

async fn live_check(mint_url: &str, proofs: &[Proof]) -> Result<(), ApiError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|err| {
            tracing::error!(error = %err, "mint client");
            ApiError::BadRequest(UNREACHABLE.to_string())
        })?;
    let base = mint_url.trim_end_matches('/');
    let keysets: KeysetResponse = get_json(&client, &format!("{base}/v1/keysets")).await?;
    let active = keysets
        .keysets
        .into_iter()
        .find(|keyset| keyset.active && keyset.unit == CurrencyUnit::Sat)
        .ok_or_else(|| ApiError::BadRequest(SIGNED.to_string()))?;
    if proofs.iter().any(|proof| proof.keyset_id != active.id) {
        return Err(ApiError::BadRequest(SIGNED.to_string()));
    }
    let keys_body: KeysResponse =
        get_json(&client, &format!("{base}/v1/keys/{}", active.id)).await?;
    let keyset = keys_body
        .keysets
        .into_iter()
        .find(|keyset| keyset.id == active.id)
        .ok_or_else(|| ApiError::BadRequest(SIGNED.to_string()))?;
    let mut ys = Vec::with_capacity(proofs.len());
    for proof in proofs {
        let Some(pubkey) = keyset.keys.amount_key(proof.amount) else {
            return Err(ApiError::BadRequest(SIGNED.to_string()));
        };
        if proof.verify_dleq(pubkey).is_err() {
            return Err(ApiError::BadRequest(SIGNED.to_string()));
        }
        let y = proof.y().map_err(|err| {
            tracing::error!(error = %err, "proof y");
            ApiError::BadRequest(SIGNED.to_string())
        })?;
        ys.push(y);
    }
    let states: CheckStateResponse = post_json(
        &client,
        &format!("{base}/v1/checkstate"),
        &CheckStateRequest { ys: ys.clone() },
    )
    .await?;
    if states.states.len() != proofs.len() {
        return Err(ApiError::BadRequest(SPENDABLE.to_string()));
    }
    for (y, state) in ys.into_iter().zip(states.states) {
        if state.y != y || state.state != State::Unspent {
            return Err(ApiError::BadRequest(SPENDABLE.to_string()));
        }
    }
    Ok(())
}

async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
) -> Result<T, ApiError> {
    let response = client.get(url).send().await.map_err(unreachable)?;
    read_json(response).await
}

async fn post_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    body: &impl serde::Serialize,
) -> Result<T, ApiError> {
    let response = client
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(unreachable)?;
    read_json(response).await
}

async fn read_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, ApiError> {
    if !response.status().is_success() {
        tracing::error!(status = %response.status(), "mint request failed");
        return Err(ApiError::BadRequest(UNREACHABLE.to_string()));
    }
    response.json().await.map_err(|err| {
        tracing::error!(error = %err, "mint response was not the expected json");
        ApiError::BadRequest(UNREACHABLE.to_string())
    })
}

fn unreachable(err: reqwest::Error) -> ApiError {
    tracing::error!(error = %err, "mint request failed");
    ApiError::BadRequest(UNREACHABLE.to_string())
}

/// A proof the mint key for `amount` would produce. Tests use it. The secret
/// string is the one passed in, so a caller can lock it to a pot.
#[cfg(test)]
pub(crate) fn sign_proof(secret: &str, amount: u64) -> Proof {
    use cashu::dhke::{blind_message, construct_proofs, sign_message};
    use cashu::nuts::{Id, Keys, SecretKey};
    use cashu::secret::Secret;
    use cashu::{Amount, BlindSignature};
    use std::collections::BTreeMap;
    use std::str::FromStr;

    let mint_key = SecretKey::generate();
    let mint_pubkey = mint_key.public_key();
    let secret = Secret::new(secret);
    let (blinded, blinding) = blind_message(secret.as_bytes(), None).expect("blind");
    let signed = sign_message(&mint_key, &blinded).expect("sign");
    // Version byte `00` plus 14 hex digits. A shorter id is rejected.
    let keyset_id = Id::from_str("0011111111111111").expect("keyset id");
    let promise = BlindSignature::new(Amount::from(amount), signed, keyset_id, &blinded, &mint_key)
        .expect("dleq");
    let mut keys = BTreeMap::new();
    keys.insert(Amount::from(amount), mint_pubkey);
    let proofs = construct_proofs(
        vec![promise],
        vec![blinding],
        vec![secret],
        &Keys::new(keys),
    )
    .expect("proof");
    let proof = proofs.into_iter().next().expect("one proof");
    proof
        .verify_dleq(mint_pubkey)
        .expect("signed proof verifies");
    proof
}

#[cfg(test)]
mod tests {
    use super::*;
    use cashu::nuts::SecretKey;

    #[test]
    fn a_signed_proof_verifies_only_with_its_mint_key() {
        let mint_key = SecretKey::generate();
        let proof = sign_proof("pot-secret", 4);
        assert!(proof.verify_dleq(mint_key.public_key()).is_err());
    }

    #[tokio::test]
    async fn a_short_reserve_does_not_melt() {
        let proof = sign_proof("fee", 4);
        let err = melt_proofs(
            &MintBackend::Scripted(ScriptedMint::Fee(20)),
            "http://127.0.0.1:9",
            "lno1qq",
            1_000,
            1_001,
            &[proof],
        )
        .await
        .expect_err("fee");
        assert!(matches!(err, ApiError::BadRequest(message) if message == UNPAID));
    }

    #[tokio::test]
    async fn a_covered_scripted_quote_melts() {
        let proof = sign_proof("ok", 4);
        let paid = melt_proofs(
            &MintBackend::Scripted(ScriptedMint::Accept),
            "http://127.0.0.1:9",
            "lno1qq",
            1_000,
            1_001,
            &[proof],
        )
        .await
        .expect("paid");
        assert_eq!(paid.amount_sats, 1_000);
        assert_eq!(paid.fee_reserve_sats, 1);
        assert_eq!(paid.change.len(), 1);
        assert_eq!(u64::from(paid.change[0].amount), 1);
    }

    #[tokio::test]
    async fn a_zero_surplus_returns_no_change() {
        let proof = sign_proof("zero", 4);
        let paid = melt_proofs(
            &MintBackend::Scripted(ScriptedMint::Fee(0)),
            "http://127.0.0.1:9",
            "lno1qq",
            1_000,
            1_000,
            &[proof],
        )
        .await
        .expect("paid");
        assert!(paid.change.is_empty());
    }

    #[tokio::test]
    async fn a_three_sat_surplus_returns_two_proofs() {
        let proof = sign_proof("three", 4);
        let paid = melt_proofs(
            &MintBackend::Scripted(ScriptedMint::Fee(0)),
            "http://127.0.0.1:9",
            "lno1qq",
            1_000,
            1_003,
            &[proof],
        )
        .await
        .expect("paid");
        let amounts: Vec<u64> = paid
            .change
            .iter()
            .map(|proof| u64::from(proof.amount))
            .collect();
        assert_eq!(amounts, vec![1, 2]);
    }

    #[test]
    fn a_melt_request_names_the_quote_and_sends_no_change() {
        let mut proof = sign_proof("melt-secret", 4);
        proof.sign_p2pk(SecretKey::generate()).expect("sign");
        let request = cashu::MeltRequest::new("quote-1".to_string(), vec![proof], None);
        let json = serde_json::to_value(&request).expect("json");
        assert_eq!(json["quote"], "quote-1");
        assert!(json["outputs"].is_null());
        assert!(json["inputs"][0]["witness"].is_string());
        assert!(json["inputs"][0].get("dleq").is_none() || json["inputs"][0]["dleq"].is_null());
    }

    #[test]
    fn a_one_sat_surplus_sends_one_blank_output() {
        use cashu::nuts::{Id, PreMintSecrets};
        use cashu::Amount;
        use std::str::FromStr;

        let keyset_id = Id::from_str("0011111111111111").expect("keyset id");
        let blanks = PreMintSecrets::blank(keyset_id, Amount::from(1)).expect("blank");
        let mut proof = sign_proof("blank-secret", 4);
        proof.sign_p2pk(SecretKey::generate()).expect("sign");
        let request = cashu::MeltRequest::new(
            "quote-1".to_string(),
            vec![proof],
            Some(blanks.blinded_messages()),
        );
        let json = serde_json::to_value(&request).expect("json");
        let outputs = json["outputs"].as_array().expect("outputs");
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0]["amount"], 0);
    }

    #[test]
    fn pending_is_not_paid() {
        assert!(finished(cashu::MeltQuoteState::Paid).is_ok());
        let pending = finished(cashu::MeltQuoteState::Pending).expect_err("pending");
        assert!(matches!(pending, ApiError::BadRequest(message) if message == PENDING));
    }
}

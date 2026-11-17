use std::time::Duration;

use cashu::nuts::{
    CheckStateRequest, CheckStateResponse, CurrencyUnit, KeysResponse, KeysetResponse, State,
};
use cashu::Proof;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::http::SimpleCache;

pub(crate) const SIGNED: &str = "The mint did not sign this proof.";
pub(crate) const SPENDABLE: &str = "The proof is not spendable.";
pub(crate) const UNPAID: &str = "The mint did not pay this pot.";
pub(crate) const PENDING: &str = "The mint has not finished this payment.";
const UNREACHABLE: &str = "The mint could not be reached.";

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

/// The quote the spend used. The fee-gate cache does not keep the mint's quote id.
#[derive(Debug)]
pub struct PaidMelt {
    pub amount_sats: u64,
    pub fee_reserve_sats: u64,
}

/// Spends the signed proofs at the pot's mint. `Ok` means the mint reported `Paid`.
/// This asks for a new quote and does not read the fee-gate cache. No change outputs
/// are sent, so the mint keeps any unused reserve.
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
            covered(&quote, target_sats, locked_sats)?;
            Ok(PaidMelt {
                amount_sats: quote.amount_sats,
                fee_reserve_sats: quote.fee_reserve_sats,
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

fn covered(quote: &MeltQuote, target_sats: u64, locked_sats: u64) -> Result<(), ApiError> {
    let Some(needed) = quote.amount_sats.checked_add(quote.fee_reserve_sats) else {
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
    covered(&quote, target_sats, locked_sats)?;
    // `MeltRequest::new` drops the DLEQ. cashu 0.18 already writes the P2PK
    // witness as a string of JSON, which is the shape CDK accepts.
    let request = cashu::MeltRequest::new(quoted.quote, proofs.to_vec(), None);
    let paid: MeltResponse = post_json(&client, &format!("{base}/v1/melt/{method}"), &request)
        .await
        .map_err(|_| ApiError::BadRequest(UNPAID.to_string()))?;
    finished(paid.state)?;
    Ok(PaidMelt {
        amount_sats: quote.amount_sats,
        fee_reserve_sats: quote.fee_reserve_sats,
    })
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
    fn pending_is_not_paid() {
        assert!(finished(cashu::MeltQuoteState::Paid).is_ok());
        let pending = finished(cashu::MeltQuoteState::Pending).expect_err("pending");
        assert!(matches!(pending, ApiError::BadRequest(message) if message == PENDING));
    }
}

use std::time::Duration;

use cashu::nuts::{
    CheckStateRequest, CheckStateResponse, CurrencyUnit, KeysResponse, KeysetResponse, State,
};
use cashu::Proof;

use crate::error::ApiError;

pub(crate) const SIGNED: &str = "The mint did not sign this proof.";
pub(crate) const SPENDABLE: &str = "The proof is not spendable.";
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
    BadSignature,
    NotSpendable,
}

pub async fn check_proofs(
    backend: &MintBackend,
    mint_url: &str,
    proofs: &[Proof],
) -> Result<(), ApiError> {
    match backend {
        MintBackend::Scripted(ScriptedMint::Accept) => Ok(()),
        MintBackend::Scripted(ScriptedMint::BadSignature) => {
            Err(ApiError::BadRequest(SIGNED.to_string()))
        }
        MintBackend::Scripted(ScriptedMint::NotSpendable) => {
            Err(ApiError::BadRequest(SPENDABLE.to_string()))
        }
        MintBackend::Live => live_check(mint_url, proofs).await,
    }
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
}

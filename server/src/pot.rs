use serde::{Deserialize, Serialize};

use crate::{
    config::{
        FEE_RESERVE_RECIPIENT, GRACE_SECONDS, MELT_FEE_RESERVE_MIN_SATS, SIGFLAG_V1, SIGFLAG_V2,
    },
    mint,
};

pub const MAX_TITLE_CHARS: usize = 80;
pub const MAX_DESTINATION_CHARS: usize = 2048;
pub const MAX_DEADLINE_HORIZON_SECONDS: i64 = 366 * 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPot {
    pub title: String,
    pub kind: String,
    pub target_sats: u64,
    pub deadline_unix: i64,
    pub destination: String,
    pub organizer_pubkey: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePotRequest {
    pub title: String,
    pub kind: String,
    pub target_sats: u64,
    pub deadline_unix: i64,
    pub destination: String,
    #[serde(default)]
    pub organizer_pubkey: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PotView {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub target_sats: u64,
    pub deadline_unix: i64,
    pub destination: String,
    pub mint_url: String,
    pub organizer_pubkey: Option<String>,
    /// Compressed public key a contribution must lock to. Null when the
    /// server has no key configured. This is not the organizer key.
    pub server_pubkey: Option<String>,
    pub status: String,
    pub created_at_unix: i64,
    pub contribution_count: i64,
    pub locked_sats: u64,
    /// Display only. This is not a NUT-11 secret.
    pub locktime_unix: i64,
    /// The server stops co-signing here, which is the advertised deadline.
    pub co_sign_until_unix: i64,
    pub grace_seconds: u64,
    pub fee_reserve_recipient: &'static str,
    pub sigflag_v1: &'static str,
    pub sigflag_v2: &'static str,
    pub fee_reserve_sats: Option<u64>,
    /// NUT-02 rate of the mint's active sat keyset. Null when the mint did not say.
    pub input_fee_ppk: Option<u64>,
    /// Input fee for the proofs already counted. Null when the rate is unknown.
    pub input_fee_sats: Option<u64>,
    pub sats_still_needed: u64,
    pub payout_allowed: bool,
    pub fee_quoted: bool,
    pub quote_amount_sats: Option<u64>,
    /// Sum of stored NUT-08 change. The proof JSON is not on this view.
    pub change_sats: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredPot {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub target_sats: u64,
    pub deadline_unix: i64,
    pub destination: String,
    pub mint_url: String,
    pub organizer_pubkey: Option<String>,
    pub status: String,
    pub created_at_unix: i64,
    pub contribution_count: i64,
    pub locked_sats: u64,
    pub change_sats: u64,
}

/// What a melt of `proof_count` proofs has to hold: the invoice, the Lightning
/// reserve on the quote, and the NUT-02 input fee. A rate of 0 leaves the
/// cover at the invoice plus the Lightning reserve.
pub(crate) fn spend_cover(
    target_sats: u64,
    lightning_reserve_sats: u64,
    input_fee_ppk: u64,
    proof_count: u64,
) -> Option<u64> {
    target_sats
        .checked_add(lightning_reserve_sats)?
        .checked_add(mint::input_fee_sats(input_fee_ppk, proof_count))
}

/// The most a new post may bring the locked total to.
/// The spend cover uses the proof count after the post. cashu-ts `includeFees`
/// also adds about one input fee for the new proofs on top of the amount the
/// page asked to lock, so that extra fits too. A rate below 1000 cannot raise
/// the fee by more than the sats in the new proofs, each of which is at least 1.
pub(crate) fn accept_ceiling(
    target_sats: u64,
    lightning_reserve_sats: u64,
    input_fee_ppk: u64,
    existing_count: u64,
    new_count: u64,
) -> Option<u64> {
    let n_after = existing_count.checked_add(new_count)?;
    let cover = spend_cover(target_sats, lightning_reserve_sats, input_fee_ppk, n_after)?;
    cover.checked_add(mint::input_fee_sats(input_fee_ppk, new_count))
}

/// Ceiling passed to the contribution insert. A quote whose amount is not the
/// target stays at the target. No quote stays at the target plus one sat.
/// Neither of those paths can pay, so the input fee is not added there.
pub(crate) fn contribution_ceiling(
    target_sats: u64,
    lookup: &mint::MeltQuoteLookup,
    input_fee_ppk: u64,
    existing_count: u64,
    new_count: u64,
) -> u64 {
    match lookup {
        mint::MeltQuoteLookup::Quoted(quote) if quote.amount_sats == target_sats => accept_ceiling(
            target_sats,
            quote.fee_reserve_sats,
            input_fee_ppk,
            existing_count,
            new_count,
        )
        .unwrap_or(target_sats),
        mint::MeltQuoteLookup::Quoted(_) => target_sats,
        mint::MeltQuoteLookup::Unavailable => target_sats.saturating_add(MELT_FEE_RESERVE_MIN_SATS),
    }
}

impl StoredPot {
    // Example, with a zero input fee: the cover is 1001. sats_still_needed is
    // 901 when 100 are locked. payout_allowed is true once 1001 are locked.
    // A keyset that charges an input fee adds that fee to the same cover.
    // None means the mint did not say the rate. That never opens a payout.
    pub fn view(&self, lookup: &mint::MeltQuoteLookup, input_fee_ppk: Option<u64>) -> PotView {
        let count = u64::try_from(self.contribution_count).unwrap_or(0);
        let (shown_ppk, shown_fee) = match input_fee_ppk {
            Some(ppk) => (Some(ppk), Some(mint::input_fee_sats(ppk, count))),
            None => (None, None),
        };
        let mut sats_still_needed = self.target_sats.saturating_sub(self.locked_sats);
        let mut payout_allowed = false;
        let fee_quoted;
        let fee_reserve_sats;
        let quote_amount_sats;
        match lookup {
            mint::MeltQuoteLookup::Quoted(melt_quote) => {
                fee_quoted = true;
                fee_reserve_sats = Some(melt_quote.fee_reserve_sats);
                quote_amount_sats = Some(melt_quote.amount_sats);
                if melt_quote.amount_sats == self.target_sats {
                    if let Some(ppk) = input_fee_ppk {
                        if let Some(cover) =
                            spend_cover(self.target_sats, melt_quote.fee_reserve_sats, ppk, count)
                        {
                            payout_allowed = self.locked_sats >= cover;
                            sats_still_needed = cover.saturating_sub(self.locked_sats);
                        }
                    } else if let Some(lightning) =
                        self.target_sats.checked_add(melt_quote.fee_reserve_sats)
                    {
                        // The Lightning reserve alone is not enough to pay.
                        // Leave one sat showing so the pot does not look full.
                        let gap = lightning.saturating_sub(self.locked_sats);
                        sats_still_needed = gap.max(1);
                    }
                }
            }
            mint::MeltQuoteLookup::Unavailable => {
                fee_quoted = false;
                fee_reserve_sats = None;
                quote_amount_sats = None;
            }
        }
        // A paid pot has already spent its proofs. The fee numbers can still
        // describe the quote, but the route must not offer another payout.
        let payout_allowed = self.status == "open" && payout_allowed;
        PotView {
            id: self.id.clone(),
            title: self.title.clone(),
            kind: self.kind.clone(),
            target_sats: self.target_sats,
            deadline_unix: self.deadline_unix,
            destination: self.destination.clone(),
            mint_url: self.mint_url.clone(),
            organizer_pubkey: self.organizer_pubkey.clone(),
            // The row does not store this. The HTTP layer copies it from
            // the process config onto the response.
            server_pubkey: None,
            status: self.status.clone(),
            created_at_unix: self.created_at_unix,
            contribution_count: self.contribution_count,
            locked_sats: self.locked_sats,
            locktime_unix: self.deadline_unix.saturating_add(GRACE_SECONDS as i64),
            co_sign_until_unix: self.deadline_unix,
            grace_seconds: GRACE_SECONDS,
            fee_reserve_recipient: FEE_RESERVE_RECIPIENT,
            sigflag_v1: SIGFLAG_V1,
            sigflag_v2: SIGFLAG_V2,
            fee_reserve_sats,
            input_fee_ppk: shown_ppk,
            input_fee_sats: shown_fee,
            sats_still_needed,
            payout_allowed,
            fee_quoted,
            quote_amount_sats,
            change_sats: self.change_sats,
        }
    }
}

pub fn validate_new_pot(
    request: CreatePotRequest,
    now_unix: i64,
    max_pot_sats: u64,
) -> Result<NewPot, String> {
    let title = request.title.trim();
    if title.is_empty() {
        return Err("Title is empty.".to_string());
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(format!(
            "Title is longer than {MAX_TITLE_CHARS} characters."
        ));
    }
    if title.chars().any(|ch| ch.is_control()) {
        return Err("Title contains a control character.".to_string());
    }

    let kind = match request.kind.as_str() {
        "cause" | "purchase" | "split_bill" => request.kind,
        _ => return Err("Kind must be cause, purchase, or split_bill.".to_string()),
    };

    if request.target_sats == 0 || request.target_sats > max_pot_sats {
        return Err(format!("Target must be from 1 to {max_pot_sats} sats."));
    }

    if request.deadline_unix <= now_unix {
        return Err("Deadline must be in the future.".to_string());
    }
    if request.deadline_unix.saturating_sub(now_unix) > MAX_DEADLINE_HORIZON_SECONDS {
        return Err("Deadline is more than 366 days away.".to_string());
    }

    let destination = request.destination.trim();
    if destination.is_empty() {
        return Err("Destination is empty.".to_string());
    }
    if destination.chars().count() > MAX_DESTINATION_CHARS {
        return Err(format!(
            "Destination is longer than {MAX_DESTINATION_CHARS} characters."
        ));
    }
    if destination.chars().any(|ch| ch.is_control()) {
        return Err("Destination contains a control character.".to_string());
    }

    let organizer_pubkey = normalize_pubkey(request.organizer_pubkey)?;
    let _ = request
        .deadline_unix
        .checked_add(GRACE_SECONDS as i64)
        .ok_or_else(|| "Deadline plus grace does not fit.".to_string())?;

    Ok(NewPot {
        title: title.to_string(),
        kind,
        target_sats: request.target_sats,
        deadline_unix: request.deadline_unix,
        destination: destination.to_string(),
        organizer_pubkey,
    })
}

fn normalize_pubkey(raw: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let lowered = trimmed.to_ascii_lowercase();
    let bytes = lowered.as_bytes();
    let compressed = bytes.len() == 66
        && (bytes[0] == b'0')
        && (bytes[1] == b'2' || bytes[1] == b'3')
        && bytes[2..].iter().all(u8::is_ascii_hexdigit);
    if !compressed {
        return Err(
            "Organizer pubkey must be a 66-character compressed hex key (02 or 03, then 64 hex digits)."
                .to_string(),
        );
    }
    Ok(Some(lowered))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_762_200_000;

    fn request() -> CreatePotRequest {
        CreatePotRequest {
            title: " Milk for the trip ".to_string(),
            kind: "purchase".to_string(),
            target_sats: 21_000,
            deadline_unix: NOW + 86_400,
            destination: " lno1exampleoffer ".to_string(),
            organizer_pubkey: None,
        }
    }

    #[test]
    fn accepts_a_pot_and_computes_locktime_without_building_a_lock() {
        let pot = validate_new_pot(request(), NOW, 100_000).expect("valid");
        assert_eq!(pot.title, "Milk for the trip");
        assert_eq!(pot.destination, "lno1exampleoffer");
        assert!(pot.organizer_pubkey.is_none());
        let stored = StoredPot {
            id: "id".to_string(),
            title: pot.title,
            kind: pot.kind,
            target_sats: pot.target_sats,
            deadline_unix: pot.deadline_unix,
            destination: pot.destination,
            mint_url: String::new(),
            organizer_pubkey: pot.organizer_pubkey,
            status: "open".to_string(),
            created_at_unix: NOW,
            contribution_count: 0,
            locked_sats: 0,
            change_sats: 0,
        };

        let melt_quoted_lookup = mint::MeltQuoteLookup::Quoted(mint::MeltQuote {
            amount_sats: pot.target_sats,
            fee_reserve_sats: 1,
            expiry_unix: u64::MAX,
        });
        let view = stored.view(&melt_quoted_lookup, Some(0));
        assert_eq!(view.locked_sats, 0);
        assert_eq!(view.locktime_unix, view.deadline_unix + 600);
        assert_eq!(view.co_sign_until_unix, view.deadline_unix);
        assert_eq!(view.fee_reserve_recipient, "organizer");
        assert_eq!(view.sigflag_v1, "SIG_INPUTS");
        assert_eq!(view.sigflag_v2, "SIG_ALL");
        assert_eq!(view.status, "open");
        assert_eq!(view.fee_reserve_sats, Some(1));
        assert_eq!(view.input_fee_ppk, Some(0));
        assert_eq!(view.input_fee_sats, Some(0));
        assert_eq!(view.payout_allowed, false);
        assert_eq!(view.mint_url, "");
        assert_eq!(view.change_sats, 0);
    }

    #[test]
    fn input_fee_raises_the_amount_a_pot_must_hold() {
        // testnut's active sat keyset charges 100 ppk: 1 to 10 proofs cost 1 sat.
        assert_eq!(mint::input_fee_sats(100, 0), 0);
        assert_eq!(mint::input_fee_sats(100, 1), 1);
        assert_eq!(mint::input_fee_sats(100, 10), 1);
        assert_eq!(mint::input_fee_sats(100, 11), 2);
        assert_eq!(mint::input_fee_sats(0, 4), 0);

        let quote = mint::MeltQuoteLookup::Quoted(mint::MeltQuote {
            amount_sats: 10,
            fee_reserve_sats: 1,
            expiry_unix: u64::MAX,
        });
        let mut stored = StoredPot {
            id: "id".to_string(),
            title: "Fee".to_string(),
            kind: "cause".to_string(),
            target_sats: 10,
            deadline_unix: NOW + 86_400,
            destination: "lno1exampleoffer".to_string(),
            mint_url: "https://mint.example".to_string(),
            organizer_pubkey: None,
            status: "open".to_string(),
            created_at_unix: NOW,
            contribution_count: 0,
            locked_sats: 0,
            change_sats: 0,
        };
        let empty = stored.view(&quote, Some(100));
        assert_eq!(empty.input_fee_sats, Some(0));
        assert_eq!(empty.sats_still_needed, 11);
        assert!(!empty.payout_allowed);

        // Four proofs at 100 ppk add 1 sat. Eleven locked sats are one short.
        stored.contribution_count = 4;
        stored.locked_sats = 11;
        let short = stored.view(&quote, Some(100));
        assert_eq!(short.input_fee_sats, Some(1));
        assert_eq!(short.sats_still_needed, 1);
        assert!(!short.payout_allowed);

        stored.locked_sats = 12;
        let covered = stored.view(&quote, Some(100));
        assert_eq!(covered.sats_still_needed, 0);
        assert!(covered.payout_allowed);

        // A zero rate does not move the old ceiling: target plus the Lightning reserve.
        assert_eq!(contribution_ceiling(10, &quote, 0, 0, 4), 11);
        // The wallet's includeFees adds about one input fee for the new proofs.
        // The first fill of 12 fits. A post of 14 does not.
        assert_eq!(contribution_ceiling(10, &quote, 100, 0, 4), 13);
        let unknown = stored.view(&quote, None);
        assert_eq!(unknown.input_fee_ppk, None);
        assert_eq!(unknown.input_fee_sats, None);
        assert!(!unknown.payout_allowed);
    }

    #[test]
    fn rejects_the_cap_a_past_deadline_and_a_bad_key() {
        let mut over = request();
        over.target_sats = 100_001;
        assert!(validate_new_pot(over, NOW, 100_000).is_err());

        let mut past = request();
        past.deadline_unix = NOW;
        assert!(validate_new_pot(past, NOW, 100_000)
            .unwrap_err()
            .contains("future"));

        let mut key = request();
        key.organizer_pubkey = Some("02abcd".to_string());
        assert!(validate_new_pot(key, NOW, 100_000).is_err());

        let mut good_key = request();
        let pubkey = format!("02{}", "ab".repeat(32));
        good_key.organizer_pubkey = Some(pubkey.clone());
        let pot = validate_new_pot(good_key, NOW, 100_000).expect("key");
        assert_eq!(
            pot.organizer_pubkey.as_deref(),
            Some(pubkey.to_ascii_lowercase().as_str())
        );
    }

    #[test]
    fn rejects_unknown_kind_and_blank_destination() {
        let mut kind = request();
        kind.kind = "escrow".to_string();
        assert!(validate_new_pot(kind, NOW, 100_000).is_err());

        let mut destination = request();
        destination.destination = " \n ".to_string();
        assert!(validate_new_pot(destination, NOW, 100_000).is_err());
    }
}

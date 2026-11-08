use serde::{Deserialize, Serialize};

use crate::config::{
    melt_fee_reserve_sats, FEE_RESERVE_RECIPIENT, GRACE_SECONDS, SIGFLAG_V1, SIGFLAG_V2,
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
    pub status: String,
    pub created_at_unix: i64,
    pub contribution_count: i64,
    pub locked_sats: u64,
    /// Display only. This is not a NUT-11 secret.
    pub locktime_unix: i64,
    /// The server stops co-signing here. Co-signing is not implemented.
    pub co_sign_until_unix: i64,
    pub grace_seconds: u64,
    pub fee_reserve_recipient: &'static str,
    pub sigflag_v1: &'static str,
    pub sigflag_v2: &'static str,
    pub fee_reserve_sats: u64,
    pub sats_still_needed: u64,
    pub payout_allowed: bool,
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
}

impl StoredPot {
    pub fn view(&self) -> PotView {
        let fee_reserve_sats = melt_fee_reserve_sats(self.target_sats);
        let payout_sats = self.target_sats.saturating_add(fee_reserve_sats);
        let sats_still_needed = payout_sats - self.locked_sats;
        let payout_allowed = self.locked_sats >= payout_sats;
        PotView {
            id: self.id.clone(),
            title: self.title.clone(),
            kind: self.kind.clone(),
            target_sats: self.target_sats,
            deadline_unix: self.deadline_unix,
            destination: self.destination.clone(),
            mint_url: self.mint_url.clone(),
            organizer_pubkey: self.organizer_pubkey.clone(),
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
            sats_still_needed,
            payout_allowed,
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
        };
        let view = stored.view();
        assert_eq!(view.locked_sats, 0);
        assert_eq!(view.locktime_unix, view.deadline_unix + 600);
        assert_eq!(view.co_sign_until_unix, view.deadline_unix);
        assert_eq!(view.fee_reserve_recipient, "organizer");
        assert_eq!(view.sigflag_v1, "SIG_INPUTS");
        assert_eq!(view.sigflag_v2, "SIG_ALL");
        assert_eq!(view.status, "open");
        assert_eq!(view.fee_reserve_sats, 1);
        assert_eq!(view.payout_allowed, false);
        assert_eq!(view.mint_url, "");
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

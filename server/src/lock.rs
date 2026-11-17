use serde::Deserialize;

#[derive(Deserialize)]
struct SecretBody {
    nonce: String,
    data: String,
    #[serde(default)]
    tags: Vec<Vec<String>>,
}

/// True only for this pot's v1 lock.
///
/// `data` is the server key, `refund` is the contributor key, and `locktime`
/// is the pot's locktime. No other tag is allowed. `02` and `03` with the
/// same x-coordinate are the same key. An empty nonce or a parse failure is
/// a reject.
pub(crate) fn secret_matches_pot(
    server_pub_key: &str,
    refund_pubkey: &str,
    locktime_unix: u64,
    secret: &str,
) -> bool {
    let Ok((kind, body)) = serde_json::from_str::<(String, SecretBody)>(secret) else {
        return false;
    };
    if kind != "P2PK" || body.nonce.is_empty() {
        return false;
    }
    let Some(server_x) = pubkey_x(server_pub_key) else {
        return false;
    };
    let Some(refund_x) = pubkey_x(refund_pubkey) else {
        return false;
    };
    if pubkey_x(&body.data).as_deref() != Some(server_x.as_str()) {
        return false;
    }
    if body.tags.len() != 2 {
        return false;
    }
    let expected_locktime = locktime_unix.to_string();
    if single_tag(&body.tags, "locktime").as_deref() != Some(expected_locktime.as_str()) {
        return false;
    }
    let Some(refund) = single_tag(&body.tags, "refund") else {
        return false;
    };
    pubkey_x(&refund).as_deref() == Some(refund_x.as_str())
}

/// The refund tag matches this key. `02` and `03` with the same x-coordinate match.
/// This does not require the server key, so a refund can be prepared when that key is gone.
pub(crate) fn secret_refund_matches(secret: &str, refund_pubkey: &str) -> bool {
    let Some(wanted) = pubkey_x(refund_pubkey) else {
        return false;
    };
    let Ok((kind, body)) = serde_json::from_str::<(String, SecretBody)>(secret) else {
        return false;
    };
    if kind != "P2PK" {
        return false;
    }
    let Some(refund) = single_tag(&body.tags, "refund") else {
        return false;
    };
    pubkey_x(&refund).as_deref() == Some(wanted.as_str())
}

pub(crate) fn refund_pubkey_ok(pubkey: &str) -> bool {
    pubkey_x(pubkey).is_some()
}

pub(crate) fn contribution_lock_ok(
    server_pubkey: Option<&str>,
    pot_locktime_unix: i64,
    refund_pubkey: &str,
    secret: &str,
) -> bool {
    let Some(server_pubkey) = server_pubkey else {
        return false;
    };
    let Ok(locktime) = u64::try_from(pot_locktime_unix) else {
        return false;
    };
    secret_matches_pot(server_pubkey, refund_pubkey, locktime, secret)
}

fn pubkey_x(pubkey: &str) -> Option<String> {
    let trimmed = pubkey.trim().to_ascii_lowercase();
    let bytes = trimmed.as_bytes();
    let compressed = bytes.len() == 66
        && bytes[0] == b'0'
        && (bytes[1] == b'2' || bytes[1] == b'3')
        && bytes[2..].iter().all(u8::is_ascii_hexdigit);
    if !compressed {
        return None;
    }
    Some(trimmed[2..].to_string())
}

fn single_tag(tags: &[Vec<String>], name: &str) -> Option<String> {
    let mut found: Option<&Vec<String>> = None;
    for tag in tags {
        if tag.first().map(String::as_str) != Some(name) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(tag);
    }
    let tag = found?;
    if tag.len() != 2 || tag[1].is_empty() {
        return None;
    }
    Some(tag[1].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCKTIME: u64 = 1_800_000_000;

    fn key(prefix: &str, byte: &str) -> String {
        format!("{prefix}{}", byte.repeat(32))
    }

    fn p2pk(data: &str, nonce: &str, tags: &[Vec<String>]) -> String {
        serde_json::json!(["P2PK", { "nonce": nonce, "data": data, "tags": tags }]).to_string()
    }

    fn tag(name: &str, value: &str) -> Vec<String> {
        vec![name.to_string(), value.to_string()]
    }

    #[test]
    fn accepts_this_pots_v1_lock() {
        let server = key("02", "11");
        let refund = key("03", "22");
        let secret = p2pk(
            &server,
            "aa",
            &[tag("locktime", "1800000000"), tag("refund", &refund)],
        );
        assert!(secret_matches_pot(&server, &refund, LOCKTIME, &secret));
        assert!(secret_refund_matches(&secret, &refund));
        let flipped = format!("02{}", "22".repeat(32));
        assert!(secret_refund_matches(&secret, &flipped));
        assert!(!secret_refund_matches(&secret, &key("03", "44")));

        let flipped = p2pk(
            &key("03", "11"),
            "bb",
            &[
                tag("refund", &key("02", "22")),
                tag("locktime", "1800000000"),
            ],
        );
        assert!(secret_matches_pot(
            &format!("  {}  ", server.to_ascii_uppercase()),
            &refund,
            LOCKTIME,
            &flipped
        ));
    }

    #[test]
    fn contribution_lock_uses_the_configured_key_and_the_pot_locktime() {
        let server = key("02", "11");
        let other = key("02", "44");
        let refund = key("03", "22");
        let secret = p2pk(
            &server,
            "aa",
            &[tag("locktime", "1800000000"), tag("refund", &refund)],
        );
        let locked_to_other = p2pk(
            &other,
            "aa",
            &[tag("locktime", "1800000000"), tag("refund", &refund)],
        );
        let other_locktime = p2pk(
            &server,
            "aa",
            &[tag("locktime", "1800000001"), tag("refund", &refund)],
        );

        assert!(!contribution_lock_ok(
            None,
            LOCKTIME as i64,
            &refund,
            &secret
        ));
        assert!(!contribution_lock_ok(
            Some(&server),
            LOCKTIME as i64,
            &refund,
            &locked_to_other
        ));
        assert!(!contribution_lock_ok(
            Some(&server),
            LOCKTIME as i64,
            &refund,
            &other_locktime
        ));
        assert!(!contribution_lock_ok(Some(&server), -1, &refund, &secret));
        assert!(contribution_lock_ok(
            Some(&server),
            LOCKTIME as i64,
            &refund,
            &secret
        ));
    }

    #[test]
    fn rejects_a_secret_that_is_not_this_pots_lock() {
        let server = key("02", "11");
        let refund = key("03", "22");
        let locktime = tag("locktime", "1800000000");
        let refund_tag = tag("refund", &refund);
        let good_tags = [locktime.clone(), refund_tag.clone()];

        let cases = [
            ("wrong data", p2pk(&key("02", "44"), "aa", &good_tags)),
            (
                "wrong refund",
                p2pk(
                    &server,
                    "aa",
                    &[locktime.clone(), tag("refund", &key("02", "55"))],
                ),
            ),
            (
                "wrong locktime",
                p2pk(
                    &server,
                    "aa",
                    &[tag("locktime", "1800000001"), refund_tag.clone()],
                ),
            ),
            ("missing refund", p2pk(&server, "aa", &[locktime.clone()])),
            (
                "extra pubkey",
                p2pk(
                    &server,
                    "aa",
                    &[
                        locktime.clone(),
                        refund_tag.clone(),
                        tag("pubkeys", &key("02", "33")),
                    ],
                ),
            ),
            (
                "sig all",
                p2pk(
                    &server,
                    "aa",
                    &[
                        locktime.clone(),
                        refund_tag.clone(),
                        tag("sigflag", "SIG_ALL"),
                    ],
                ),
            ),
            (
                "explicit sig inputs",
                p2pk(
                    &server,
                    "aa",
                    &[
                        locktime.clone(),
                        refund_tag.clone(),
                        tag("sigflag", "SIG_INPUTS"),
                    ],
                ),
            ),
            ("not a secret", "nope".to_string()),
            (
                "empty nonce",
                p2pk(&server, "", &[locktime.clone(), refund_tag.clone()]),
            ),
            (
                "numeric locktime",
                format!(
                    r#"["P2PK",{{"nonce":"aa","data":"{server}","tags":[["locktime",{LOCKTIME}],["refund","{refund}"]]}}]"#
                ),
            ),
            (
                "leading zero locktime",
                p2pk(
                    &server,
                    "aa",
                    &[tag("locktime", "01800000000"), refund_tag.clone()],
                ),
            ),
            (
                "extra locktime element",
                p2pk(
                    &server,
                    "aa",
                    &[
                        vec![
                            "locktime".to_string(),
                            "1800000000".to_string(),
                            "extra".to_string(),
                        ],
                        refund_tag,
                    ],
                ),
            ),
        ];

        for (name, secret) in cases {
            assert!(
                !secret_matches_pot(&server, &refund, LOCKTIME, &secret),
                "{name}"
            );
        }
        let good = p2pk(&server, "aa", &good_tags);
        assert!(!secret_matches_pot("02abcd", &refund, LOCKTIME, &good));
    }
}

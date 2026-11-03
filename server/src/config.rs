use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

/// Confirmed demo cap. Environment cannot raise it.
pub const MAX_POT_SATS: u64 = 100_000;

/// Confirmed demo grace: advertised deadline plus 10 minutes.
pub const GRACE_SECONDS: u64 = 600;

/// CDK `origin/main` to pin when the money path is added.
/// This binary does not depend on CDK.
pub const CDK_GIT_REV: &str = "24360fe5fabda9d8aa3ae98b1a7e5e15986aad5a";

/// cashu-ts release used by the browser. The server does not call it.
pub const CASHU_TS_VERSION: &str = "5.0.0-rc.12";

/// Git commit of the `v5.0.0-rc.12` tag. `dd8bbe3` is a later main commit.
pub const CASHU_TS_COMMIT: &str = "4d6e1f86aaae0627bd4b06f4497da9fce0abe37b";

pub const FEE_RESERVE_RECIPIENT: &str = "organizer";
pub const SIGFLAG_V1: &str = "SIG_INPUTS";
pub const SIGFLAG_V2: &str = "SIG_ALL";

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub mint_url: String,
    pub db_path: PathBuf,
    pub max_pot_sats: u64,
    pub grace_seconds: u64,
    pub cors_origin: String,
    pub experimental: bool,
    /// Location of the server key. The file is never opened or logged.
    pub server_key_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerKeyStatus {
    pub path_set: bool,
    pub file_present: bool,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let bind = env_string("VACA_BIND", "127.0.0.1:8787")?
            .parse::<SocketAddr>()
            .map_err(|_| "VACA_BIND must be host:port, for example 127.0.0.1:8787".to_string())?;
        let mint_url = env_string("VACA_MINT_URL", "")?;
        validate_mint_url(&mint_url)?;
        let db_path = PathBuf::from(env_string("VACA_DB_PATH", "data/vaca.sqlite")?);
        let max_pot_sats = env_u64("VACA_MAX_POT_SATS", MAX_POT_SATS)?;
        if max_pot_sats == 0 || max_pot_sats > MAX_POT_SATS {
            return Err(format!(
                "VACA_MAX_POT_SATS must be from 1 to {MAX_POT_SATS}"
            ));
        }
        let grace_seconds = env_u64("VACA_GRACE_SECONDS", GRACE_SECONDS)?;
        if grace_seconds != GRACE_SECONDS {
            return Err(format!(
                "VACA_GRACE_SECONDS must be {GRACE_SECONDS} (the advertised deadline plus 10 minutes)"
            ));
        }
        let cors_origin = env_string("VACA_CORS_ORIGIN", "http://127.0.0.1:5173")?;
        if cors_origin == "*"
            || !(cors_origin.starts_with("http://") || cors_origin.starts_with("https://"))
        {
            return Err(
                "VACA_CORS_ORIGIN must be one http(s) origin, for example http://127.0.0.1:5173"
                    .to_string(),
            );
        }
        let experimental = env_bool("VACA_EXPERIMENTAL", true)?;
        let server_key_path = match env::var("VACA_SERVER_KEY_PATH") {
            Ok(value) if !value.trim().is_empty() => Some(PathBuf::from(value)),
            _ => None,
        };
        Ok(Config {
            bind,
            mint_url,
            db_path,
            max_pot_sats,
            grace_seconds,
            cors_origin,
            experimental,
            server_key_path,
        })
    }

    /// Metadata only. Does not open the key file and does not log the path.
    pub fn server_key_status(&self) -> ServerKeyStatus {
        let Some(path) = &self.server_key_path else {
            return ServerKeyStatus {
                path_set: false,
                file_present: false,
            };
        };
        let file_present = std::fs::metadata(path)
            .map(|meta| meta.is_file())
            .unwrap_or(false);
        ServerKeyStatus {
            path_set: true,
            file_present,
        }
    }
}

fn validate_mint_url(mint_url: &str) -> Result<(), String> {
    if mint_url.is_empty() {
        return Ok(());
    }
    if !(mint_url.starts_with("http://") || mint_url.starts_with("https://")) {
        return Err("VACA_MINT_URL must be empty or an http(s) URL".to_string());
    }
    if mint_url.contains('@') {
        return Err("VACA_MINT_URL must not include credentials".to_string());
    }
    Ok(())
}

fn env_string(name: &str, default: &str) -> Result<String, String> {
    match env::var(name) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.to_string()),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{name} is not valid Unicode")),
    }
}

fn env_u64(name: &str, default: u64) -> Result<u64, String> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|_| format!("{name} must be a non-negative integer")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{name} is not valid Unicode")),
    }
}

fn env_bool(name: &str, default: bool) -> Result<bool, String> {
    match env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Ok(true),
            "0" | "false" | "no" => Ok(false),
            _ => Err(format!("{name} must be true or false")),
        },
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{name} is not valid Unicode")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().expect("env lock")
    }

    #[test]
    fn defaults_match_the_confirmed_demo() {
        let _guard = lock_env();
        clear_vaca_env();
        let config = Config::from_env().expect("defaults");
        assert_eq!(config.bind, "127.0.0.1:8787".parse().unwrap());
        assert!(config.mint_url.is_empty());
        assert_eq!(config.max_pot_sats, 100_000);
        assert_eq!(config.grace_seconds, 600);
        assert!(config.experimental);
        assert!(config.server_key_path.is_none());
        assert_eq!(
            config.server_key_status(),
            ServerKeyStatus {
                path_set: false,
                file_present: false,
            }
        );
    }

    #[test]
    fn cap_cannot_be_raised() {
        let _guard = lock_env();
        clear_vaca_env();
        // SAFETY: this test holds ENV_LOCK, so the process environment is not
        // changed concurrently. The variables are cleared before return.
        unsafe { env::set_var("VACA_MAX_POT_SATS", "100001") };
        let error = Config::from_env().expect_err("cap");
        assert!(error.contains("100000"));
        unsafe { env::remove_var("VACA_MAX_POT_SATS") };
    }

    #[test]
    fn grace_is_fixed_at_ten_minutes() {
        let _guard = lock_env();
        clear_vaca_env();
        unsafe { env::set_var("VACA_GRACE_SECONDS", "60") };
        let error = Config::from_env().expect_err("grace");
        assert!(error.contains("600"));
        unsafe { env::remove_var("VACA_GRACE_SECONDS") };
    }

    #[test]
    fn mint_url_rejects_credentials() {
        let _guard = lock_env();
        clear_vaca_env();
        unsafe { env::set_var("VACA_MINT_URL", "http://user:secret@127.0.0.1:8085") };
        let error = Config::from_env().expect_err("mint url");
        assert!(!error.contains("secret"));
        unsafe { env::remove_var("VACA_MINT_URL") };
    }

    #[test]
    fn server_key_status_does_not_require_reading_a_file() {
        let _guard = lock_env();
        clear_vaca_env();
        let missing =
            std::env::temp_dir().join(format!("vaca-missing-key-{}", uuid::Uuid::new_v4()));
        unsafe { env::set_var("VACA_SERVER_KEY_PATH", missing.as_os_str()) };
        let config = Config::from_env().expect("config");
        assert_eq!(
            config.server_key_status(),
            ServerKeyStatus {
                path_set: true,
                file_present: false,
            }
        );
        unsafe { env::remove_var("VACA_SERVER_KEY_PATH") };
    }

    fn clear_vaca_env() {
        for name in [
            "VACA_BIND",
            "VACA_MINT_URL",
            "VACA_DB_PATH",
            "VACA_MAX_POT_SATS",
            "VACA_GRACE_SECONDS",
            "VACA_CORS_ORIGIN",
            "VACA_EXPERIMENTAL",
            "VACA_SERVER_KEY_PATH",
        ] {
            unsafe { env::remove_var(name) };
        }
    }
}

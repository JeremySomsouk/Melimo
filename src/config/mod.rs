pub mod store;

use std::{env, fmt};

/// Deliberately redacted even in panic/debug output. No Display implementation.
#[derive(Clone)]
pub struct Arl(String);

impl fmt::Debug for Arl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Arl([REDACTED])")
    }
}

impl Arl {
    pub fn from_env() -> Result<Self, &'static str> {
        let value = env::var("DEEZER_ARL").map_err(|_| "Set DEEZER_ARL to your own Deezer session cookie, or run melimo --mock for the offline demo.")?;
        Self::parse(value)
    }

    pub(crate) fn parse(value: String) -> Result<Self, &'static str> {
        // ARL cookies are 192 hexadecimal characters. Reject whitespace and
        // delimiters rather than risking header injection or echoing the input.
        if value.len() != 192 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("DEEZER_ARL must contain exactly 192 hexadecimal characters (no spaces).");
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// Deezer's regular web sign-in. The former
/// `/desktop/login/electron/callback` URL only completes inside Deezer's
/// official Electron app and serves "la connexion a échoué" in a normal
/// browser, so it must not be opened directly.
const LOGIN_URL: &str = "https://www.deezer.com/login";

/// Open the official browser sign-in flow; never scrape a browser cookie store.
pub fn login_arl() -> Result<Arl, String> {
    let url = LOGIN_URL;
    println!("Sign in to Deezer in your browser: {url}");
    println!(
        "Then open DevTools (Cmd+Option+I) > Application/Storage > Cookies > https://www.deezer.com"
    );
    println!("and copy the value of the `arl` cookie (192 hex characters).");
    println!(
        "Paste it below. Input is hidden; a verified login is saved unless MELIMO_NO_STORE=1."
    );
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(target_os = "windows")]
    let opener = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let opener = "xdg-open";
    let _ = std::process::Command::new(opener)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let input = rpassword::prompt_password("Deezer ARL cookie (blank cancels): ")
        .map_err(|_| "Cannot read hidden terminal input.")?;
    parse_login(input)
}

pub fn parse_login(input: String) -> Result<Arl, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Login cancelled.".into());
    }
    let value = if let Some(link) = input.strip_prefix("deezer://") {
        link.split_once('/')
            .map(|(_, path)| path.trim_end_matches('/'))
            .ok_or("Invalid Deezer login link.")?
    } else {
        input
    };
    Arl::parse(value.to_owned()).map_err(str::to_owned)
}

/// Resolve credentials for non-interactive flows such as `--check-auth`:
/// `DEEZER_ARL` first, then a saved login. Never writes anything.
pub fn resolve_arl() -> Result<Arl, String> {
    if env::var_os("DEEZER_ARL").is_some() {
        return Arl::from_env().map_err(str::to_owned);
    }
    match store::load()? {
        Some(arl) => Ok(arl),
        None => Err(
            "Set DEEZER_ARL to your own Deezer session cookie, or run melimo --login to sign in."
                .into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_opens_web_sign_in_not_the_electron_callback() {
        assert!(LOGIN_URL.starts_with("https://www.deezer.com/"));
        assert!(!LOGIN_URL.contains("electron"));
        assert!(!LOGIN_URL.contains("callback"));
    }

    #[test]
    fn login_accepts_hidden_cookie_or_callback_and_redacts_errors() {
        let cookie = "a".repeat(192);
        assert!(parse_login(cookie.clone()).is_ok());
        assert!(parse_login(format!("deezer://autolog/{cookie}")).is_ok());
        let error = parse_login("deezer://autolog/PRIVATE_SECRET".into()).unwrap_err();
        assert!(!error.contains("PRIVATE_SECRET"));
        assert!(
            parse_login(String::new())
                .unwrap_err()
                .contains("cancelled")
        );
    }
    #[test]
    fn credentials_are_validated_and_debug_redacted() {
        let value = "a".repeat(192);
        let arl = Arl::parse(value.clone()).unwrap();
        assert!(!format!("{arl:?}").contains(&value));
        for value in [
            String::new(),
            "x".repeat(192),
            format!("{}\r\n", "a".repeat(190)),
        ] {
            assert!(Arl::parse(value).is_err());
        }
    }
}

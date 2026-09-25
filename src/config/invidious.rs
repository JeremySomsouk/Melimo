//! Non-secret settings; automatic discovery is enabled by default.
use reqwest::Url;
use serde::Deserialize;
use std::{env, fs, io::Read, path::PathBuf};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub invidious: InvidiousSettings,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InvidiousSettings {
    pub enabled: bool,
    pub invidious_instance: String,
}

impl Default for InvidiousSettings {
    fn default() -> Self { Self { enabled: true, invidious_instance: String::new() } }
}

impl Settings {
    pub fn load() -> Result<Self, String> {
        let explicit = env::var_os("MELIMO_CONFIG");
        let path = explicit.clone().map(PathBuf::from).or_else(|| {
            env::var_os("XDG_CONFIG_HOME")
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .or_else(|| env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
                .map(|p| p.join("melimo/config.toml"))
        });
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let file =
            match fs::File::open(path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && explicit.is_none() => {
                    return Ok(Self::default());
                }
                Err(_) => return Err(
                    "Cannot read Mélimo configuration. Check MELIMO_CONFIG and file permissions."
                        .into(),
                ),
            };
        let mut text = String::new();
        file.take(65537)
            .read_to_string(&mut text)
            .map_err(|_| "Cannot read Mélimo configuration as UTF-8.")?;
        if text.len() > 65536 {
            return Err("Mélimo configuration is too large (maximum 64 KiB).".into());
        }
        Self::parse(&text)
    }

    fn parse(text: &str) -> Result<Self, String> {
        // Parser diagnostics can contain local settings: never echo them.
        let settings: Self = toml::from_str(text)
            .map_err(|_| "Invalid Mélimo TOML configuration. Expected [invidious], enabled (boolean) and invidious_instance (URL string).")?;
        settings.invidious.instance()?;
        Ok(settings)
    }
}

impl InvidiousSettings {
    pub fn instance(&self) -> Result<Option<Url>, String> {
        if !self.enabled || self.invidious_instance.trim().is_empty() {
            return Ok(None);
        }
        validate_url(&self.invidious_instance).map(Some).map_err(|_| {
            "Invidious is enabled: set invidious.invidious_instance to an HTTP(S) instance URL without credentials, query or fragment.".into()
        })
    }
}

pub(crate) fn validate_url(value: &str) -> Result<Url, ()> {
    let mut url = Url::parse(value).map_err(|_| ())?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(());
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_by_default_and_explicit_instances_are_validated() {
        assert!(
            Settings::parse("")
                .unwrap()
                .invidious
                .instance()
                .unwrap()
                .is_none()
        );
        for url in [
            "file:///secret",
            "https://user:secret@example.invalid",
            "https://example.invalid/?secret",
            "https://example.invalid/#secret",
        ] {
            let input = format!("[invidious]\nenabled = true\ninvidious_instance = {url:?}");
            let error = Settings::parse(&input).err().unwrap();
            assert!(!error.contains("secret"));
        }
        let s = Settings::parse(
            "[invidious]\nenabled = true\ninvidious_instance = 'https://example.invalid/proxy'",
        )
        .unwrap();
        assert_eq!(
            s.invidious.instance().unwrap().unwrap().as_str(),
            "https://example.invalid/proxy/"
        );
        assert!(Settings::parse("[invidious]\nenabled = 'yes'").is_err());
        assert!(Settings::parse("[invidious]\nenabeld = true").is_err());
    }
}

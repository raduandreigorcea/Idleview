use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use idleview_core::{Units, Visibility};

/// The single in-memory source of truth for settings. Every reader and writer -
/// Tauri commands and HTTP handlers alike - goes through this, so the two can
/// never drift apart.
static SETTINGS_CACHE: OnceLock<RwLock<Settings>> = OnceLock::new();

/// Every section defaults when missing, and unknown fields are ignored, so a settings
/// file from an older build (fonts, photo quality, custom queries...) still loads.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Settings {
    pub units: Units,
    pub display: Visibility,
    pub photos: PhotosSettings,
    /// Never leaves the process over HTTP - see `redacted`. Persisted to the
    /// settings file like everything else, but stripped from every API response.
    pub secrets: SecretSettings,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SecretSettings {
    /// Shared secret the control panel must present to mutate settings. Generated
    /// on first run; clients may never set it, only use it.
    ///
    /// There is deliberately no Unsplash key here. Photos come from the proxy (see
    /// `crate::photos`), which holds the key server-side.
    #[serde(default)]
    pub auth_token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct PhotosSettings {
    /// "unsplash" or "local". Local means the user's own photos only - the photo
    /// service is never contacted.
    pub source: String,
    pub refresh_interval: u64, // minutes
    pub enable_festive_queries: bool,
}

impl Default for PhotosSettings {
    fn default() -> Self {
        Self { source: "unsplash".into(), refresh_interval: 30, enable_festive_queries: true }
    }
}

impl PhotosSettings {
    pub fn local(&self) -> bool {
        self.source == "local"
    }
}

// Every write path funnels through `validate`, so a hand-rolled `curl` or a stale
// panel cannot persist a value the rest of the app cannot cope with.
pub const REFRESH_INTERVAL_MIN: u64 = 1;
pub const REFRESH_INTERVAL_MAX: u64 = 24 * 60;

impl Settings {
    pub fn validate(&mut self) {
        self.units.validate();
        if !matches!(self.photos.source.as_str(), "unsplash" | "local") {
            self.photos.source = "unsplash".into();
        }
        self.photos.refresh_interval = self
            .photos
            .refresh_interval
            .clamp(REFRESH_INTERVAL_MIN, REFRESH_INTERVAL_MAX);
    }

    /// The shape the HTTP API is allowed to hand out: everything except `secrets`.
    pub fn redacted(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self).unwrap_or_else(|_| json!({}));
        if let Some(object) = value.as_object_mut() {
            object.remove("secrets");
        }
        value
    }
}

/// Get the cross-platform settings file path
pub fn get_settings_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .map_err(|_| "Failed to get APPDATA directory".to_string())
            .map(|appdata| PathBuf::from(appdata).join("idleview").join("settings.json"))
    }

    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .ok_or_else(|| "Failed to get home directory".to_string())
            .map(|home| {
                home.join("Library")
                    .join("Application Support")
                    .join("idleview")
                    .join("settings.json")
            })
    }

    #[cfg(target_os = "linux")]
    {
        dirs::config_dir()
            .ok_or_else(|| "Failed to get config directory".to_string())
            .map(|config| config.join("idleview").join("settings.json"))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err("Unsupported platform".to_string())
    }
}

/// Ensure the settings directory exists
fn ensure_settings_dir() -> Result<(), String> {
    let settings_path = get_settings_path()?;
    if let Some(parent) = settings_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create settings directory: {}", e))?;
        }
    }
    Ok(())
}

/// Get the shared cache, loading from disk on first access
fn settings_cache() -> &'static RwLock<Settings> {
    SETTINGS_CACHE.get_or_init(|| {
        let mut settings = read_settings_from_disk().unwrap_or_default();
        // A file written by an older build (or hand-edited) can hold values this
        // build cannot honour, so clamp on the way in as well as on the way out.
        settings.validate();
        RwLock::new(settings)
    })
}

/// Read the current settings
pub fn read_settings() -> Result<Settings, String> {
    settings_cache()
        .read()
        .map(|settings| settings.clone())
        .map_err(|e| format!("Failed to read settings: {}", e))
}

/// Serialize settings to the settings file. Does not touch the cache - callers
/// hold the cache write lock, so persisting and updating stay atomic together.
fn persist_to_disk(settings: &Settings) -> Result<(), String> {
    ensure_settings_dir()?;
    let settings_path = get_settings_path()?;

    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    fs::write(&settings_path, json)
        .map_err(|e| format!("Failed to write settings file: {}", e))
}

/// Carry `secrets` across a write from an untrusted client. The auth token is never
/// settable over the API - a client may use it, never change it.
fn preserve_secrets(incoming: &mut Settings, existing: &Settings) {
    incoming.secrets.auth_token = existing.secrets.auth_token.clone();
}

/// Replace all settings, persisting to disk and updating the cache.
pub fn write_settings(settings: &Settings) -> Result<Settings, String> {
    let mut cached = settings_cache()
        .write()
        .map_err(|e| format!("Failed to acquire write lock: {}", e))?;

    let mut updated = settings.clone();
    preserve_secrets(&mut updated, &cached);
    updated.validate();

    persist_to_disk(&updated)?;
    *cached = updated.clone();

    Ok(updated)
}

/// Merge a partial JSON patch into the current settings, persisting the result.
/// The read-modify-write happens under one write lock so concurrent patches
/// cannot clobber each other.
pub fn update_settings_partial(updates: serde_json::Value) -> Result<Settings, String> {
    let mut cached = settings_cache()
        .write()
        .map_err(|e| format!("Failed to acquire write lock: {}", e))?;

    let existing = cached.clone();

    let mut current = serde_json::to_value(&*cached)
        .map_err(|e| format!("Failed to serialize current settings: {}", e))?;

    merge_json(&mut current, updates);

    let mut updated: Settings = serde_json::from_value(current)
        .map_err(|e| format!("Failed to parse updated settings: {}", e))?;

    preserve_secrets(&mut updated, &existing);
    updated.validate();

    persist_to_disk(&updated)?;
    *cached = updated.clone();

    Ok(updated)
}

/// Characters a person has to read off a screen and type into a phone, so the
/// easily-confused ones (0/O, 1/I/l) are left out.
const TOKEN_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const TOKEN_LENGTH: usize = 8;

fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..TOKEN_LENGTH)
        .map(|_| TOKEN_ALPHABET[rng.gen_range(0..TOKEN_ALPHABET.len())] as char)
        .collect()
}

/// Return the auth token, minting and persisting one on first run.
pub fn ensure_auth_token() -> Result<String, String> {
    let mut cached = settings_cache()
        .write()
        .map_err(|e| format!("Failed to acquire write lock: {}", e))?;

    if cached.secrets.auth_token.trim().is_empty() {
        cached.secrets.auth_token = generate_token();
        persist_to_disk(&cached)?;
    }

    Ok(cached.secrets.auth_token.clone())
}

/// Compare a client-supplied token without leaking its length or contents through
/// timing. Not a defence against a determined local attacker, but it costs nothing.
pub fn token_matches(provided: &str) -> Result<bool, String> {
    let expected = ensure_auth_token()?;
    let provided = provided.as_bytes();
    let expected = expected.as_bytes();

    let mut difference = provided.len() ^ expected.len();
    for index in 0..provided.len().max(expected.len()) {
        let a = provided.get(index).copied().unwrap_or(0);
        let b = expected.get(index).copied().unwrap_or(0);
        difference |= (a ^ b) as usize;
    }

    Ok(difference == 0)
}

fn read_settings_from_disk() -> Result<Settings, String> {
    let settings_path = get_settings_path()?;

    if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .map_err(|e| format!("Failed to read settings file: {}", e))?;

        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings JSON: {}", e))
    } else {
        Ok(Settings::default())
    }
}

/// Merge JSON values recursively
fn merge_json(target: &mut serde_json::Value, source: serde_json::Value) {
    if let (Some(target_obj), Some(source_obj)) = (target.as_object_mut(), source.as_object()) {
        for (key, value) in source_obj {
            if let Some(target_value) = target_obj.get_mut(key) {
                if target_value.is_object() && value.is_object() {
                    merge_json(target_value, value.clone());
                } else {
                    *target_value = value.clone();
                }
            } else {
                target_obj.insert(key.clone(), value.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_json() {
        let mut target = serde_json::json!({ "a": 1, "b": { "c": 2, "d": 3 } });
        let source = serde_json::json!({ "b": { "c": 5 }, "e": 10 });

        merge_json(&mut target, source);

        assert_eq!(target["a"], 1);
        assert_eq!(target["b"]["c"], 5);
        assert_eq!(target["b"]["d"], 3);
        assert_eq!(target["e"], 10);
    }

    #[test]
    fn a_settings_file_from_the_customisation_era_still_loads() {
        // Written by a build that had fonts, photo quality and custom queries.
        let old = r#"{
            "units": { "temperature_unit": "fahrenheit", "time_format": "12h",
                       "date_format": "mdy", "wind_speed_unit": "mph" },
            "display": { "show_clock": true, "show_location": false, "show_debug": true,
                         "clock_font": "sacramento", "clock_font_weight": "thin" },
            "photos": { "refresh_interval": 60, "photo_quality": "high",
                        "enable_festive_queries": false, "custom_query": "misty forest" },
            "secrets": { "auth_token": "K7PMX2QD" }
        }"#;
        let settings: Settings = serde_json::from_str(old).unwrap();

        assert_eq!(settings.units.temperature_unit, "fahrenheit");
        assert!(!settings.display.show_location);
        assert!(settings.display.show_weekday, "missing toggles default to shown");
        assert_eq!(settings.photos.refresh_interval, 60);
        assert!(!settings.photos.enable_festive_queries);
        assert_eq!(settings.secrets.auth_token, "K7PMX2QD");

        // And the retired fields are gone on the next write.
        let rewritten = serde_json::to_string(&settings).unwrap();
        assert!(!rewritten.contains("custom_query") && !rewritten.contains("clock_font"));
    }

    #[test]
    fn a_partial_file_fills_in_defaults() {
        let settings: Settings = serde_json::from_str(r#"{ "photos": {} }"#).unwrap();
        assert_eq!(settings.photos.refresh_interval, 30);
        assert_eq!(settings.units.time_format, "24h");
    }

    #[test]
    fn a_zero_refresh_interval_is_clamped_rather_than_meaning_never_cache() {
        let mut settings = Settings::default();
        settings.photos.refresh_interval = 0;
        settings.validate();
        assert_eq!(settings.photos.refresh_interval, REFRESH_INTERVAL_MIN);

        settings.photos.refresh_interval = 999_999;
        settings.validate();
        assert_eq!(settings.photos.refresh_interval, REFRESH_INTERVAL_MAX);
    }

    #[test]
    fn an_unknown_photo_source_falls_back_to_unsplash() {
        let mut settings = Settings::default();
        settings.photos.source = "ftp".into();
        settings.validate();
        assert_eq!(settings.photos.source, "unsplash");
    }

    #[test]
    fn redacted_settings_never_carry_the_secrets() {
        let mut settings = Settings::default();
        settings.secrets.auth_token = "TOKEN123".to_string();

        let redacted = settings.redacted();
        assert!(!serde_json::to_string(&redacted).unwrap().contains("TOKEN123"));
        assert!(redacted.get("secrets").is_none());
    }

    #[test]
    fn a_client_cannot_overwrite_the_auth_token() {
        let mut existing = Settings::default();
        existing.secrets.auth_token = "REALTOKEN".to_string();

        let mut incoming = Settings::default();
        incoming.secrets.auth_token = "ATTACKER".to_string();

        preserve_secrets(&mut incoming, &existing);
        assert_eq!(incoming.secrets.auth_token, "REALTOKEN");
    }

    #[test]
    fn generated_tokens_are_typeable_and_not_constant() {
        let token = generate_token();
        assert_eq!(token.len(), TOKEN_LENGTH);
        assert!(token.bytes().all(|b| TOKEN_ALPHABET.contains(&b)));
        assert!(!token.contains('0') && !token.contains('O'));
        assert_ne!(generate_token(), generate_token());
    }
}

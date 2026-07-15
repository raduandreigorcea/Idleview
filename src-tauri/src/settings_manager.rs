use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use crate::fonts;

/// The single in-memory source of truth for settings. Every reader and writer -
/// Tauri commands and HTTP handlers alike - goes through this, so the two can
/// never drift apart.
static SETTINGS_CACHE: OnceLock<RwLock<Settings>> = OnceLock::new();

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    pub units: UnitsSettings,
    pub display: DisplaySettings,
    pub photos: PhotosSettings,
    /// Never leaves the process over HTTP - see `redacted`. Persisted to the
    /// settings file like everything else, but stripped from every API response.
    #[serde(default)]
    pub secrets: SecretSettings,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SecretSettings {
    /// Shared secret the control panel must present to mutate settings. Generated
    /// on first run; clients may never set it, only use it.
    ///
    /// There is deliberately no Unsplash key here. Photos come from the proxy (see
    /// `crate::photos`), which holds the key server-side - a key on the user's machine,
    /// whether compiled in or sitting in a settings file, is a key that has been given
    /// away.
    #[serde(default)]
    pub auth_token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnitsSettings {
    pub temperature_unit: String,   // "celsius" or "fahrenheit"
    pub time_format: String,        // "24h" or "12h"
    pub date_format: String,        // "mdy", "dmy", "ymd"
    pub wind_speed_unit: String,    // "kmh", "mph", "ms"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DisplaySettings {
    #[serde(default = "default_show_clock")]
    pub show_clock: bool,
    #[serde(default = "default_show_date")]
    pub show_date: bool,
    #[serde(default = "default_show_weekday")]
    pub show_weekday: bool,
    #[serde(default = "default_show_temperature")]
    pub show_temperature: bool,
    #[serde(default = "default_true")]
    pub show_humidity_wind: bool,
    #[serde(default = "default_true")]
    pub show_precipitation_cloudiness: bool,
    #[serde(default = "default_true")]
    pub show_sunrise_sunset: bool,
    #[serde(default = "default_show_location")]
    pub show_location: bool,
    #[serde(default)]
    pub show_debug: bool,
    #[serde(default = "default_clock_font")]
    pub clock_font: String,
    #[serde(default = "default_clock_font_size")]
    pub clock_font_size: u16,
    // Weights are numbers now (100-900), not words. "thin" used to mean 200, a weight
    // most of these families do not publish, so it was silently synthesised. The
    // deserializer still accepts the old words from an existing settings file.
    #[serde(default = "default_clock_font_weight", deserialize_with = "deserialize_weight")]
    pub clock_font_weight: u16,
    #[serde(default = "default_weekday_font")]
    pub weekday_font: String,
    #[serde(default = "default_weekday_font_size")]
    pub weekday_font_size: u16,
    #[serde(default = "default_weekday_font_weight", deserialize_with = "deserialize_weight")]
    pub weekday_font_weight: u16,
    #[serde(default = "default_date_font")]
    pub date_font: String,
    #[serde(default = "default_date_font_size")]
    pub date_font_size: u16,
    #[serde(default = "default_date_font_weight", deserialize_with = "deserialize_weight")]
    pub date_font_weight: u16,
}

fn default_true() -> bool { true }
fn default_show_location() -> bool { true }
fn default_show_clock() -> bool { true }
fn default_show_date() -> bool { true }
fn default_show_weekday() -> bool { true }
fn default_show_temperature() -> bool { true }

fn default_clock_font() -> String { fonts::Role::Clock.default_font().to_string() }
fn default_clock_font_size() -> u16 { 180 }
fn default_clock_font_weight() -> u16 { fonts::Role::Clock.default_weight() }
fn default_weekday_font() -> String { fonts::Role::Weekday.default_font().to_string() }
fn default_weekday_font_size() -> u16 { 70 }
fn default_weekday_font_weight() -> u16 { fonts::Role::Weekday.default_weight() }
fn default_date_font() -> String { fonts::Role::Date.default_font().to_string() }
fn default_date_font_size() -> u16 { 40 }
fn default_date_font_weight() -> u16 { fonts::Role::Date.default_weight() }

/// Accept a number, a numeric string, or one of the legacy weight words that existing
/// settings files still hold.
fn deserialize_weight<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    fn from_str(value: &str) -> u16 {
        match value.trim().to_lowercase().as_str() {
            "thin" => 200,
            "light" => 300,
            "regular" | "normal" => 400,
            "medium" => 500,
            "semibold" => 600,
            "bold" => 700,
            other => other.parse::<u16>().unwrap_or(400),
        }
    }

    struct WeightVisitor;

    impl<'de> Visitor<'de> for WeightVisitor {
        type Value = u16;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a font weight as a number or a name")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<u16, E> {
            Ok(from_str(value))
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<u16, E> {
            Ok(value.min(u16::MAX as u64) as u16)
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<u16, E> {
            Ok(value.clamp(0, u16::MAX as i64) as u16)
        }

        fn visit_f64<E: de::Error>(self, value: f64) -> Result<u16, E> {
            Ok(value.round().clamp(0.0, u16::MAX as f64) as u16)
        }
    }

    deserializer.deserialize_any(WeightVisitor)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PhotosSettings {
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval: u64,  // in minutes
    /// A percentage, not a label. The lenient deserializer below still accepts the
    /// numeric strings and legacy words ("high") that older settings files hold.
    #[serde(default = "default_photo_quality", deserialize_with = "deserialize_quality")]
    pub photo_quality: u8,
    #[serde(default = "default_true")]
    pub enable_festive_queries: bool,
    #[serde(default)]
    pub custom_query: String,  // If non-empty, replaces generated query
}

fn default_refresh_interval() -> u64 { 30 }
fn default_photo_quality() -> u8 { 80 }

// ===== Validation =====
//
// Every write path funnels through `validate`. Clamping here rather than in the
// clients is what stops a hand-rolled `curl` (or a stale UI) from persisting a
// value the rest of the app cannot cope with - a refresh_interval of 0 used to
// mean "never cache" to Rust and "30 minutes" to JS, so the photo silently
// refetched on every check while the debug panel counted down to a refresh that
// had already happened.

pub const REFRESH_INTERVAL_MIN: u64 = 1;
pub const REFRESH_INTERVAL_MAX: u64 = 24 * 60;
pub const PHOTO_QUALITY_MIN: u8 = 30;
pub const PHOTO_QUALITY_MAX: u8 = 100;
pub const CLOCK_FONT_SIZE_MIN: u16 = 120;
pub const CLOCK_FONT_SIZE_MAX: u16 = 260;
pub const SECONDARY_FONT_SIZE_MIN: u16 = 40;
pub const SECONDARY_FONT_SIZE_MAX: u16 = 200;
pub const CUSTOM_QUERY_MAX_CHARS: usize = 120;

fn clamp_choice(value: &mut String, allowed: &[&str], fallback: &str) {
    let lowered = value.to_lowercase();
    if allowed.contains(&lowered.as_str()) {
        *value = lowered;
    } else {
        *value = fallback.to_string();
    }
}

impl Settings {
    /// Force every field into a range the rest of the app can actually honour.
    pub fn validate(&mut self) {
        clamp_choice(&mut self.units.temperature_unit, &["celsius", "fahrenheit"], "celsius");
        clamp_choice(&mut self.units.time_format, &["24h", "12h"], "24h");
        clamp_choice(&mut self.units.date_format, &["mdy", "dmy", "ymd"], "dmy");
        clamp_choice(&mut self.units.wind_speed_unit, &["kmh", "mph", "ms"], "kmh");

        // Fonts and weights are resolved against the catalogue, so a settings file can
        // never name a font we do not load, nor a weight the chosen font lacks.
        let resolve = |role: fonts::Role, id: &mut String, weight: &mut u16| {
            let font = fonts::resolve_font(role, id);
            *id = font.id.to_string();
            *weight = fonts::nearest_weight(font, *weight);
        };

        resolve(
            fonts::Role::Clock,
            &mut self.display.clock_font,
            &mut self.display.clock_font_weight,
        );
        resolve(
            fonts::Role::Weekday,
            &mut self.display.weekday_font,
            &mut self.display.weekday_font_weight,
        );
        resolve(
            fonts::Role::Date,
            &mut self.display.date_font,
            &mut self.display.date_font_weight,
        );

        self.display.clock_font_size = self
            .display
            .clock_font_size
            .clamp(CLOCK_FONT_SIZE_MIN, CLOCK_FONT_SIZE_MAX);
        self.display.weekday_font_size = self
            .display
            .weekday_font_size
            .clamp(SECONDARY_FONT_SIZE_MIN, SECONDARY_FONT_SIZE_MAX);
        self.display.date_font_size = self
            .display
            .date_font_size
            .clamp(SECONDARY_FONT_SIZE_MIN, SECONDARY_FONT_SIZE_MAX);

        self.photos.refresh_interval = self
            .photos
            .refresh_interval
            .clamp(REFRESH_INTERVAL_MIN, REFRESH_INTERVAL_MAX);
        self.photos.photo_quality = self
            .photos
            .photo_quality
            .clamp(PHOTO_QUALITY_MIN, PHOTO_QUALITY_MAX);

        let trimmed: String = self
            .photos
            .custom_query
            .trim()
            .chars()
            .take(CUSTOM_QUERY_MAX_CHARS)
            .collect();
        self.photos.custom_query = trimmed;
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

// Accept a number, a numeric string, or one of the legacy words that older
// settings files on disk still contain.
fn deserialize_quality<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct QualityVisitor;

    fn from_str(value: &str) -> u8 {
        match value.trim().to_lowercase().as_str() {
            "low" => 65,
            "medium" => 80,
            "high" | "maximum" => 100,
            other => other.parse::<u8>().unwrap_or(default_photo_quality()),
        }
    }

    impl<'de> Visitor<'de> for QualityVisitor {
        type Value = u8;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a quality percentage as a number or string")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<u8, E> {
            Ok(from_str(value))
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<u8, E> {
            Ok(value.min(u8::MAX as u64) as u8)
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<u8, E> {
            Ok(value.clamp(0, u8::MAX as i64) as u8)
        }

        fn visit_f64<E: de::Error>(self, value: f64) -> Result<u8, E> {
            Ok(value.round().clamp(0.0, u8::MAX as f64) as u8)
        }
    }

    deserializer.deserialize_any(QualityVisitor)
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            units: UnitsSettings {
                temperature_unit: "celsius".to_string(),
                time_format: "24h".to_string(),
                date_format: "dmy".to_string(),
                wind_speed_unit: "kmh".to_string(),
            },
            display: DisplaySettings {
                show_clock: true,
                show_date: true,
                show_weekday: true,
                show_temperature: true,
                show_humidity_wind: true,
                show_precipitation_cloudiness: true,
                show_sunrise_sunset: true,
                show_location: true,
                show_debug: false,
                clock_font: default_clock_font(),
                clock_font_size: default_clock_font_size(),
                clock_font_weight: default_clock_font_weight(),
                weekday_font: default_weekday_font(),
                weekday_font_size: default_weekday_font_size(),
                weekday_font_weight: default_weekday_font_weight(),
                date_font: default_date_font(),
                date_font_size: default_date_font_size(),
                date_font_weight: default_date_font_weight(),
            },
            photos: PhotosSettings {
                refresh_interval: default_refresh_interval(),
                photo_quality: default_photo_quality(),
                enable_festive_queries: true,
                custom_query: String::new(),
            },
            secrets: SecretSettings::default(),
        }
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

/// A handle onto the shared settings. Holds no state of its own - it reads and
/// writes the same cache the Tauri commands use, which is what keeps the HTTP
/// API and the desktop window from disagreeing about the current settings.
#[derive(Clone, Copy, Default)]
pub struct SettingsManager;

impl SettingsManager {
    pub fn new() -> Result<Self, String> {
        // Load eagerly so a broken settings path surfaces at startup, not on the
        // first request.
        read_settings()?;
        Ok(Self)
    }

    pub fn get(&self) -> Result<Settings, String> {
        read_settings()
    }

    pub fn update_all(&self, new_settings: Settings) -> Result<Settings, String> {
        write_settings(&new_settings)
    }

    pub fn update_partial(&self, updates: serde_json::Value) -> Result<Settings, String> {
        update_settings_partial(updates)
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
    fn test_default_settings() {
        let settings = Settings::default();
        assert!(settings.display.show_clock);
        assert!(settings.display.show_date);
        assert!(settings.display.show_weekday);
        assert!(settings.display.show_temperature);
        assert_eq!(settings.units.temperature_unit, "celsius");
        assert_eq!(settings.photos.refresh_interval, 30);
        assert_eq!(settings.photos.photo_quality, 80);
        assert!(settings.photos.custom_query.is_empty());
    }

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
    fn a_zero_refresh_interval_is_clamped_rather_than_meaning_never_cache() {
        // The bug this guards: 0 made is_cache_valid always false (refetch on every
        // check) while the JS side read 0 as "fall back to 30 minutes".
        let mut settings = Settings::default();
        settings.photos.refresh_interval = 0;
        settings.validate();
        assert_eq!(settings.photos.refresh_interval, REFRESH_INTERVAL_MIN);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let mut settings = Settings::default();
        settings.photos.photo_quality = 200;
        settings.display.clock_font_size = 4000;
        settings.display.weekday_font_size = 1;
        settings.photos.refresh_interval = 999_999;
        settings.validate();

        assert_eq!(settings.photos.photo_quality, PHOTO_QUALITY_MAX);
        assert_eq!(settings.display.clock_font_size, CLOCK_FONT_SIZE_MAX);
        assert_eq!(settings.display.weekday_font_size, SECONDARY_FONT_SIZE_MIN);
        assert_eq!(settings.photos.refresh_interval, REFRESH_INTERVAL_MAX);
    }

    #[test]
    fn unknown_enum_values_fall_back_to_defaults() {
        let mut settings = Settings::default();
        settings.units.temperature_unit = "kelvin".to_string();
        settings.units.time_format = "36h".to_string();
        settings.validate();

        assert_eq!(settings.units.temperature_unit, "celsius");
        assert_eq!(settings.units.time_format, "24h");
    }

    #[test]
    fn a_font_we_do_not_load_cannot_be_persisted() {
        let mut settings = Settings::default();
        // google_sans was offered by both UIs and is not on Google Fonts at all.
        settings.display.clock_font = "google_sans".to_string();
        // A script face is not a legal clock font.
        settings.display.weekday_font = "roboto".to_string();
        settings.validate();

        assert_eq!(settings.display.clock_font, "roboto");
        assert_eq!(settings.display.weekday_font, "great_vibes");
    }

    #[test]
    fn a_weight_the_font_lacks_is_snapped_to_one_it_has() {
        let mut settings = Settings::default();
        settings.display.clock_font = "arimo".to_string();
        settings.display.clock_font_weight = 200; // Arimo has nothing below 400.
        settings.validate();

        assert_eq!(settings.display.clock_font_weight, 400);
    }

    #[test]
    fn legacy_weight_words_still_parse() {
        let parse = |raw: &str| -> u16 {
            let json = format!(
                r#"{{"show_humidity_wind":true,"show_precipitation_cloudiness":true,
                     "show_sunrise_sunset":true,"clock_font_weight":{}}}"#,
                raw
            );
            serde_json::from_str::<DisplaySettings>(&json).unwrap().clock_font_weight
        };

        assert_eq!(parse(r#""thin""#), 200);
        assert_eq!(parse(r#""regular""#), 400);
        assert_eq!(parse(r#""medium""#), 500);
        assert_eq!(parse(r#""bold""#), 700);
        assert_eq!(parse("600"), 600);   // the shape the panel now sends
        assert_eq!(parse(r#""600""#), 600);
    }

    #[test]
    fn custom_query_is_trimmed_and_bounded() {
        let mut settings = Settings::default();
        settings.photos.custom_query = format!("  {}  ", "a".repeat(500));
        settings.validate();
        assert_eq!(settings.photos.custom_query.chars().count(), CUSTOM_QUERY_MAX_CHARS);
    }

    #[test]
    fn quality_accepts_numbers_numeric_strings_and_legacy_words() {
        let parse = |raw: &str| -> u8 {
            let json = format!(
                r#"{{"refresh_interval":30,"photo_quality":{},"enable_festive_queries":true,"custom_query":""}}"#,
                raw
            );
            serde_json::from_str::<PhotosSettings>(&json).unwrap().photo_quality
        };

        assert_eq!(parse("95"), 95);        // number, the shape the panel now sends
        assert_eq!(parse(r#""95""#), 95);   // numeric string, what older files hold
        assert_eq!(parse(r#""high""#), 100); // legacy word
        assert_eq!(parse(r#""low""#), 65);
    }

    #[test]
    fn redacted_settings_never_carry_the_secrets() {
        let mut settings = Settings::default();
        settings.secrets.auth_token = "TOKEN123".to_string();

        let redacted = settings.redacted();
        let serialized = serde_json::to_string(&redacted).unwrap();

        assert!(!serialized.contains("TOKEN123"));
        assert!(redacted.get("secrets").is_none());
    }

    #[test]
    fn the_settings_hold_no_unsplash_key_at_all() {
        // Photos come from the proxy, which holds the key server-side. A key stored on
        // the user's machine - compiled in OR in this file - is a key handed away, so
        // there is deliberately nowhere here to put one.
        let mut settings = Settings::default();
        settings.secrets.auth_token = "TOKEN".to_string();
        let stored = serde_json::to_string(&settings).unwrap();

        assert!(!stored.contains("unsplash_access_key"));
        assert!(!stored.contains("access_key"));
    }

    #[test]
    fn a_client_cannot_overwrite_the_auth_token() {
        let mut existing = Settings::default();
        existing.secrets.auth_token = "REALTOKEN".to_string();

        // A client may use the token, never change it.
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
        // Ambiguous glyphs would get mistyped off a screen.
        assert!(!token.contains('0') && !token.contains('O'));
        assert_ne!(generate_token(), generate_token());
    }
}

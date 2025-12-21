use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    pub units: UnitsSettings,
    pub display: DisplaySettings,
    pub photos: PhotosSettings,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnitsSettings {
    pub temperature_unit: String,  // "celsius" or "fahrenheit"
    pub time_format: String,        // "24h" or "12h"
    pub date_format: String,        // "mdy", "dmy", "ymd"
    pub wind_speed_unit: String,    // "kmh", "mph", "ms"
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DisplaySettings {
    pub show_humidity_wind: bool,
    pub show_precipitation_cloudiness: bool,
    pub show_sunrise_sunset: bool,
    pub show_cpu_temp: bool,
    #[serde(default = "default_theme")]
    pub theme: String,  // "default", "nest"
}

fn default_theme() -> String {
    "default".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PhotosSettings {
    pub refresh_interval: u64,  // in minutes
    #[serde(deserialize_with = "deserialize_quality")]
    pub photo_quality: String,  // Accepts both "85" string or 85 number
}

// Custom deserializer to handle both string and number
fn deserialize_quality<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct QualityVisitor;

    impl<'de> Visitor<'de> for QualityVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or number")
        }

        fn visit_str<E>(self, value: &str) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_u64<E>(self, value: u64) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_i64<E>(self, value: i64) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
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
                show_humidity_wind: true,
                show_precipitation_cloudiness: true,
                show_sunrise_sunset: true,
                show_cpu_temp: false,
                theme: "default".to_string(),
            },
            photos: PhotosSettings {
                refresh_interval: 30,
                photo_quality: "80".to_string(),
            },
        }
    }
}

/// Get the cross-platform settings file path
pub fn get_settings_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        // Windows: %APPDATA%\idleview\settings.json
        std::env::var("APPDATA")
            .map_err(|_| "Failed to get APPDATA directory".to_string())
            .map(|appdata| PathBuf::from(appdata).join("idleview").join("settings.json"))
    }
    
    #[cfg(target_os = "macos")]
    {
        // macOS: ~/Library/Application Support/idleview/settings.json
        dirs::home_dir()
            .ok_or_else(|| "Failed to get home directory".to_string())
            .map(|home| home.join("Library").join("Application Support").join("idleview").join("settings.json"))
    }
    
    #[cfg(target_os = "linux")]
    {
        // Linux: ~/.config/idleview/settings.json
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

/// Read settings from disk, returning default if file doesn't exist
pub fn read_settings() -> Result<Settings, String> {
    let settings_path = get_settings_path()?;
    
    if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .map_err(|e| format!("Failed to read settings file: {}", e))?;
        
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings JSON: {}", e))
    } else {
        // Return default settings if file doesn't exist
        Ok(Settings::default())
    }
}

/// Write settings to disk
pub fn write_settings(settings: &Settings) -> Result<(), String> {
    ensure_settings_dir()?;
    let settings_path = get_settings_path()?;
    
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    
    fs::write(&settings_path, json)
        .map_err(|e| format!("Failed to write settings file: {}", e))?;
    
    Ok(())
}

/// A thread-safe settings manager
#[derive(Clone)]
pub struct SettingsManager {
    settings: Arc<RwLock<Settings>>,
}

impl SettingsManager {
    pub fn new() -> Result<Self, String> {
        let settings = read_settings()?;
        Ok(Self {
            settings: Arc::new(RwLock::new(settings)),
        })
    }

    pub fn get(&self) -> Result<Settings, String> {
        self.settings
            .read()
            .map(|s| s.clone())
            .map_err(|e| format!("Failed to read settings: {}", e))
    }

    pub fn update_all(&self, new_settings: Settings) -> Result<(), String> {
        {
            let mut settings = self.settings
                .write()
                .map_err(|e| format!("Failed to acquire write lock: {}", e))?;
            *settings = new_settings.clone();
        }
        write_settings(&new_settings)
    }

    pub fn update_partial(&self, updates: serde_json::Value) -> Result<Settings, String> {
        let mut settings = self.settings
            .write()
            .map_err(|e| format!("Failed to acquire write lock: {}", e))?;
        
        // Convert current settings to JSON Value
        let mut current = serde_json::to_value(&*settings)
            .map_err(|e| format!("Failed to serialize current settings: {}", e))?;
        
        // Merge the updates
        merge_json(&mut current, updates);
        
        // Deserialize back to Settings
        let updated_settings: Settings = serde_json::from_value(current)
            .map_err(|e| format!("Failed to parse updated settings: {}", e))?;
        
        *settings = updated_settings.clone();
        drop(settings); // Release lock before writing to disk
        
        write_settings(&updated_settings)?;
        Ok(updated_settings)
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
        assert_eq!(settings.units.temperature_unit, "celsius");
        assert_eq!(settings.photos.refresh_interval, 30);
    }

    #[test]
    fn test_merge_json() {
        let mut target = serde_json::json!({
            "a": 1,
            "b": {
                "c": 2,
                "d": 3
            }
        });
        
        let source = serde_json::json!({
            "b": {
                "c": 5
            },
            "e": 10
        });
        
        merge_json(&mut target, source);
        
        assert_eq!(target["a"], 1);
        assert_eq!(target["b"]["c"], 5);
        assert_eq!(target["b"]["d"], 3);
        assert_eq!(target["e"], 10);
    }
}

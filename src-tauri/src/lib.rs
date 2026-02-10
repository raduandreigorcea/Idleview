use serde::{Deserialize, Serialize};
use chrono::{Datelike, Local};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

// HTTP server modules
pub mod settings_manager;
pub mod http_server;

// Re-export settings types from settings_manager
use settings_manager::Settings;

// ===== Core functions (public for testing) =====

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static UNSPLASH_ACCESS_KEY: OnceLock<String> = OnceLock::new();
static SUN_TIMES_CACHE: OnceLock<Mutex<SunTimesCache>> = OnceLock::new();

#[derive(Clone)]
struct SunTimesCache {
    sunrise_raw: String,
    sunset_raw: String,
    sunrise: chrono::NaiveDateTime,
    sunset: chrono::NaiveDateTime,
}

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(reqwest::Client::new)
}

fn unsplash_access_key() -> &'static str {
    UNSPLASH_ACCESS_KEY
        .get_or_init(|| {
            std::env::var("UNSPLASH_ACCESS_KEY").unwrap_or_else(|_| {
                option_env!("UNSPLASH_ACCESS_KEY")
                    .unwrap_or("YOUR_UNSPLASH_ACCESS_KEY")
                    .to_string()
            })
        })
        .as_str()
}

fn get_cached_sun_times(
    sunrise_str: &str,
    sunset_str: &str,
) -> Option<(chrono::NaiveDateTime, chrono::NaiveDateTime)> {
    if let Some(cache) = SUN_TIMES_CACHE.get() {
        if let Ok(cache) = cache.lock() {
            if cache.sunrise_raw == sunrise_str && cache.sunset_raw == sunset_str {
                return Some((cache.sunrise, cache.sunset));
            }
        }
    }

    let parsed = (
        chrono::NaiveDateTime::parse_from_str(sunrise_str, "%Y-%m-%dT%H:%M"),
        chrono::NaiveDateTime::parse_from_str(sunset_str, "%Y-%m-%dT%H:%M"),
    );

    if let (Ok(sunrise), Ok(sunset)) = parsed {
        let cache = SUN_TIMES_CACHE.get_or_init(|| {
            Mutex::new(SunTimesCache {
                sunrise_raw: sunrise_str.to_string(),
                sunset_raw: sunset_str.to_string(),
                sunrise,
                sunset,
            })
        });

        if let Ok(mut cache) = cache.lock() {
            *cache = SunTimesCache {
                sunrise_raw: sunrise_str.to_string(),
                sunset_raw: sunset_str.to_string(),
                sunrise,
                sunset,
            };
        }

        return Some((sunrise, sunset));
    }

    None
}

pub fn get_season_impl() -> Season {
    let now = Local::now();
    let month = now.month();
    
    let season = match month {
        3..=5 => "spring",
        6..=8 => "summer",
        9..=11 => "autumn",
        _ => "winter",
    };
    
    Season {
        season: season.to_string(),
    }
}

pub fn get_time_of_day_impl(sunrise_iso: Option<String>, sunset_iso: Option<String>) -> TimeOfDay {
    // If we have sunrise/sunset data, use it
    if let (Some(sunrise_str), Some(sunset_str)) = (sunrise_iso, sunset_iso) {
        // Parse as naive datetime (no timezone) since Open-Meteo returns local time
        if let Some((sunrise, sunset)) = get_cached_sun_times(&sunrise_str, &sunset_str) {
            let now = Local::now().naive_local();
            
            // Define dawn as 30 minutes before sunrise, dusk as 30 minutes after sunset
            let dawn_start = sunrise - chrono::Duration::minutes(30);
            let dawn_end = sunrise + chrono::Duration::minutes(30);
            let dusk_start = sunset - chrono::Duration::minutes(30);
            let dusk_end = sunset + chrono::Duration::minutes(30);
            
            let time_of_day = if now < dawn_start || now > dusk_end {
                "night"
            } else if now >= dawn_start && now <= dawn_end {
                "dawn"
            } else if now >= dusk_start && now <= dusk_end {
                "dusk"
            } else {
                "day"
            };
            
            return TimeOfDay {
                time_of_day: time_of_day.to_string(),
                source: "api".to_string(),
            };
        }
    }
    
    // Fallback to simple hour-based detection
    TimeOfDay {
        time_of_day: "night".to_string(),
        source: "fallback".to_string(),
    }
}

pub fn build_photo_query_impl(
    cloudcover: f64,
    rain: f64,
    snowfall: f64,
    sunrise_iso: Option<String>,
    sunset_iso: Option<String>,
    enable_festive: Option<bool>,
) -> PhotoQuery {
    
    // Get time of day and season
    let tod = get_time_of_day_impl(sunrise_iso, sunset_iso);
    let season = get_season_impl();
    
    // Check for festive/holiday periods
    let enable_festive = enable_festive.unwrap_or(true);
    if enable_festive {
        let now = Local::now();
        let month = now.month();
        let day = now.day();
        
        // Christmas period (Dec 20-26)
        if month == 12 && day >= 20 && day <= 26 {
            return PhotoQuery { query: "christmas".to_string() };
        }
        // New Year period (Dec 27 - Jan 5)
        if (month == 12 && day >= 27) || (month == 1 && day <= 5) {
            return PhotoQuery { query: "new year".to_string() };
        }
        // Halloween period (Oct 25-31)
        if month == 10 && day >= 25 {
            return PhotoQuery { query: "halloween".to_string() };
        }
    }
    
    // Determine precipitation type
    let has_snow = snowfall > 0.5;
    let has_rain = rain > 0.5;
    
    // Priority: time of day > season > precipitation
    // Night/dawn/dusk are "special" times that override season focus
    // During regular day, season takes priority
    
    let query = match tod.time_of_day.as_str() {
        "night" => {
            // Night is always prominent
            // Add precipitation as compound phrase: "{season} snowy night", "{season} rainy night"
            if has_snow {
                format!("{} snowy night", season.season)
            } else if has_rain {
                format!("{} rainy night", season.season)
            } else {
                // Just night + season
                format!("{} night", season.season)
            }
        },
        "dawn" => format!("{} dawn", season.season),
        "dusk" => format!("{} dusk", season.season),
        _ => {
            // Daytime: season is primary, add precipitation if present
            if has_snow {
                format!("{} snow", season.season)
            } else if has_rain {
                format!("{} rain", season.season)
            } else if cloudcover > 70.0 && season.season != "winter" {
                format!("{} cloudy", season.season)
            } else {
                // Clear day - just season
                season.season.to_string()
            }
        }
    };
    
    PhotoQuery { query }
}

pub fn get_current_time_impl() -> FormattedTime {
    let now = Local::now();
    
    // Get settings to determine format
    let settings = settings_manager::read_settings().unwrap_or_default();
    
    // Format time based on settings
    let time = if settings.units.time_format == "12h" {
        now.format("%-I:%M %p").to_string()
    } else {
        now.format("%H:%M").to_string()
    };
    
    // Format date based on settings
    let date = match settings.units.date_format.as_str() {
        "mdy" => now.format("%b %d, %Y").to_string(),  // Nov 28, 2025
        "dmy" => now.format("%d %b %Y").to_string(),   // 28 Nov 2025
        "ymd" => now.format("%Y %b %d").to_string(),   // 2025 Nov 28
        _ => now.format("%b %d, %Y").to_string(),      // Default to MDY
    };
    
    let day_of_week = now.format("%A").to_string().to_uppercase();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    FormattedTime {
        time,
        date,
        day_of_week,
        timestamp,
    }
}

pub fn get_precipitation_display_impl(weather: WeatherData) -> PrecipitationDisplay {
    if weather.snowfall > 0.0 {
        PrecipitationDisplay {
            icon: "snowflake.svg".to_string(),
            label: "Snow".to_string(),
            value: format!("{:.1} cm", weather.snowfall),
        }
    } else if weather.rain > 0.0 {
        PrecipitationDisplay {
            icon: "droplets.svg".to_string(),
            label: "Rain".to_string(),
            value: format!("{:.1} mm", weather.rain),
        }
    } else {
        PrecipitationDisplay {
            icon: "umbrella.svg".to_string(),
            label: "Precip".to_string(),
            value: "Clear".to_string(),
        }
    }
}

pub fn is_cache_valid_impl(cache_timestamp: u64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    let settings = settings_manager::read_settings().unwrap_or_default();
    let refresh_interval_ms = (settings.photos.refresh_interval as u64) * 60 * 1000;
    
    let cache_age = now.saturating_sub(cache_timestamp);
    cache_age < refresh_interval_ms
}

pub fn format_time_remaining_impl(milliseconds: i64) -> String {
    if milliseconds <= 0 {
        return "0s".to_string();
    }
    
    let total_seconds = milliseconds / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    
    if hours > 0 {
        format!("{}h {:02}m", hours, minutes)
    } else if minutes > 0 {
        format!("{}m {:02}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

// ===== Tauri Commands (wrappers) =====

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn get_settings() -> Result<Settings, String> {
    settings_manager::read_settings()
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), String> {
    settings_manager::write_settings(&settings)
}

#[tauri::command]
fn reset_settings() -> Result<Settings, String> {
    let settings = Settings::default();
    save_settings(settings.clone())?;
    Ok(settings)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    pub city: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IpApiResponse {
    lat: f64,
    lon: f64,
    city: Option<String>,
    country: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnsplashPhoto {
    pub url: String,
    pub author: String,
    pub author_url: String,
    pub download_location: String,
}

#[derive(Debug, Deserialize)]
struct UnsplashApiResponse {
    urls: UnsplashUrls,
    user: UnsplashUser,
    links: UnsplashPhotoLinks,
}

#[derive(Debug, Deserialize)]
struct UnsplashPhotoLinks {
    download_location: String,
}

#[derive(Debug, Deserialize)]
struct UnsplashUrls {
    regular: String,
}

#[derive(Debug, Deserialize)]
struct UnsplashUser {
    name: String,
    links: UnsplashUserLinks,
}

#[derive(Debug, Deserialize)]
struct UnsplashUserLinks {
    html: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WeatherData {
    pub temperature: f64,
    pub temperature_unit: String,
    pub humidity: f64,
    pub wind_speed: f64,
    pub wind_speed_unit: String,
    pub wind_speed_label: String,
    pub cloudcover: f64,
    pub rain: f64,
    pub snowfall: f64,
    pub sunrise: String,
    pub sunset: String,
    pub timezone: String,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoResponse {
    current: OpenMeteoCurrentData,
    daily: OpenMeteoDailyData,
    timezone: String,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoCurrentData {
    temperature_2m: f64,
    relative_humidity_2m: f64,
    rain: f64,
    snowfall: f64,
    cloudcover: f64,
    wind_speed_10m: f64,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoDailyData {
    sunrise: Vec<String>,
    sunset: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TimeOfDay {
    pub time_of_day: String, // "dawn", "day", "dusk", "night"
    pub source: String,      // "api" or "fallback"
}

#[derive(Debug, Serialize)]
pub struct Season {
    pub season: String, // "spring", "summer", "autumn", "winter"
}

#[derive(Debug, Serialize)]
pub struct Holiday {
    pub holiday: Option<String>, // "christmas", "new year", "halloween", "easter"
}

#[derive(Debug, Serialize)]
pub struct PhotoQuery {
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct FormattedTime {
    pub time: String,           // HH:MM
    pub date: String,           // e.g., "Nov 28, 2025"
    pub day_of_week: String,    // e.g., "FRIDAY"
    pub timestamp: u64,         // Unix timestamp in milliseconds
}

#[derive(Debug, Serialize)]
pub struct PhotoCache {
    pub photo: UnsplashPhoto,
    pub query: String,
    pub timestamp: u64,
}

#[derive(Debug, Serialize)]
pub struct PrecipitationDisplay {
    pub icon: String,      // "snowflake.svg", "droplets.svg", "umbrella.svg"
    pub label: String,     // "Snow", "Rain", "Precip"
    pub value: String,     // "5.0 cm", "3.2 mm", "Clear"
}

#[derive(Debug, Serialize)]
pub struct DebugInfo {
    pub photo_age: String,
    pub query: String,
    pub time_source: String, // "api" or "fallback"
    pub time_of_day: String, // "dawn", "day", "dusk", "night"
    pub api_key_status: String,
    pub api_key_source: String,
    // Weather info
    pub temperature: String,
    pub rain: String,
    pub snowfall: String,
    pub cloudcover: String,
    pub season: String,
}

#[tauri::command]
async fn get_location() -> Result<Location, String> {
    let response = http_client()
        .get("http://ip-api.com/json/")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch location: {}", e))?;
    
    let data: IpApiResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse location data: {}", e))?;
    
    Ok(Location {
        latitude: data.lat,
        longitude: data.lon,
        city: data.city,
        country: data.country,
    })
}

#[tauri::command]
async fn get_weather(latitude: f64, longitude: f64) -> Result<WeatherData, String> {
    let settings = get_settings().unwrap_or_default();
    
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,relative_humidity_2m,rain,snowfall,cloudcover,wind_speed_10m&daily=sunrise,sunset&timezone=auto",
        latitude, longitude
    );
    
    let response = http_client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch weather: {}", e))?;
    
    let data: OpenMeteoResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse weather data: {}", e))?;
    
    // Convert temperature based on user settings
    let temperature = match settings.units.temperature_unit.as_str() {
        "fahrenheit" => data.current.temperature_2m * 9.0 / 5.0 + 32.0,
        _ => data.current.temperature_2m, // celsius is default
    };
    
    // Convert wind speed based on user settings
    let wind_speed = match settings.units.wind_speed_unit.as_str() {
        "mph" => data.current.wind_speed_10m * 0.621371,
        "ms" => data.current.wind_speed_10m / 3.6,
        _ => data.current.wind_speed_10m, // kmh is default
    };
    
    // Get wind speed label
    let wind_speed_label = match settings.units.wind_speed_unit.as_str() {
        "mph" => "mph",
        "ms" => "m/s",
        _ => "km/h",
    }.to_string();
    
    Ok(WeatherData {
        temperature,
        temperature_unit: settings.units.temperature_unit.clone(),
        humidity: data.current.relative_humidity_2m,
        wind_speed,
        wind_speed_unit: settings.units.wind_speed_unit.clone(),
        wind_speed_label,
        cloudcover: data.current.cloudcover,
        rain: data.current.rain,
        snowfall: data.current.snowfall,
        sunrise: data.daily.sunrise.get(0).cloned().unwrap_or_default(),
        sunset: data.daily.sunset.get(0).cloned().unwrap_or_default(),
        timezone: data.timezone,
    })
}

#[tauri::command]
fn get_season() -> Season {
    get_season_impl()
}

#[tauri::command]
fn get_holiday() -> Holiday {
    let now = Local::now();
    let month = now.month();
    let day = now.day();
    
    let holiday = if month == 12 && day <= 26 {
        Some("christmas".to_string())
    } else if (month == 12 && day >= 27) || (month == 1 && day <= 5) {
        Some("new year".to_string())
    } else if month == 10 && day >= 25 {
        Some("halloween".to_string())
    } else if (month == 3 && day >= 20) || (month == 4 && day <= 20) {
        Some("easter".to_string())
    } else {
        None
    };
    
    Holiday { holiday }
}

#[tauri::command]
fn get_time_of_day(sunrise_iso: Option<String>, sunset_iso: Option<String>) -> TimeOfDay {
    get_time_of_day_impl(sunrise_iso, sunset_iso)
}

#[tauri::command]
fn build_photo_query(
    cloudcover: f64,
    rain: f64,
    snowfall: f64,
    sunrise_iso: Option<String>,
    sunset_iso: Option<String>,
    enable_festive: Option<bool>,
) -> PhotoQuery {
    build_photo_query_impl(cloudcover, rain, snowfall, sunrise_iso, sunset_iso, enable_festive)
}

#[tauri::command]
async fn get_unsplash_photo(width: u32, height: u32, query: String) -> Result<UnsplashPhoto, String> {
    let url = format!(
        "https://api.unsplash.com/photos/random?orientation=landscape&query={}&w={}&h={}",
        urlencoding::encode(&query),
        width,
        height
    );

    let response = http_client()
        .get(&url)
        .header("Authorization", format!("Client-ID {}", unsplash_access_key()))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch photo: {}", e))?;
    
    // Check response status
    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("Unsplash API error ({}): {}", status, error_text));
    }
    
    let data: UnsplashApiResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse photo data: {}", e))?;
    
    // Add cache-busting timestamp to prevent browser/CDN caching
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    
    // Apply photo quality setting
    let settings = get_settings().unwrap_or_default();
    
    // Parse quality as number (supports both string numbers like "100" and legacy text like "high")
    let quality = match settings.photos.photo_quality.as_str() {
        // Legacy string values (backwards compatibility)
        "low" => 65,
        "medium" => 80,
        "high" => 100,
        "maximum" => 100,
        // Parse numeric strings directly
        _ => settings.photos.photo_quality.parse::<u32>().unwrap_or(80)
    };
    
    // Parse the URL and replace existing quality parameter
    let mut url = data.urls.regular.clone();
    
    // Remove existing quality parameter if present
    if let Some(pos) = url.find("&q=") {
        if let Some(end_pos) = url[pos+1..].find('&') {
            url.replace_range(pos..pos+end_pos+1, "");
        } else {
            url.truncate(pos);
        }
    }
    
    // Add our parameters
    let separator = if url.contains('?') { "&" } else { "?" };
    let photo_url = format!("{}{}w={}&h={}&fit=crop&q={}&t={}", url, separator, width, height, quality, timestamp);
    
    Ok(UnsplashPhoto {
        url: photo_url,
        author: data.user.name,
        author_url: data.user.links.html,
        download_location: data.links.download_location,
    })
}

#[tauri::command]
async fn trigger_unsplash_download(download_url: String) -> Result<(), String> {
    let _response = http_client()
        .get(&download_url)
        .header("Authorization", format!("Client-ID {}", unsplash_access_key()))
        .send()
        .await
        .map_err(|e| format!("Failed to trigger download: {}", e))?;
    
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct CpuTemp {
    pub value: f32,
    pub display: String,
}

#[tauri::command]
fn get_cpu_temp() -> Result<CpuTemp, String> {
    #[cfg(target_os = "linux")]
    {
        match std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp") {
            Ok(contents) => {
                let temp_millidegrees: i32 = contents.trim()
                    .parse()
                    .map_err(|e| format!("Failed to parse temperature: {}", e))?;
                let temp_celsius = temp_millidegrees as f32 / 1000.0;
                
                if temp_celsius <= 0.0 {
                    return Ok(CpuTemp {
                        value: 0.0,
                        display: String::new(),
                    });
                }
                
                // Get settings for unit conversion
                let settings = get_settings().unwrap_or_default();
                let (display_temp, unit) = if settings.units.temperature_unit == "fahrenheit" {
                    (temp_celsius * 9.0 / 5.0 + 32.0, "°F")
                } else {
                    (temp_celsius, "°C")
                };
                
                Ok(CpuTemp {
                    value: temp_celsius,
                    display: format!("{} {}", display_temp.round() as i32, unit),
                })
            }
            Err(_) => Ok(CpuTemp {
                value: 0.0,
                display: String::new(),
            })
        }
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        Ok(CpuTemp {
            value: 0.0,
            display: String::new(),
        })
    }
}

#[tauri::command]
fn get_current_time() -> FormattedTime {
    get_current_time_impl()
}

#[tauri::command]
fn get_precipitation_display(weather: WeatherData) -> PrecipitationDisplay {
    get_precipitation_display_impl(weather)
}

#[tauri::command]
fn is_cache_valid(cache_timestamp: u64) -> bool {
    is_cache_valid_impl(cache_timestamp)
}

#[tauri::command]
fn format_time_remaining(milliseconds: i64) -> String {
    format_time_remaining_impl(milliseconds)
}

#[tauri::command]
fn get_debug_info(
    cache_timestamp: Option<u64>,
    query: Option<String>,
    sunrise_iso: Option<String>,
    sunset_iso: Option<String>,
    // Weather data
    temperature: Option<f64>,
    rain: Option<f64>,
    snowfall: Option<f64>,
    cloudcover: Option<f64>,
) -> DebugInfo {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    let photo_age = if let Some(ts) = cache_timestamp {
        // Use saturating_sub to avoid overflow if timestamp is in the future
        let diff = now.saturating_sub(ts);
        let seconds = diff / 1000;
        
        if seconds < 60 {
            format!("{}s ago", seconds)
        } else {
            let minutes = seconds / 60;
            if minutes < 60 {
                format!("{}m ago", minutes)
            } else {
                let hours = minutes / 60;
                if hours < 24 {
                    format!("{}h ago", hours)
                } else {
                    format!("{}d ago", hours / 24)
                }
            }
        }
    } else {
        "unknown".to_string()
    };
    
    let query_str = query.unwrap_or_else(|| "n/a".to_string());
    
    // Get time of day info
    let tod = get_time_of_day(sunrise_iso.clone(), sunset_iso.clone());
    
    // Get season
    let season_info = get_season();
    
    // Check API key availability
    let (api_key_status, api_key_source) = match std::env::var("UNSPLASH_ACCESS_KEY") {
        Ok(key) if key.len() > 10 && key != "YOUR_UNSPLASH_ACCESS_KEY" => {
            ("Available".to_string(), "Runtime env".to_string())
        },
        _ => {
            match option_env!("UNSPLASH_ACCESS_KEY") {
                Some(key) if key.len() > 10 && key != "YOUR_UNSPLASH_ACCESS_KEY" => {
                    ("Available".to_string(), "Compile-time".to_string())
                },
                _ => ("Missing or invalid".to_string(), "None".to_string())
            }
        }
    };
    
    // Get settings for temperature unit
    let settings = get_settings().unwrap_or_default();
    let temp_unit = settings.units.temperature_unit.as_str();
    
    DebugInfo {
        photo_age,
        query: query_str,
        time_source: tod.source,
        time_of_day: tod.time_of_day,
        api_key_status,
        api_key_source,
        temperature: temperature.map(|t| {
            if temp_unit == "fahrenheit" {
                format!("{:.1}°F", t)
            } else {
                format!("{:.1}°C", t)
            }
        }).unwrap_or_else(|| "n/a".to_string()),
        rain: rain.map(|r| format!("{:.1}mm", r)).unwrap_or_else(|| "n/a".to_string()),
        snowfall: snowfall.map(|s| format!("{:.1}cm", s)).unwrap_or_else(|| "n/a".to_string()),
        cloudcover: cloudcover.map(|c| format!("{}%", c as i32)).unwrap_or_else(|| "n/a".to_string()),
        season: season_info.season,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load .env file if it exists
    let _ = dotenvy::dotenv();
    
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            
            // Start HTTP server in a separate thread with app handle
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Runtime::new().unwrap();
                runtime.block_on(async move {
                    if let Err(e) = http_server::start_server(8737, app_handle).await {
                        eprintln!("HTTP server error: {}", e);
                    }
                });
            });
            
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_location,
            get_weather,
            get_unsplash_photo,
            get_cpu_temp,
            trigger_unsplash_download,
            get_season,
            get_holiday,
            get_time_of_day,
            build_photo_query,
            get_current_time,
            get_precipitation_display,
            is_cache_valid,
            format_time_remaining,
            get_debug_info,
            get_settings,
            save_settings,
            reset_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
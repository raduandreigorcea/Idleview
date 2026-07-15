use serde::{Deserialize, Serialize};
use chrono::{Datelike, Local, Timelike};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

// HTTP server modules
pub mod settings_manager;
pub mod http_server;
pub mod fonts;
pub mod photos;

/// Port the control-panel HTTP server listens on.
pub const HTTP_PORT: u16 = 8737;

use settings_manager::Settings;

// ===== Core functions (public for testing) =====

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(reqwest::Client::new)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

pub fn get_season_impl() -> Season {
    let season = match Local::now().month() {
        3..=5 => "spring",
        6..=8 => "summer",
        9..=11 => "autumn",
        _ => "winter",
    };

    Season { season: season.to_string() }
}

pub fn get_time_of_day_impl(sunrise_iso: Option<String>, sunset_iso: Option<String>) -> TimeOfDay {
    if let (Some(sunrise_str), Some(sunset_str)) = (sunrise_iso, sunset_iso) {
        // Open-Meteo returns local time, so these parse as naive datetimes.
        let sunrise = chrono::NaiveDateTime::parse_from_str(&sunrise_str, "%Y-%m-%dT%H:%M");
        let sunset = chrono::NaiveDateTime::parse_from_str(&sunset_str, "%Y-%m-%dT%H:%M");

        if let (Ok(sunrise), Ok(sunset)) = (sunrise, sunset) {
            let now = Local::now().naive_local();

            // Dawn is 30 minutes either side of sunrise, dusk 30 either side of sunset.
            let dawn_start = sunrise - chrono::Duration::minutes(30);
            let dawn_end = sunrise + chrono::Duration::minutes(30);
            let dusk_start = sunset - chrono::Duration::minutes(30);
            let dusk_end = sunset + chrono::Duration::minutes(30);

            let time_of_day = if now < dawn_start || now > dusk_end {
                "night"
            } else if now <= dawn_end {
                "dawn"
            } else if now >= dusk_start {
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

    // No usable sunrise/sunset (weather API down, or unparseable timestamps):
    // fall back to the local clock rather than assuming night, which would pick
    // night photos in broad daylight.
    TimeOfDay {
        time_of_day: time_of_day_from_hour(Local::now().hour()).to_string(),
        source: "fallback".to_string(),
    }
}

/// Rough time-of-day bands used when sunrise/sunset data is unavailable.
pub fn time_of_day_from_hour(hour: u32) -> &'static str {
    match hour {
        5..=7 => "dawn",
        8..=17 => "day",
        18..=20 => "dusk",
        _ => "night",
    }
}

/// The festive window the photo query and the debug panel both read. Keeping one
/// definition is what stops the two from disagreeing about when Christmas is.
pub fn holiday_for(month: u32, day: u32) -> Option<String> {
    if month == 12 && (20..=26).contains(&day) {
        return Some("christmas".to_string());
    }
    if (month == 12 && day >= 27) || (month == 1 && day <= 5) {
        return Some("new year".to_string());
    }
    if month == 10 && day >= 25 {
        return Some("halloween".to_string());
    }
    None
}

pub fn build_photo_query_impl(
    cloudcover: f64,
    rain: f64,
    showers: f64,
    snowfall: f64,
    weathercode: i32,
    sunrise_iso: Option<String>,
    sunset_iso: Option<String>,
    enable_festive: Option<bool>,
) -> PhotoQuery {
    let tod = get_time_of_day_impl(sunrise_iso, sunset_iso);
    let season = get_season_impl();

    if enable_festive.unwrap_or(true) {
        let now = Local::now();
        if let Some(holiday) = holiday_for(now.month(), now.day()) {
            return PhotoQuery { query: holiday };
        }
    }

    // Use the weathercode as a fallback for precipitation that has only just started
    // and has not yet accumulated a measurable depth.
    let has_snow = snowfall > 0.5 || matches!(weathercode, 71..=77 | 85 | 86);
    let has_rain = (rain + showers) > 0.5 || matches!(weathercode, 51..=67 | 80..=82 | 95..=99);

    // Night, dawn and dusk are distinctive enough to lead the query; during an
    // ordinary day the season leads and weather only qualifies it.
    let query = match tod.time_of_day.as_str() {
        "night" => {
            if has_snow {
                format!("{} snowy night", season.season)
            } else if has_rain {
                format!("{} rainy night", season.season)
            } else {
                format!("{} night", season.season)
            }
        }
        "dawn" => format!("{} dawn", season.season),
        "dusk" => format!("{} dusk", season.season),
        _ => {
            if has_snow {
                format!("{} snow", season.season)
            } else if has_rain {
                format!("{} rain", season.season)
            } else if cloudcover > 70.0 && season.season != "winter" {
                format!("{} cloudy", season.season)
            } else {
                season.season.to_string()
            }
        }
    };

    PhotoQuery { query }
}

pub fn get_current_time_impl() -> FormattedTime {
    let now = Local::now();
    let settings = settings_manager::read_settings().unwrap_or_default();

    let time = if settings.units.time_format == "12h" {
        now.format("%-I:%M %p").to_string()
    } else {
        now.format("%H:%M").to_string()
    };

    let date = match settings.units.date_format.as_str() {
        "mdy" => now.format("%b %d, %Y").to_string(),
        "ymd" => now.format("%Y %b %d").to_string(),
        _ => now.format("%d %b %Y").to_string(), // dmy is the default
    };

    FormattedTime {
        time,
        date,
        day_of_week: now.format("%A").to_string(),
        timestamp: now_millis(),
    }
}

pub fn get_precipitation_display_impl(weather: WeatherData) -> PrecipitationDisplay {
    let liquid_mm = weather.rain + weather.showers;

    let rain_value = if liquid_mm > 0.0 {
        format!("{:.1} mm", liquid_mm)
    } else {
        "< 0.1 mm".to_string()
    };
    let snow_value = if weather.snowfall > 0.0 {
        format!("{:.1} cm", weather.snowfall)
    } else {
        "< 0.1 cm".to_string()
    };

    let (icon, label, value) = match weather.weathercode {
        95..=99 => ("droplets.svg", "Thunder", rain_value),
        82 => ("droplets.svg", "Heavy Shower", rain_value),
        80 | 81 => ("droplets.svg", "Shower", rain_value),
        85 | 86 => ("snowflake.svg", "Snow Shower", snow_value),
        56 | 57 | 66 | 67 => ("droplet.svg", "Sleet", rain_value),
        65 => ("droplets.svg", "Heavy Rain", rain_value),
        63 => ("droplets.svg", "Rain", rain_value),
        61 => ("droplet.svg", "Light Rain", rain_value),
        51 | 53 | 55 => ("droplet.svg", "Drizzle", rain_value),
        75 => ("snowflake.svg", "Heavy Snow", snow_value),
        71 | 73 | 77 => ("snowflake.svg", "Snow", snow_value),
        45 | 48 => ("cloud-fog.svg", "Fog", "Active".to_string()),
        // Unknown code: trust the measured values if there are any, else call it clear.
        _ if weather.snowfall > 0.0 => ("snowflake.svg", "Snow", snow_value),
        _ if liquid_mm > 0.0 => ("droplets.svg", "Rain", rain_value),
        _ => ("umbrella.svg", "Precip", "Clear".to_string()),
    };

    PrecipitationDisplay {
        icon: icon.to_string(),
        label: label.to_string(),
        value,
    }
}

pub fn is_cache_valid_impl(cache_timestamp: u64) -> bool {
    let settings = settings_manager::read_settings().unwrap_or_default();
    // refresh_interval is clamped to at least 1 minute on every write, so this can
    // never collapse to a zero-length window that makes every photo instantly stale.
    let refresh_interval_ms = settings.photos.refresh_interval * 60 * 1000;

    now_millis().saturating_sub(cache_timestamp) < refresh_interval_ms
}

// ===== Tauri Commands =====

#[tauri::command]
fn get_settings() -> Result<Settings, String> {
    settings_manager::read_settings()
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<Settings, String> {
    settings_manager::write_settings(&settings)
}

#[tauri::command]
fn reset_settings() -> Result<Settings, String> {
    settings_manager::write_settings(&Settings::default())
}

/// The font catalogue, so the dashboard renders exactly the fonts (and weights) the
/// control panel offered. Neither app keeps its own copy any more.
#[tauri::command]
fn get_font_catalogue() -> fonts::FontCatalogue {
    fonts::catalogue()
}

/// What the dashboard needs to show a pairing card: where the control panel lives
/// and the token required to change anything through it.
#[tauri::command]
fn get_server_info() -> Result<ServerInfo, String> {
    Ok(ServerInfo {
        port: HTTP_PORT,
        token: settings_manager::ensure_auth_token()?,
        urls: http_server::get_local_ips()
            .into_iter()
            .map(|ip| format!("http://{}:{}", ip, HTTP_PORT))
            .collect(),
    })
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

#[derive(Debug, Serialize)]
pub struct ServerInfo {
    pub port: u16,
    pub token: String,
    pub urls: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnsplashPhoto {
    pub url: String,
    pub author: String,
    pub author_url: String,
    pub download_location: String,
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
    pub showers: f64,
    pub snowfall: f64,
    pub weathercode: i32,
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
    showers: f64,
    snowfall: f64,
    cloudcover: f64,
    wind_speed_10m: f64,
    weathercode: i32,
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
pub struct PhotoQuery {
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct FormattedTime {
    pub time: String,
    pub date: String,
    pub day_of_week: String,
    pub timestamp: u64,
}

#[derive(Debug, Serialize)]
pub struct PrecipitationDisplay {
    pub icon: String,
    pub label: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct DebugInfo {
    pub photo_age: String,
    pub query: String,
    pub time_source: String,
    pub time_of_day: String,
    /// Where photos are fetched from. Never a key - the app does not have one.
    pub photo_source: String,
    pub photo_mode: String,
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
    let settings = settings_manager::read_settings().unwrap_or_default();

    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,relative_humidity_2m,rain,showers,snowfall,cloudcover,wind_speed_10m,weathercode&daily=sunrise,sunset&timezone=auto",
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

    let temperature = match settings.units.temperature_unit.as_str() {
        "fahrenheit" => data.current.temperature_2m * 9.0 / 5.0 + 32.0,
        _ => data.current.temperature_2m,
    };

    let wind_speed = match settings.units.wind_speed_unit.as_str() {
        "mph" => data.current.wind_speed_10m * 0.621371,
        "ms" => data.current.wind_speed_10m / 3.6,
        _ => data.current.wind_speed_10m,
    };

    let wind_speed_label = match settings.units.wind_speed_unit.as_str() {
        "mph" => "mph",
        "ms" => "m/s",
        _ => "km/h",
    }
    .to_string();

    Ok(WeatherData {
        temperature,
        temperature_unit: settings.units.temperature_unit.clone(),
        humidity: data.current.relative_humidity_2m,
        wind_speed,
        wind_speed_unit: settings.units.wind_speed_unit.clone(),
        wind_speed_label,
        cloudcover: data.current.cloudcover,
        rain: data.current.rain,
        showers: data.current.showers,
        snowfall: data.current.snowfall,
        weathercode: data.current.weathercode,
        sunrise: data.daily.sunrise.first().cloned().unwrap_or_default(),
        sunset: data.daily.sunset.first().cloned().unwrap_or_default(),
        timezone: data.timezone,
    })
}

#[tauri::command]
fn build_photo_query(
    cloudcover: f64,
    rain: f64,
    showers: f64,
    snowfall: f64,
    weathercode: i32,
    sunrise_iso: Option<String>,
    sunset_iso: Option<String>,
    enable_festive: Option<bool>,
) -> PhotoQuery {
    let settings = settings_manager::read_settings().unwrap_or_default();
    let custom_query = settings.photos.custom_query.trim();
    if !custom_query.is_empty() {
        return PhotoQuery { query: custom_query.to_string() };
    }

    build_photo_query_impl(
        cloudcover, rain, showers, snowfall, weathercode, sunrise_iso, sunset_iso, enable_festive,
    )
}

/// Rewrite an Unsplash CDN URL to the size and quality we want. Parsing the URL
/// rather than splicing strings means a change to Unsplash's parameter order or
/// set cannot silently produce a malformed URL (or a duplicate `q=`).
fn build_photo_url(base: &str, width: u32, height: u32, quality: u8) -> Result<String, String> {
    let mut url = reqwest::Url::parse(base)
        .map_err(|e| format!("Unsplash returned an unparseable photo URL: {}", e))?;

    // Keep Unsplash's own tracking/identity params, drop the sizing ones we set.
    let ours = ["w", "h", "fit", "q", "t"];
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| !ours.contains(&key.as_ref()))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();

    let mut query = url.query_pairs_mut();
    query.clear();
    for (key, value) in &kept {
        query.append_pair(key, value);
    }
    query
        .append_pair("w", &width.to_string())
        .append_pair("h", &height.to_string())
        .append_pair("fit", "crop")
        .append_pair("q", &quality.to_string())
        // Cache-buster: without it the webview will happily reuse the previous image
        // when two queries resolve to the same photo.
        .append_pair("t", &now_millis().to_string());
    drop(query);

    Ok(url.to_string())
}

#[tauri::command]
async fn get_unsplash_photo(width: u32, height: u32, query: String) -> Result<UnsplashPhoto, String> {
    // The key lives in the proxy, not here - see src/photos.rs.
    let photo = photos::fetch_photo(http_client(), &query).await?;

    let settings = settings_manager::read_settings().unwrap_or_default();

    Ok(UnsplashPhoto {
        url: build_photo_url(&photo.raw_url, width, height, settings.photos.photo_quality)?,
        author: photo.author,
        author_url: photo.author_url,
        download_location: photo.download_location,
    })
}

#[tauri::command]
async fn trigger_unsplash_download(download_url: String) -> Result<(), String> {
    photos::trigger_download(http_client(), &download_url).await;
    Ok(())
}

#[tauri::command]
fn get_current_time() -> FormattedTime {
    get_current_time_impl()
}

/// Publish the photo the dashboard just displayed. This is in-process state, so the
/// dashboard calls it directly instead of making an HTTP round-trip to our own
/// server - which also means the API needs no cross-origin grant for any browser.
#[tauri::command]
fn set_current_photo(
    photos: tauri::State<'_, http_server::PhotoChannel>,
    photo: http_server::CurrentPhoto,
) -> Result<(), String> {
    photos.set(photo)
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
fn get_debug_info(
    cache_timestamp: Option<u64>,
    query: Option<String>,
    sunrise_iso: Option<String>,
    sunset_iso: Option<String>,
    temperature: Option<f64>,
    rain: Option<f64>,
    snowfall: Option<f64>,
    cloudcover: Option<f64>,
) -> DebugInfo {
    let photo_age = cache_timestamp
        .map(|ts| {
            let seconds = now_millis().saturating_sub(ts) / 1000;
            let minutes = seconds / 60;
            let hours = minutes / 60;
            if seconds < 60 {
                format!("{}s ago", seconds)
            } else if minutes < 60 {
                format!("{}m ago", minutes)
            } else if hours < 24 {
                format!("{}h ago", hours)
            } else {
                format!("{}d ago", hours / 24)
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    let tod = get_time_of_day_impl(sunrise_iso, sunset_iso);
    let season = get_season_impl();

    // Reports where photos come from, never a key - the app never holds one. Every
    // build fetches through the proxy, so this is always the proxy's address.
    let photo_source = photos::proxy_base();

    let settings = settings_manager::read_settings().unwrap_or_default();
    let temp_unit = settings.units.temperature_unit.as_str();

    DebugInfo {
        photo_age,
        query: query.unwrap_or_else(|| "n/a".to_string()),
        time_source: tod.source,
        time_of_day: tod.time_of_day,
        photo_source,
        photo_mode: "Proxy".to_string(),
        temperature: temperature
            .map(|t| {
                if temp_unit == "fahrenheit" {
                    format!("{:.1}\u{00b0}F", t)
                } else {
                    format!("{:.1}\u{00b0}C", t)
                }
            })
            .unwrap_or_else(|| "n/a".to_string()),
        rain: rain.map(|r| format!("{:.1}mm", r)).unwrap_or_else(|| "n/a".to_string()),
        snowfall: snowfall.map(|s| format!("{:.1}cm", s)).unwrap_or_else(|| "n/a".to_string()),
        cloudcover: cloudcover.map(|c| format!("{}%", c as i32)).unwrap_or_else(|| "n/a".to_string()),
        season: season.season,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holiday_windows_are_bounded() {
        assert_eq!(holiday_for(12, 25).as_deref(), Some("christmas"));
        assert_eq!(holiday_for(12, 20).as_deref(), Some("christmas"));
        assert_eq!(holiday_for(12, 26).as_deref(), Some("christmas"));
        assert_eq!(holiday_for(12, 31).as_deref(), Some("new year"));
        assert_eq!(holiday_for(1, 5).as_deref(), Some("new year"));
        assert_eq!(holiday_for(10, 31).as_deref(), Some("halloween"));

        // Early December is ordinary winter, not Christmas.
        assert_eq!(holiday_for(12, 1), None);
        assert_eq!(holiday_for(12, 19), None);
        assert_eq!(holiday_for(1, 6), None);
        assert_eq!(holiday_for(10, 24), None);
        assert_eq!(holiday_for(4, 10), None);
    }

    #[test]
    fn hour_fallback_is_not_always_night() {
        assert_eq!(time_of_day_from_hour(0), "night");
        assert_eq!(time_of_day_from_hour(6), "dawn");
        assert_eq!(time_of_day_from_hour(12), "day");
        assert_eq!(time_of_day_from_hour(19), "dusk");
        assert_eq!(time_of_day_from_hour(23), "night");
    }

    #[test]
    fn missing_sun_times_fall_back_to_the_clock() {
        let tod = get_time_of_day_impl(None, None);
        assert_eq!(tod.source, "fallback");
        assert_eq!(tod.time_of_day, time_of_day_from_hour(Local::now().hour()));
    }

    #[test]
    fn unparseable_sun_times_fall_back_to_the_clock() {
        let tod = get_time_of_day_impl(Some("not-a-time".into()), Some("also-not".into()));
        assert_eq!(tod.source, "fallback");
    }

    #[test]
    fn sun_times_place_midday_between_dawn_and_dusk() {
        let now = Local::now().naive_local();
        let sunrise = (now - chrono::Duration::hours(4)).format("%Y-%m-%dT%H:%M").to_string();
        let sunset = (now + chrono::Duration::hours(4)).format("%Y-%m-%dT%H:%M").to_string();

        let tod = get_time_of_day_impl(Some(sunrise), Some(sunset));
        assert_eq!(tod.source, "api");
        assert_eq!(tod.time_of_day, "day");
    }

    #[test]
    fn after_dusk_is_night() {
        let now = Local::now().naive_local();
        let sunrise = (now - chrono::Duration::hours(8)).format("%Y-%m-%dT%H:%M").to_string();
        let sunset = (now - chrono::Duration::hours(2)).format("%Y-%m-%dT%H:%M").to_string();

        let tod = get_time_of_day_impl(Some(sunrise), Some(sunset));
        assert_eq!(tod.source, "api");
        assert_eq!(tod.time_of_day, "night");
    }

    #[test]
    fn festive_queries_can_be_disabled() {
        let q = build_photo_query_impl(0.0, 0.0, 0.0, 0.0, 0, None, None, Some(false));
        assert!(!["christmas", "new year", "halloween"].contains(&q.query.as_str()));
    }

    #[test]
    fn precipitation_is_derived_from_the_weathercode() {
        let weather = |weathercode, rain: f64, snowfall: f64| WeatherData {
            temperature: 0.0,
            temperature_unit: "celsius".to_string(),
            humidity: 0.0,
            wind_speed: 0.0,
            wind_speed_unit: "kmh".to_string(),
            wind_speed_label: "km/h".to_string(),
            cloudcover: 0.0,
            rain,
            showers: 0.0,
            snowfall,
            weathercode,
            sunrise: String::new(),
            sunset: String::new(),
            timezone: "UTC".to_string(),
        };

        let thunder = get_precipitation_display_impl(weather(95, 3.0, 0.0));
        assert_eq!(thunder.label, "Thunder");
        assert_eq!(thunder.value, "3.0 mm");

        let snow = get_precipitation_display_impl(weather(75, 0.0, 5.0));
        assert_eq!(snow.label, "Heavy Snow");
        assert_eq!(snow.value, "5.0 cm");
        assert_eq!(snow.icon, "snowflake.svg");

        let fog = get_precipitation_display_impl(weather(45, 0.0, 0.0));
        assert_eq!(fog.label, "Fog");

        // Unknown code with no measurable precipitation reads as clear.
        let clear = get_precipitation_display_impl(weather(0, 0.0, 0.0));
        assert_eq!(clear.label, "Precip");
        assert_eq!(clear.value, "Clear");

        // Unknown code but measurable snow still reports snow.
        let odd = get_precipitation_display_impl(weather(3, 0.0, 2.0));
        assert_eq!(odd.label, "Snow");
        assert_eq!(odd.value, "2.0 cm");
    }

    #[test]
    fn photo_url_replaces_sizing_params_without_duplicating_them() {
        let url = build_photo_url(
            "https://images.unsplash.com/photo-1?ixid=ABC&q=80&w=1080&fit=max",
            1920,
            1080,
            95,
        )
        .unwrap();

        // Unsplash's own params survive, ours are set exactly once.
        assert!(url.contains("ixid=ABC"));
        assert_eq!(url.matches("q=").count(), 1);
        assert_eq!(url.matches("w=").count(), 1);
        assert!(url.contains("q=95"));
        assert!(url.contains("w=1920"));
        assert!(url.contains("h=1080"));
        assert!(url.contains("fit=crop"));
        assert!(!url.contains("fit=max"));
    }

    #[test]
    fn photo_url_handles_a_base_with_no_query_at_all() {
        let url = build_photo_url("https://images.unsplash.com/photo-2", 800, 600, 65).unwrap();
        assert!(url.contains("?"));
        assert!(url.contains("q=65"));
        assert_eq!(url.matches("q=").count(), 1);
    }

    #[test]
    fn photo_url_rejects_garbage_rather_than_producing_a_broken_url() {
        assert!(build_photo_url("not a url", 800, 600, 80).is_err());
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load .env if present, so IDLEVIEW_PHOTO_PROXY can point the app at a local
    // `wrangler dev` worker. The app never holds an Unsplash key - photos always go
    // through the proxy, dev and release alike.
    let _ = dotenvy::dotenv();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_handle = app.handle().clone();

            // One photo channel, shared by the HTTP server and the set_current_photo
            // command, so both publish to the same SSE subscribers.
            let photos = http_server::PhotoChannel::new();
            app.manage(photos.clone());

            std::thread::spawn(move || {
                let runtime = match tokio::runtime::Runtime::new() {
                    Ok(runtime) => runtime,
                    Err(e) => {
                        eprintln!("Failed to start HTTP runtime: {}", e);
                        return;
                    }
                };
                runtime.block_on(async move {
                    if let Err(e) = http_server::start_server(HTTP_PORT, app_handle, photos).await {
                        eprintln!("HTTP server error: {}", e);
                    }
                });
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_location,
            get_weather,
            get_unsplash_photo,
            trigger_unsplash_download,
            build_photo_query,
            get_current_time,
            set_current_photo,
            get_precipitation_display,
            is_cache_valid,
            get_debug_info,
            get_settings,
            save_settings,
            reset_settings,
            get_server_info,
            get_font_catalogue,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

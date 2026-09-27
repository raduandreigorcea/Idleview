//! Everything the screen does, except drawing.
//!
//! One task owns the schedule: location once, weather every 15 minutes (5 while rain or
//! snow is starting or stopping), a photo every `refresh_interval` (from Unsplash via
//! the proxy, or from the user's own library), and a clock tick on each minute. Whenever the result would look different it emits a finished
//! `idleview_core::View` as the `view` event. The webview only places text.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDateTime, Timelike, Utc};
use idleview_core::{Photo, Precip, View, Weather};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

use crate::http_server::PhotoChannel;
use crate::settings_manager::{self, Settings};
use crate::{library, photos};

const WEATHER_EVERY: Duration = Duration::from_secs(15 * 60);
const WEATHER_WHILE_CHANGING: Duration = Duration::from_secs(5 * 60);
/// After a failed photo fetch. The minute tick would otherwise hammer a proxy that is
/// most likely failing because it is over quota.
const PHOTO_RETRY: Duration = Duration::from_secs(5 * 60);
/// The most often Unsplash is ever asked, whatever the reason - schedule, switching the
/// photo source back, or anything else a client can do.
const UNSPLASH_MIN_GAP: Duration = Duration::from_secs(5 * 60);

/// Handle to the running dashboard, managed as Tauri state.
#[derive(Clone)]
pub struct Dashboard {
    wake: Arc<Notify>,
    latest: Arc<Mutex<Option<View>>>,
}

impl Dashboard {
    /// Something changed (settings, the photo library): re-check and redraw now.
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// The last view emitted, for a webview that loads after the first emit.
    pub fn latest(&self) -> Option<View> {
        self.latest.lock().ok().and_then(|view| view.clone())
    }
}

pub fn start(app: AppHandle, photo_channel: PhotoChannel) -> Dashboard {
    let wake = Arc::new(Notify::new());
    let latest = Arc::new(Mutex::new(None));
    tauri::async_runtime::spawn(run(app, photo_channel, wake.clone(), latest.clone()));
    Dashboard { wake, latest }
}

struct Location {
    latitude: f64,
    longitude: f64,
    label: String,
}

/// The shown photo, persisted so a restart shows it immediately instead of a blank
/// screen while the network comes up.
#[derive(Serialize, Deserialize)]
struct CachedPhoto {
    photo: Photo,
    query: String,
    /// Unix seconds, so the age survives a restart.
    fetched_at: i64,
    /// Set when the photo is one of the user's own, from `library`.
    #[serde(default)]
    local_id: Option<String>,
}

struct State {
    client: reqwest::Client,
    location: Option<Location>,
    weather: Option<Weather>,
    weather_due: Instant,
    precip: Option<Precip>,
    photo: Option<CachedPhoto>,
    photo_retry_at: Instant,
    unsplash_allowed_at: Instant,
    /// The Unsplash photo set aside while the user's own photos are showing, so
    /// switching back shows it at once instead of a dark screen until a fetch is allowed.
    set_aside: Option<CachedPhoto>,
}

async fn run(
    app: AppHandle,
    photo_channel: PhotoChannel,
    wake: Arc<Notify>,
    latest: Arc<Mutex<Option<View>>>,
) {
    let mut state = State {
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default(),
        location: None,
        weather: None,
        weather_due: Instant::now(),
        precip: None,
        photo: load_cached_photo(),
        photo_retry_at: Instant::now(),
        unsplash_allowed_at: Instant::now(),
        set_aside: None,
    };
    if let Some(cached) = &state.photo {
        let _ = photo_channel.set(panel_photo(cached));
    }

    loop {
        let settings = settings_manager::read_settings().unwrap_or_default();
        // Publish before and after the network work, so a slow fetch never holds
        // the clock back.
        publish(&app, &latest, &state, &settings);
        state.update(&app, &photo_channel, &settings).await;
        publish(&app, &latest, &state, &settings);

        tokio::select! {
            _ = tokio::time::sleep(until_next_minute()) => {}
            _ = wake.notified() => {}
        }
    }
}

impl State {
    async fn update(&mut self, app: &AppHandle, photo_channel: &PhotoChannel, settings: &Settings) {
        if self.location.is_none() {
            match fetch_location(&self.client).await {
                Ok(location) => self.location = Some(location),
                Err(e) => eprintln!("Location unavailable, retrying next minute: {}", e),
            }
        }

        if let Some(location) = &self.location {
            if Instant::now() >= self.weather_due {
                match fetch_weather(&self.client, location).await {
                    Ok(weather) => {
                        let precip = idleview_core::precip(&weather);
                        let changing = self.precip.is_some_and(|last| last != precip);
                        self.precip = Some(precip);
                        self.weather = Some(weather);
                        self.weather_due = Instant::now()
                            + if changing { WEATHER_WHILE_CHANGING } else { WEATHER_EVERY };
                    }
                    // weather_due stays in the past, so the next minute tick retries.
                    Err(e) => eprintln!("Weather unavailable, retrying next minute: {}", e),
                }
            }
        }

        let max_age = (settings.photos.refresh_interval * 60) as i64;
        let local = settings.photos.local();

        let showing_unsplash = self.photo.as_ref().is_some_and(|cached| cached.local_id.is_none());
        if local && showing_unsplash {
            self.set_aside = self.photo.take();
        } else if !local && !showing_unsplash {
            if let Some(cached) = self.set_aside.take() {
                let _ = photo_channel.set(panel_photo(&cached));
                self.photo = Some(cached);
            }
        }

        let current = self.photo.as_ref().filter(|cached| match &cached.local_id {
            // A deleted photo is no longer current.
            Some(id) => local && library::photo_path(id).is_some_and(|path| path.exists()),
            None => !local,
        });
        let stale = current.is_none_or(|cached| Utc::now().timestamp() - cached.fetched_at >= max_age);
        if !stale {
            return;
        }

        if local {
            // The user's own photos only. Unsplash is never contacted in this mode, and
            // an empty library leaves the screen dark rather than falling back to it.
            let current_id = current.and_then(|cached| cached.local_id.clone());
            self.photo = library::pick(current_id.as_deref()).and_then(|id| {
                let path = library::photo_path(&id)?;
                Some(CachedPhoto {
                    photo: Photo { url: asset_url(&path), author: String::new(), author_url: String::new() },
                    query: String::new(),
                    fetched_at: Utc::now().timestamp(),
                    local_id: Some(id),
                })
            });
            match &self.photo {
                Some(cached) => {
                    save_cached_photo(cached);
                    let _ = photo_channel.set(panel_photo(cached));
                }
                None => photo_channel.clear(),
            }
            return;
        }

        let now = Instant::now();
        if now < self.photo_retry_at || now < self.unsplash_allowed_at {
            return;
        }
        self.unsplash_allowed_at = now + UNSPLASH_MIN_GAP;

        let query = idleview_core::photo_query(
            local_now(),
            self.weather.as_ref(),
            settings.photos.enable_festive_queries,
        );
        let (width, height) = screen_size(app);

        match photos::fetch(&self.client, &query, width, height).await {
            Ok(fetched) => {
                let client = self.client.clone();
                tauri::async_runtime::spawn(async move {
                    photos::trigger_download(&client, &fetched.download_location).await;
                });

                let cached = CachedPhoto {
                    photo: fetched.photo,
                    query,
                    fetched_at: Utc::now().timestamp(),
                    local_id: None,
                };
                save_cached_photo(&cached);
                let _ = photo_channel.set(panel_photo(&cached));
                self.photo = Some(cached);
            }
            Err(e) => {
                // Keep showing the old photo.
                eprintln!("Photo fetch failed for {:?}: {}", query, e);
                self.photo_retry_at = Instant::now() + PHOTO_RETRY;
            }
        }
    }
}

/// What the control panel shows for the current photo. A local photo is given by its
/// thumbnail URL on the panel's own server (token-gated), since the panel cannot read
/// the screen's disk.
fn panel_photo(cached: &CachedPhoto) -> Photo {
    match &cached.local_id {
        Some(id) => Photo {
            url: format!("/api/photos/{id}/thumb"),
            author: String::new(),
            author_url: String::new(),
        },
        None => cached.photo.clone(),
    }
}

/// A file URL the webview may load through Tauri's asset protocol (the scope is limited
/// to the library directory in `lib.rs`). Same format as the JS `convertFileSrc`.
fn asset_url(path: &std::path::Path) -> String {
    let encoded = urlencoding::encode(&path.to_string_lossy()).into_owned();
    if cfg!(windows) {
        format!("http://asset.localhost/{encoded}")
    } else {
        format!("asset://localhost/{encoded}")
    }
}

/// Emit the view if it differs from the last one.
fn publish(app: &AppHandle, latest: &Mutex<Option<View>>, state: &State, settings: &Settings) {
    let view = idleview_core::view(
        local_now(),
        &settings.units,
        &settings.display,
        state.location.as_ref().map(|location| location.label.as_str()),
        state.weather.as_ref(),
        state.photo.as_ref().map(|cached| &cached.photo),
    );

    let Ok(mut last) = latest.lock() else { return };
    if last.as_ref() == Some(&view) {
        return;
    }
    let _ = app.emit("view", &view);
    *last = Some(view);
}

fn local_now() -> NaiveDateTime {
    Local::now().naive_local()
}

/// Sleep until just past the next minute boundary, so the clock flips on time.
fn until_next_minute() -> Duration {
    let now = Local::now();
    let into_minute = now.second() as u64 * 1000 + (now.nanosecond() / 1_000_000).min(999) as u64;
    Duration::from_millis(60_000 - into_minute + 50)
}

/// Physical pixels, so a HiDPI screen gets a sharp photo.
fn screen_size(app: &AppHandle) -> (u32, u32) {
    app.get_webview_window("main")
        .and_then(|window| window.inner_size().ok())
        .filter(|size| size.width > 0 && size.height > 0)
        .map(|size| (size.width, size.height))
        .unwrap_or((1920, 1080))
}

async fn fetch_location(client: &reqwest::Client) -> Result<Location, String> {
    #[derive(Deserialize)]
    struct IpApi {
        lat: f64,
        lon: f64,
        city: Option<String>,
    }

    let data: IpApi = client
        .get("http://ip-api.com/json/")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    Ok(Location {
        latitude: data.lat,
        longitude: data.lon,
        label: data
            .city
            .filter(|city| !city.trim().is_empty())
            .unwrap_or_else(|| format!("{:.2}°, {:.2}°", data.lat, data.lon)),
    })
}

async fn fetch_weather(client: &reqwest::Client, location: &Location) -> Result<Weather, String> {
    #[derive(Deserialize)]
    struct Response {
        current: Current,
        daily: Daily,
    }
    #[derive(Deserialize)]
    struct Current {
        temperature_2m: f64,
        relative_humidity_2m: f64,
        rain: f64,
        showers: f64,
        snowfall: f64,
        cloudcover: f64,
        wind_speed_10m: f64,
        weathercode: i32,
    }
    #[derive(Deserialize)]
    struct Daily {
        sunrise: Vec<String>,
        sunset: Vec<String>,
    }

    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,relative_humidity_2m,rain,showers,snowfall,cloudcover,wind_speed_10m,weathercode&daily=sunrise,sunset&timezone=auto",
        location.latitude, location.longitude
    );
    let data: Response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    // Open-Meteo gives local times at the location, e.g. "2026-04-26T06:13".
    let first_time = |times: &[String]| {
        times
            .first()
            .and_then(|t| NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M").ok())
    };

    Ok(Weather {
        temperature_c: data.current.temperature_2m,
        humidity: data.current.relative_humidity_2m,
        wind_kmh: data.current.wind_speed_10m,
        cloudcover: data.current.cloudcover,
        rain_mm: data.current.rain,
        showers_mm: data.current.showers,
        snowfall_cm: data.current.snowfall,
        weathercode: data.current.weathercode,
        sunrise: first_time(&data.daily.sunrise),
        sunset: first_time(&data.daily.sunset),
    })
}

fn cache_path() -> Option<std::path::PathBuf> {
    settings_manager::get_settings_path()
        .ok()
        .map(|path| path.with_file_name("photo.json"))
}

fn load_cached_photo() -> Option<CachedPhoto> {
    let content = std::fs::read_to_string(cache_path()?).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_cached_photo(cached: &CachedPhoto) {
    let Some(path) = cache_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let result = serde_json::to_string(cached)
        .map_err(|e| e.to_string())
        .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()));
    if let Err(e) = result {
        eprintln!("Could not cache the photo (it is still shown): {}", e);
    }
}

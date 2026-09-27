//! What the Idleview screen shows, as pure functions.
//!
//! No network, no files, no clock reads: every function takes "now" as an argument.
//! That keeps the tests deterministic, and lets code running somewhere other than the
//! screen (the photo Worker runs in UTC) pass in the screen's local time.

use chrono::{Datelike, Duration, NaiveDateTime, Timelike};
use serde::{Deserialize, Serialize};

// ===== Inputs =====

/// Current conditions, always metric, as Open-Meteo reports them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    pub temperature_c: f64,
    pub humidity: f64,
    pub wind_kmh: f64,
    pub cloudcover: f64,
    pub rain_mm: f64,
    pub showers_mm: f64,
    pub snowfall_cm: f64,
    pub weathercode: i32,
    /// Local time at the location (Open-Meteo's `timezone=auto`).
    pub sunrise: Option<NaiveDateTime>,
    pub sunset: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    pub url: String,
    pub author: String,
    pub author_url: String,
}

/// Unit choices. Strings rather than enums so a settings file holding a value this build
/// does not know still loads; `validate` then snaps it to a default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Units {
    pub temperature_unit: String, // "celsius" | "fahrenheit"
    pub time_format: String,      // "24h" | "12h"
    pub date_format: String,      // "dmy" | "mdy" | "ymd"
    pub wind_speed_unit: String,  // "kmh" | "mph" | "ms"
}

impl Default for Units {
    fn default() -> Self {
        Self {
            temperature_unit: "celsius".into(),
            time_format: "24h".into(),
            date_format: "dmy".into(),
            wind_speed_unit: "kmh".into(),
        }
    }
}

fn clamp_choice(value: &mut String, allowed: &[&str], fallback: &str) {
    let lowered = value.trim().to_lowercase();
    *value = if allowed.contains(&lowered.as_str()) { lowered } else { fallback.to_string() };
}

impl Units {
    pub fn validate(&mut self) {
        clamp_choice(&mut self.temperature_unit, &["celsius", "fahrenheit"], "celsius");
        clamp_choice(&mut self.time_format, &["24h", "12h"], "24h");
        clamp_choice(&mut self.date_format, &["dmy", "mdy", "ymd"], "dmy");
        clamp_choice(&mut self.wind_speed_unit, &["kmh", "mph", "ms"], "kmh");
    }

    fn is_12h(&self) -> bool {
        self.time_format == "12h"
    }
}

/// Which parts of the screen are shown. Missing fields default to shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Visibility {
    pub show_clock: bool,
    pub show_date: bool,
    pub show_weekday: bool,
    pub show_location: bool,
    pub show_temperature: bool,
    pub show_sunrise_sunset: bool,
    pub show_precipitation_cloudiness: bool,
    pub show_humidity_wind: bool,
}

impl Default for Visibility {
    fn default() -> Self {
        Self {
            show_clock: true,
            show_date: true,
            show_weekday: true,
            show_location: true,
            show_temperature: true,
            show_sunrise_sunset: true,
            show_precipitation_cloudiness: true,
            show_humidity_wind: true,
        }
    }
}

// ===== Photo search =====

pub fn season(month: u32) -> &'static str {
    match month {
        3..=5 => "spring",
        6..=8 => "summer",
        9..=11 => "autumn",
        _ => "winter",
    }
}

pub fn holiday(month: u32, day: u32) -> Option<&'static str> {
    match (month, day) {
        (12, 20..=26) => Some("christmas"),
        (12, 27..=31) | (1, 1..=5) => Some("new year"),
        (10, 25..=31) => Some("halloween"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeOfDay {
    Dawn,
    Day,
    Dusk,
    Night,
}

impl TimeOfDay {
    pub fn as_str(self) -> &'static str {
        match self {
            TimeOfDay::Dawn => "dawn",
            TimeOfDay::Day => "day",
            TimeOfDay::Dusk => "dusk",
            TimeOfDay::Night => "night",
        }
    }
}

/// Dawn and dusk are 30 minutes either side of sunrise and sunset. Without sun times
/// (weather unavailable) fall back to rough clock bands rather than assuming night.
pub fn time_of_day(
    now: NaiveDateTime,
    sunrise: Option<NaiveDateTime>,
    sunset: Option<NaiveDateTime>,
) -> TimeOfDay {
    if let (Some(sunrise), Some(sunset)) = (sunrise, sunset) {
        let half_hour = Duration::minutes(30);
        return if now < sunrise - half_hour || now > sunset + half_hour {
            TimeOfDay::Night
        } else if now <= sunrise + half_hour {
            TimeOfDay::Dawn
        } else if now >= sunset - half_hour {
            TimeOfDay::Dusk
        } else {
            TimeOfDay::Day
        };
    }

    match now.hour() {
        5..=7 => TimeOfDay::Dawn,
        8..=17 => TimeOfDay::Day,
        18..=20 => TimeOfDay::Dusk,
        _ => TimeOfDay::Night,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precip {
    None,
    Rain,
    Snow,
}

/// Coarse precipitation state. The weathercode catches rain or snow that has only just
/// started and has not yet accumulated a measurable amount.
pub fn precip(weather: &Weather) -> Precip {
    let code = weather.weathercode;
    if weather.snowfall_cm > 0.5 || matches!(code, 71..=77 | 85 | 86) {
        Precip::Snow
    } else if weather.rain_mm + weather.showers_mm > 0.5 || matches!(code, 51..=67 | 80..=82 | 95..=99) {
        Precip::Rain
    } else {
        Precip::None
    }
}

/// The Unsplash search for right now. Night, dawn and dusk lead the query because they
/// are the most recognisable; during the day the season leads and weather qualifies it.
pub fn photo_query(now: NaiveDateTime, weather: Option<&Weather>, festive: bool) -> String {
    if festive {
        if let Some(name) = holiday(now.month(), now.day()) {
            return name.to_string();
        }
    }

    let season = season(now.month());
    let tod = time_of_day(now, weather.and_then(|w| w.sunrise), weather.and_then(|w| w.sunset));
    let precip = weather.map(precip).unwrap_or(Precip::None);
    let cloudy = weather.is_some_and(|w| w.cloudcover > 70.0) && season != "winter";

    match (tod, precip) {
        (TimeOfDay::Night, Precip::Snow) => format!("{season} snowy night"),
        (TimeOfDay::Night, Precip::Rain) => format!("{season} rainy night"),
        (TimeOfDay::Night, Precip::None) => format!("{season} night"),
        (TimeOfDay::Dawn, _) => format!("{season} dawn"),
        (TimeOfDay::Dusk, _) => format!("{season} dusk"),
        (TimeOfDay::Day, Precip::Snow) => format!("{season} snow"),
        (TimeOfDay::Day, Precip::Rain) => format!("{season} rain"),
        (TimeOfDay::Day, Precip::None) if cloudy => format!("{season} cloudy"),
        (TimeOfDay::Day, Precip::None) => season.to_string(),
    }
}

// ===== What the screen shows =====

/// Everything the renderer needs, already formatted. The renderer only places text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct View {
    pub time: String,
    /// "AM"/"PM" in 12-hour mode, drawn smaller beside the time.
    pub period: Option<String>,
    pub weekday: String,
    pub date: String,
    pub location: Option<String>,
    pub weather: Option<WeatherView>,
    pub photo: Option<Photo>,
    pub show: Visibility,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WeatherView {
    pub temperature: String,
    pub sunrise: String,
    pub sunset: String,
    /// File name under `assets/`.
    pub precip_icon: String,
    pub precip_label: String,
    pub precip_value: String,
    pub humidity: String,
    pub wind: String,
    pub clouds: String,
}

pub fn view(
    now: NaiveDateTime,
    units: &Units,
    show: &Visibility,
    location: Option<&str>,
    weather: Option<&Weather>,
    photo: Option<&Photo>,
) -> View {
    let (time, period) = if units.is_12h() {
        (now.format("%-I:%M").to_string(), Some(now.format("%p").to_string()))
    } else {
        (now.format("%H:%M").to_string(), None)
    };

    let date = match units.date_format.as_str() {
        "mdy" => now.format("%b %d, %Y"),
        "ymd" => now.format("%Y %b %d"),
        _ => now.format("%d %b %Y"),
    }
    .to_string();

    View {
        time,
        period,
        weekday: now.format("%A").to_string(),
        date,
        location: location.map(str::to_string),
        weather: weather.map(|w| weather_view(w, units)),
        photo: photo.cloned(),
        show: show.clone(),
    }
}

fn weather_view(weather: &Weather, units: &Units) -> WeatherView {
    let (temperature, temp_label) = match units.temperature_unit.as_str() {
        "fahrenheit" => (weather.temperature_c * 9.0 / 5.0 + 32.0, "°F"),
        _ => (weather.temperature_c, "°C"),
    };
    let (wind, wind_label) = match units.wind_speed_unit.as_str() {
        "mph" => (weather.wind_kmh * 0.621371, "mph"),
        "ms" => (weather.wind_kmh / 3.6, "m/s"),
        _ => (weather.wind_kmh, "km/h"),
    };
    let sun = |time: Option<NaiveDateTime>| match time {
        None => "--:--".to_string(),
        Some(t) if units.is_12h() => t.format("%-I:%M%P").to_string(),
        Some(t) => t.format("%H:%M").to_string(),
    };
    let (icon, label, value) = precipitation(weather);

    WeatherView {
        temperature: format!("{} {}", whole(temperature), temp_label),
        sunrise: sun(weather.sunrise),
        sunset: sun(weather.sunset),
        precip_icon: icon.to_string(),
        precip_label: label.to_string(),
        precip_value: value,
        humidity: format!("{}%", whole(weather.humidity)),
        wind: format!("{} {}", whole(wind), wind_label),
        clouds: format!("{}%", whole(weather.cloudcover)),
    }
}

/// Rounded for display. Integer, so -0.4 °C reads "0 °C" rather than "-0 °C".
fn whole(value: f64) -> i64 {
    value.round() as i64
}

/// Icon, label and value for the precipitation tile, from the WMO weathercode.
fn precipitation(weather: &Weather) -> (&'static str, &'static str, String) {
    let liquid = weather.rain_mm + weather.showers_mm;
    let rain = if liquid > 0.0 { format!("{liquid:.1} mm") } else { "< 0.1 mm".to_string() };
    let snow = if weather.snowfall_cm > 0.0 {
        format!("{:.1} cm", weather.snowfall_cm)
    } else {
        "< 0.1 cm".to_string()
    };

    match weather.weathercode {
        95..=99 => ("droplets.svg", "Thunder", rain),
        82 => ("droplets.svg", "Heavy Shower", rain),
        80 | 81 => ("droplets.svg", "Shower", rain),
        85 | 86 => ("snowflake.svg", "Snow Shower", snow),
        56 | 57 | 66 | 67 => ("droplet.svg", "Sleet", rain),
        65 => ("droplets.svg", "Heavy Rain", rain),
        63 => ("droplets.svg", "Rain", rain),
        61 => ("droplet.svg", "Light Rain", rain),
        51 | 53 | 55 => ("droplet.svg", "Drizzle", rain),
        75 => ("snowflake.svg", "Heavy Snow", snow),
        71 | 73 | 77 => ("snowflake.svg", "Snow", snow),
        45 | 48 => ("cloud-fog.svg", "Fog", "Active".to_string()),
        // Unknown code: trust the measurements if there are any, else call it clear.
        _ if weather.snowfall_cm > 0.0 => ("snowflake.svg", "Snow", snow),
        _ if liquid > 0.0 => ("droplets.svg", "Rain", rain),
        _ => ("umbrella.svg", "Precip", "Clear".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, month, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    fn weather() -> Weather {
        Weather {
            temperature_c: 18.2,
            humidity: 50.0,
            wind_kmh: 12.3,
            cloudcover: 0.0,
            rain_mm: 0.0,
            showers_mm: 0.0,
            snowfall_cm: 0.0,
            weathercode: 0,
            sunrise: Some(at(4, 26, 6, 13)),
            sunset: Some(at(4, 26, 20, 12)),
        }
    }

    #[test]
    fn renders_the_screenshot() {
        let view = view(
            at(4, 26, 10, 42),
            &Units { date_format: "mdy".into(), ..Units::default() },
            &Visibility::default(),
            Some("Bucharest"),
            Some(&weather()),
            None,
        );
        assert_eq!(view.time, "10:42");
        assert_eq!(view.period, None);
        assert_eq!(view.weekday, "Sunday");
        assert_eq!(view.date, "Apr 26, 2026");

        let w = view.weather.unwrap();
        assert_eq!(w.temperature, "18 °C");
        assert_eq!(w.sunrise, "06:13");
        assert_eq!(w.sunset, "20:12");
        assert_eq!((w.precip_label.as_str(), w.precip_value.as_str()), ("Precip", "Clear"));
        assert_eq!(w.humidity, "50%");
        assert_eq!(w.wind, "12 km/h");
        assert_eq!(w.clouds, "0%");
    }

    #[test]
    fn twelve_hour_and_imperial_units() {
        let units = Units {
            temperature_unit: "fahrenheit".into(),
            time_format: "12h".into(),
            wind_speed_unit: "mph".into(),
            ..Units::default()
        };
        let view = view(at(4, 26, 15, 5), &units, &Visibility::default(), None, Some(&weather()), None);
        assert_eq!(view.time, "3:05");
        assert_eq!(view.period.as_deref(), Some("PM"));
        assert_eq!(view.date, "26 Apr 2026");

        let w = view.weather.unwrap();
        assert_eq!(w.temperature, "65 °F");
        assert_eq!(w.wind, "8 mph");
        assert_eq!(w.sunrise, "6:13am");
        assert_eq!(w.sunset, "8:12pm");
    }

    #[test]
    fn unknown_units_snap_to_defaults() {
        let mut units = Units { temperature_unit: "Kelvin".into(), time_format: "12H".into(), ..Units::default() };
        units.validate();
        assert_eq!(units.temperature_unit, "celsius");
        assert_eq!(units.time_format, "12h");
    }

    #[test]
    fn holidays_are_bounded() {
        assert_eq!(holiday(12, 20), Some("christmas"));
        assert_eq!(holiday(12, 26), Some("christmas"));
        assert_eq!(holiday(12, 31), Some("new year"));
        assert_eq!(holiday(1, 5), Some("new year"));
        assert_eq!(holiday(10, 31), Some("halloween"));
        assert_eq!(holiday(12, 19), None);
        assert_eq!(holiday(1, 6), None);
        assert_eq!(holiday(10, 24), None);
    }

    #[test]
    fn time_of_day_uses_sun_times_then_the_clock() {
        let (rise, set) = (Some(at(4, 26, 6, 0)), Some(at(4, 26, 20, 0)));
        assert_eq!(time_of_day(at(4, 26, 5, 20), rise, set), TimeOfDay::Night);
        assert_eq!(time_of_day(at(4, 26, 6, 20), rise, set), TimeOfDay::Dawn);
        assert_eq!(time_of_day(at(4, 26, 12, 0), rise, set), TimeOfDay::Day);
        assert_eq!(time_of_day(at(4, 26, 19, 45), rise, set), TimeOfDay::Dusk);
        assert_eq!(time_of_day(at(4, 26, 21, 0), rise, set), TimeOfDay::Night);

        // No sun times: not always night.
        assert_eq!(time_of_day(at(4, 26, 12, 0), None, None), TimeOfDay::Day);
        assert_eq!(time_of_day(at(4, 26, 23, 0), None, None), TimeOfDay::Night);
    }

    #[test]
    fn photo_queries() {
        let mut w = weather();
        assert_eq!(photo_query(at(4, 26, 12, 0), Some(&w), true), "spring");
        assert_eq!(photo_query(at(4, 26, 23, 0), Some(&w), true), "spring night");

        w.weathercode = 61; // just started raining, nothing measured yet
        assert_eq!(photo_query(at(4, 26, 12, 0), Some(&w), true), "spring rain");
        assert_eq!(photo_query(at(4, 26, 23, 0), Some(&w), true), "spring rainy night");

        w.weathercode = 0;
        w.cloudcover = 90.0;
        assert_eq!(photo_query(at(4, 26, 12, 0), Some(&w), true), "spring cloudy");

        // No weather yet still gives a sensible search.
        assert_eq!(photo_query(at(7, 1, 12, 0), None, true), "summer");

        assert_eq!(photo_query(at(12, 24, 12, 0), None, true), "christmas");
        assert_eq!(photo_query(at(12, 24, 12, 0), None, false), "winter");
    }

    #[test]
    fn precipitation_tile_follows_the_weathercode() {
        let mut w = weather();
        w.weathercode = 95;
        w.rain_mm = 3.0;
        assert_eq!(precipitation(&w), ("droplets.svg", "Thunder", "3.0 mm".to_string()));

        let mut w = weather();
        w.weathercode = 75;
        w.snowfall_cm = 5.0;
        assert_eq!(precipitation(&w), ("snowflake.svg", "Heavy Snow", "5.0 cm".to_string()));

        let mut w = weather();
        w.weathercode = 45;
        assert_eq!(precipitation(&w).1, "Fog");

        // Unknown code but measurable snow still reports snow.
        let mut w = weather();
        w.weathercode = 3;
        w.snowfall_cm = 2.0;
        assert_eq!(precipitation(&w), ("snowflake.svg", "Snow", "2.0 cm".to_string()));
    }

    #[test]
    fn slightly_below_zero_is_not_negative_zero() {
        let mut w = weather();
        w.temperature_c = -0.4;
        let view = view(at(1, 10, 12, 0), &Units::default(), &Visibility::default(), None, Some(&w), None);
        assert_eq!(view.weather.unwrap().temperature, "0 °C");
    }

    #[test]
    fn missing_sun_times_render_as_dashes() {
        let mut w = weather();
        w.sunrise = None;
        let view = view(at(4, 26, 12, 0), &Units::default(), &Visibility::default(), None, Some(&w), None);
        assert_eq!(view.weather.unwrap().sunrise, "--:--");
    }
}

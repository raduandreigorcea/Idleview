// Pure helpers for photo selection and caching. Kept free of DOM and Tauri calls
// so they can be tested directly.

// The settings that determine *which* photo we show. A change to any of these
// makes a cached photo wrong, not merely stale, so it must be refetched rather
// than served from cache. refresh_interval is deliberately excluded: it changes
// how long a photo stays valid, not which photo is correct.
export function photoSettingsSignature(settings) {
    const photos = settings?.photos || {};
    return JSON.stringify([
        photos.custom_query ?? '',
        photos.enable_festive_queries ?? true,
        photos.photo_quality ?? '',
    ]);
}

// Coarse precipitation state, used to poll weather more often across a transition.
// Mirrors the weathercode bands the Rust side uses in get_precipitation_display.
export function getPrecipState(weather) {
    const code = weather?.weathercode ?? -1;
    if (weather?.snowfall > 0 || (code >= 71 && code <= 77) || code === 85 || code === 86) return 'snow';
    if (weather?.rain > 0 || (code >= 51 && code <= 67) || (code >= 80 && code <= 82) || code >= 95) return 'rain';
    return 'none';
}

export const DEFAULT_REFRESH_INTERVAL_MINUTES = 30;

export function refreshIntervalMs(settings) {
    const minutes = settings?.photos?.refresh_interval || DEFAULT_REFRESH_INTERVAL_MINUTES;
    return minutes * 60 * 1000;
}

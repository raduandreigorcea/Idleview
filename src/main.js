import { photoSettingsSignature, getPrecipState, refreshIntervalMs } from './photo-settings.js';

const invoke = window.__TAURI__.core.invoke;

// Store state
let currentWeather = null;
let creditTimeout = null;
let debugInterval = null;
let prefetchedPhoto = null;
let userSettings = null;
let lastCacheValid = null;
let timeInterval = null;
let timeTimeout = null;
let lastTimeHtml = null;
let lastDateKey = null;
let sunriseSunsetTimeFormat = null;
let sunriseSunsetIs12h = false;
let weatherTimeout = null;
let lastPrecipState = null;
let pairingTimeout = null;

// The font catalogue comes from Rust (see src-tauri/src/fonts.rs). This file used to
// keep its own copies of the font list, the CSS stacks, the weight names and the size
// clamps, and every one of them had drifted from the control panel's copies: fonts were
// offered that were never loaded, weights that the families do not publish, and the
// weekday/date size range here (16-120) disagreed with the panel's and the backend's
// (40-200), so a size the panel accepted was silently capped on screen.
//
// There is one catalogue now, and the backend has already validated and snapped every
// stored value against it - so this just applies what it is given.
let fontCatalogue = null;

function fontStack(id) {
    return fontCatalogue?.fonts.find(font => font.id === id)?.stack || 'sans-serif';
}

// Optical correction: font-size sets the em-box, not the letters, so the same px value
// renders visibly larger in a tall face than a short one. size_scale (from the catalogue,
// measured from the real font files) evens the apparent cap-height out. See fonts.rs.
function fontScale(id) {
    return fontCatalogue?.fonts.find(font => font.id === id)?.size_scale || 1;
}

function scaledSize(id, px) {
    return `${Number(px) * fontScale(id)}px`;
}

function applyTypographySettings() {
    const root = document.documentElement;
    if (!root || !fontCatalogue) return;

    const display = userSettings?.display || {};

    root.style.setProperty('--font-time', fontStack(display.clock_font));
    root.style.setProperty('--clock-size-desktop', scaledSize(display.clock_font, display.clock_font_size));
    root.style.setProperty('--font-weight-time', display.clock_font_weight);

    root.style.setProperty('--font-weekday', fontStack(display.weekday_font));
    root.style.setProperty('--font-size-weekday', scaledSize(display.weekday_font, display.weekday_font_size));
    root.style.setProperty('--font-weight-weekday', display.weekday_font_weight);

    root.style.setProperty('--font-date', fontStack(display.date_font));
    root.style.setProperty('--font-size-date', scaledSize(display.date_font, display.date_font_size));
    root.style.setProperty('--font-weight-date', display.date_font_weight);
}

// Simple element setters
const setText = (id, value) => {
    const el = document.getElementById(id);
    if (el && el.textContent !== value) el.textContent = value;
};

// Unsplash display names and profile URLs are user-controlled strings. Interpolating
// them into innerHTML lets a crafted name inject markup, and a `javascript:` href
// would run on click - so links are built as nodes and every URL is scheme-checked.
function safeHttpUrl(value) {
    try {
        const url = new URL(value);
        return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null;
    } catch {
        return null;
    }
}

function externalLink(href, text) {
    const safeHref = safeHttpUrl(href);
    const el = document.createElement(safeHref ? 'a' : 'span');
    if (safeHref) {
        el.href = safeHref;
        el.target = '_blank';
        el.rel = 'noopener noreferrer';
    }
    el.textContent = text;
    return el;
}



// Update weather display
function updateWeatherDisplay(weather) {
    const tempUnit = weather.temperature_unit === 'fahrenheit' ? '°F' : '°C';
    setText('temp', `${Math.round(weather.temperature)} ${tempUnit}`);
    setText('humidity', `${weather.humidity}%`);
    setText('wind', `${Math.round(weather.wind_speed)} ${weather.wind_speed_label}`);
    setText('cloudiness', `${weather.cloudcover}%`);

    invoke('get_precipitation_display', { weather }).then(precip => {
        setText('precipitation', precip.value);
        
        const iconEl = document.querySelector('[data-metric="precipitation"] .metric-icon');
        const labelEl = document.querySelector('[data-metric="precipitation"] .metric-label');
        if (iconEl) {
            iconEl.src = `assets/${precip.icon}`;
            iconEl.alt = precip.label;
        }
        if (labelEl) labelEl.textContent = precip.label;
    });

    // Update sunrise/sunset
    const sunrise = new Date(weather.sunrise);
    const sunset = new Date(weather.sunset);
    if (!sunriseSunsetTimeFormat) updateTimeFormatCache();
    const timeFormat = sunriseSunsetTimeFormat || { hour: '2-digit', minute: '2-digit', hour12: false };
    
    let sunriseText = sunrise.toLocaleTimeString('en-US', timeFormat);
    let sunsetText = sunset.toLocaleTimeString('en-US', timeFormat);
    
    if (sunriseSunsetIs12h) {
        sunriseText = sunriseText.replace(' AM', 'am').replace(' PM', 'pm');
        sunsetText = sunsetText.replace(' AM', 'am').replace(' PM', 'pm');
    }
    
    setText('sunrise', sunriseText);
    setText('sunset', sunsetText);
    currentWeather = weather;
}

// Retry helper with exponential backoff
async function retryWithBackoff(fn, maxRetries = 5, baseDelayMs = 2000) {
    let lastError;
    for (let attempt = 0; attempt < maxRetries; attempt++) {
        try {
            return await fn();
        } catch (error) {
            lastError = error;
            const delay = baseDelayMs * Math.pow(2, attempt);
            console.warn(`Attempt ${attempt + 1}/${maxRetries} failed, retrying in ${delay / 1000}s...`, error);
            await new Promise(resolve => setTimeout(resolve, delay));
        }
    }
    throw lastError;
}

// Fetch and display location
async function fetchLocation() {
    try {
        const location = await retryWithBackoff(() => invoke('get_location'));
        const locationText = location.city || `${location.latitude.toFixed(2)}°, ${location.longitude.toFixed(2)}°`;
        setText('location', locationText);
        window.userLocation = location;
        await updateWeather(location);
    } catch (error) {
        console.error('Failed to fetch location after retries:', error);
        setText('location', 'Unknown');
        // Schedule another attempt in 30 seconds
        setTimeout(fetchLocation, 30000);
    }
}

// Update weather data (self-scheduling: 15 min normally, 5 min if precip state changed)
// forcePhoto bypasses the photo cache - used when a settings change invalidated it.
async function updateWeather(location, { forcePhoto = false } = {}) {
    if (weatherTimeout) {
        clearTimeout(weatherTimeout);
        weatherTimeout = null;
    }
    try {
        const weather = await retryWithBackoff(() => invoke('get_weather', {
            latitude: location.latitude,
            longitude: location.longitude
        }));

        const newPrecipState = getPrecipState(weather);
        const precipChanged = lastPrecipState !== null && lastPrecipState !== newPrecipState;
        lastPrecipState = newPrecipState;

        updateWeatherDisplay(weather);
        await fetchUnsplashPhoto(forcePhoto);

        // Poll sooner if precipitation just started or stopped, otherwise every 15 min
        const delay = precipChanged ? 5 * 60 * 1000 : 15 * 60 * 1000;
        weatherTimeout = setTimeout(() => updateWeather(location), delay);
    } catch (error) {
        console.error('Failed to fetch weather after retries:', error);
        weatherTimeout = setTimeout(() => updateWeather(location), 30000);
    }
}

// Update time and date
async function updateTimeAndDate() {
    try {
        const timeData = await invoke('get_current_time');
        
        const timeEl = document.getElementById('time');
        if (timeEl) {
            let timeText = timeData.time;
            if (timeText.includes('AM') || timeText.includes('PM')) {
                timeText = timeText.replace(/\s?(AM|PM)/, '<span class="time-period">$1</span>');
            }
            if (timeText !== lastTimeHtml) {
                timeEl.innerHTML = timeText;
                lastTimeHtml = timeText;
            }
        }
        
        const dateEl = document.getElementById('date');
        if (dateEl) {
            const dateKey = `${timeData.day_of_week}|${timeData.date}`;
            if (dateKey !== lastDateKey) {
                dateEl.innerHTML = `<span class="weekday">${timeData.day_of_week}</span><span class="date-value">${timeData.date}</span>`;
                lastDateKey = dateKey;
            }
        }
    } catch (error) {
        console.error('Failed to update time:', error);
    }
}

function updateTimeFormatCache() {
    sunriseSunsetIs12h = userSettings?.units?.time_format === '12h';
    sunriseSunsetTimeFormat = sunriseSunsetIs12h
        ? { hour: 'numeric', minute: '2-digit', hour12: true }
        : { hour: '2-digit', minute: '2-digit', hour12: false };
}

// Load and apply user settings. The backend returns its own defaults when no
// settings file exists, so there is deliberately no fallback copy of the defaults
// here - a second copy would drift from the Rust one.
async function loadSettings() {
    try {
        userSettings = await invoke('get_settings');
    } catch (error) {
        console.error('Failed to load settings:', error);
    }
    updateTimeFormatCache();
    applyDisplaySettings();
}

// Reload settings and refresh UI
async function reloadSettings() {
    const photoSettingsBefore = photoSettingsSignature(userSettings);
    await loadSettings();
    const photoSettingsChanged = photoSettingsBefore !== photoSettingsSignature(userSettings);

    // The cached and prefetched photos were built from the old query, so honouring
    // the cache here would leave a new custom_query with no visible effect until it
    // expired (up to refresh_interval minutes later).
    if (photoSettingsChanged) prefetchedPhoto = null;

    if (window.userLocation) {
        await updateWeather(window.userLocation, { forcePhoto: photoSettingsChanged });
    } else if (photoSettingsChanged) {
        await fetchUnsplashPhoto(true);
    }
    await updateTimeAndDate();
}

function startTimeTicker() {
    if (timeInterval) {
        clearInterval(timeInterval);
        timeInterval = null;
    }
    if (timeTimeout) {
        clearTimeout(timeTimeout);
        timeTimeout = null;
    }

    updateTimeAndDate();
    const now = new Date();
    const msUntilNextMinute = ((60 - now.getSeconds()) * 1000) - now.getMilliseconds();
    timeTimeout = setTimeout(() => {
        updateTimeAndDate();
        timeInterval = setInterval(updateTimeAndDate, 60 * 1000);
    }, Math.max(0, msUntilNextMinute));
}

// Apply display settings
function applyDisplaySettings() {
    if (!userSettings) return;

    applyTypographySettings();
    
    const showClock = userSettings.display.show_clock !== false;
    const showDate = userSettings.display.show_date !== false;
    const showWeekday = userSettings.display.show_weekday !== false;
    const showTemperature = userSettings.display.show_temperature !== false;
    const showSunriseSunset = userSettings.display.show_sunrise_sunset !== false;
    const showPrecipCloud = userSettings.display.show_precipitation_cloudiness !== false;
    const showHumidityWind = userSettings.display.show_humidity_wind !== false;
    const anyMetricsVisible = showSunriseSunset || showPrecipCloud || showHumidityWind;
    const showBottomSection = showTemperature || anyMetricsVisible;

    const clockEl = document.getElementById('time');
    if (clockEl) clockEl.style.display = showClock ? '' : 'none';

    const dateEl = document.getElementById('date');
    if (dateEl) dateEl.style.display = showDate || showWeekday ? '' : 'none';

    const weekdayEl = document.querySelector('#date .weekday');
    if (weekdayEl) weekdayEl.style.display = showWeekday ? '' : 'none';

    const dateValueEl = document.querySelector('#date .date-value');
    if (dateValueEl) dateValueEl.style.display = showDate ? '' : 'none';

    const bottomSection = document.querySelector('.bottom-section');
    if (bottomSection) bottomSection.style.display = showBottomSection ? 'flex' : 'none';

    const mainWeatherStatus = document.querySelector('.main-weather-status');
    if (mainWeatherStatus) mainWeatherStatus.style.display = showTemperature ? 'flex' : 'none';
    
    const metricsMap = {
        'sunrise': showSunriseSunset,
        'sunset': showSunriseSunset,
        'precipitation': showPrecipCloud,
        'cloudiness': showPrecipCloud,
        'humidity': showHumidityWind,
        'wind': showHumidityWind
    };
    
    Object.entries(metricsMap).forEach(([metric, show]) => {
        const el = document.querySelector(`[data-metric="${metric}"]`);
        if (el) el.style.display = show ? 'flex' : 'none';
    });
    
    const metricsGrid = document.querySelector('.metrics-grid');
    
    if (metricsGrid) metricsGrid.style.display = anyMetricsVisible ? 'grid' : 'none';
    if (mainWeatherStatus) mainWeatherStatus.classList.toggle('no-metrics', !anyMetricsVisible);

    const locationBadge = document.querySelector('.location-badge');
    if (locationBadge) locationBadge.style.display = userSettings.display.show_location !== false ? '' : 'none';

}

// Cache helpers
function getCachedPhoto() {
    try {
        const cachedData = localStorage.getItem('unsplash_photo_cache');
        return cachedData ? JSON.parse(cachedData) : null;
    } catch (error) {
        return null;
    }
}

function cachePhoto(photo, query) {
    localStorage.setItem('unsplash_photo_cache', JSON.stringify({
        photo, query, timestamp: Date.now()
    }));
}

// Helper: Build photo query parameters
function buildPhotoQueryParams() {
    if (!currentWeather) return null;
    return {
        cloudcover: currentWeather.cloudcover,
        rain: currentWeather.rain,
        showers: currentWeather.showers,
        snowfall: currentWeather.snowfall,
        weathercode: currentWeather.weathercode,
        sunriseIso: currentWeather.sunrise,
        sunsetIso: currentWeather.sunset,
        enableFestive: userSettings?.photos?.enable_festive_queries ?? true
    };
}

// Helper: Fetch photo from Unsplash
async function fetchPhotoWithQuery(query) {
    return await invoke('get_unsplash_photo', { 
        width: window.innerWidth, 
        height: window.innerHeight,
        query
    });
}

// Display photo
async function displayPhoto(photo) {
    
    // Preload image
    const img = new Image();
    await new Promise((resolve, reject) => {
        img.onload = () => img.decode ? img.decode().then(resolve).catch(resolve) : resolve();
        img.onerror = reject;
        img.src = photo.url;
    }).catch(err => console.error('Failed to load photo:', err));
    
    // Apply background
    document.body.style.backgroundImage = `url('${photo.url}')`;
    document.body.style.backgroundSize = 'cover';
    document.body.style.backgroundRepeat = 'no-repeat';
    document.body.style.backgroundPosition = 'center';
    
    // Photo credit
    let creditElement = document.getElementById('photo-credit');
    if (!creditElement) {
        creditElement = document.createElement('div');
        creditElement.id = 'photo-credit';
        document.body.appendChild(creditElement);
    }
    creditElement.replaceChildren(
        'Photo by ',
        externalLink(photo.author_url, photo.author || 'Unknown'),
        ' on ',
        externalLink('https://unsplash.com', 'Unsplash')
    );
    creditElement.classList.remove('hidden');

    if (creditTimeout) clearTimeout(creditTimeout);
    creditTimeout = setTimeout(() => creditElement.classList.add('hidden'), 10000);

    if (debugInterval) clearInterval(debugInterval);

    // Trigger Unsplash download
    if (photo.download_location) {
        invoke('trigger_unsplash_download', { downloadUrl: photo.download_location }).catch(() => {});
    }

    // Publish to the control panel (fire-and-forget). This goes through a Tauri
    // command rather than an HTTP call to our own server: the state is in-process,
    // and a cross-origin fetch would need the API to grant CORS to a browser origin.
    invoke('set_current_photo', {
        photo: { url: photo.url, author: photo.author, author_url: photo.author_url }
    }).catch(() => {});

    // Debug display
    if (userSettings?.display?.show_debug) {
        const debugEl = document.getElementById('debug');
        if (debugEl) {
            debugEl.style.display = 'grid';
            const renderDebug = async () => {
                try {
                    const cached = getCachedPhoto();
                    const debugInfo = await invoke('get_debug_info', {
                        cacheTimestamp: cached?.timestamp,
                        query: cached?.query,
                        sunriseIso: currentWeather?.sunrise,
                        sunsetIso: currentWeather?.sunset,
                        temperature: currentWeather?.temperature,
                        rain: currentWeather?.rain,
                        snowfall: currentWeather?.snowfall,
                        cloudcover: currentWeather?.cloudcover
                    });

                    let nextRefreshDisplay = 'N/A';
                    if (cached?.timestamp) {
                        const refreshInterval = refreshIntervalMs(userSettings);
                        const cacheAge = Date.now() - cached.timestamp;
                        const timeUntilRefresh = Math.max(0, refreshInterval - cacheAge);
                        nextRefreshDisplay = Math.floor(timeUntilRefresh / 1000) + 's';
                    }

                    // The query is whatever the user typed into the control panel, so
                    // this is built as text nodes rather than interpolated markup.
                    const row = (text) => {
                        const div = document.createElement('div');
                        div.textContent = text;
                        return div;
                    };

                    const weather = document.createElement('div');
                    weather.className = 'debug-weather';
                    weather.append(
                        row(`Temp: ${debugInfo.temperature}`),
                        row(`Rain: ${debugInfo.rain}`),
                        row(`Snow: ${debugInfo.snowfall}`),
                        row(`Clouds: ${debugInfo.cloudcover}`)
                    );

                    debugEl.replaceChildren(
                        row(`Photo cached: ${debugInfo.photo_age}`),
                        row(`Query: ${debugInfo.query}`),
                        row(`Time: ${debugInfo.time_of_day} (${debugInfo.time_source})`),
                        row(`Season: ${debugInfo.season}`),
                        row(`Photos: ${debugInfo.photo_mode}`),
                        row(`Source: ${debugInfo.photo_source}`),
                        row(`Cache valid: ${lastCacheValid === null ? 'N/A' : lastCacheValid ? 'Yes' : 'No'}`),
                        row(`Next refresh: ${nextRefreshDisplay}`),
                        weather
                    );
                } catch (e) {
                    console.error('Failed to render debug:', e);
                }
            };

            await renderDebug();
            debugInterval = setInterval(renderDebug, 1000);
        }
    } else {
        const debugEl = document.getElementById('debug');
        if (debugEl) debugEl.style.display = 'none';
    }
}

// Fetch Unsplash photo
async function fetchUnsplashPhoto(forceRefresh = false) {
    try {
        const cached = getCachedPhoto();
        
        if (!forceRefresh && cached) {
            lastCacheValid = await invoke('is_cache_valid', { cacheTimestamp: cached.timestamp });
            if (lastCacheValid) {
                await displayPhoto(cached.photo);
                return;
            }
        }
        
        if (!currentWeather) {
            return;
        }
        
        if (prefetchedPhoto && !forceRefresh) {
            cachePhoto(prefetchedPhoto.photo, prefetchedPhoto.query);
            await displayPhoto(prefetchedPhoto.photo);
            prefetchedPhoto = null;
            return;
        }
        
        const queryParams = buildPhotoQueryParams();
        if (!queryParams) return;
        
        const queryResult = await invoke('build_photo_query', queryParams);
        
        const photo = await fetchPhotoWithQuery(queryResult.query);
        
        cachePhoto(photo, queryResult.query);
        await displayPhoto(photo);
        
    } catch (error) {
        console.error('Failed to fetch Unsplash photo:', error);
        const cached = getCachedPhoto();
        if (cached) await displayPhoto(cached.photo);
    }
}

// Prefetch next photo
async function prefetchNextPhoto() {
    if (!currentWeather) return;
    
    try {
        const queryParams = buildPhotoQueryParams();
        if (!queryParams) return;
        
        const queryResult = await invoke('build_photo_query', queryParams);
        const photo = await fetchPhotoWithQuery(queryResult.query);
        prefetchedPhoto = { photo, query: queryResult.query };
    } catch (error) {
        console.error('Failed to prefetch photo:', error);
    }
}

// Check if photo needs refresh
async function checkPhotoContext() {
    const cached = getCachedPhoto();
    if (!cached) return;
    
    try {
        const cacheAge = Date.now() - cached.timestamp;
        const refreshInterval = refreshIntervalMs(userSettings);
        const prefetchTime = refreshInterval - (60 * 1000);
        
        if (cacheAge >= prefetchTime && cacheAge < refreshInterval && !prefetchedPhoto) {
            await prefetchNextPhoto();
        }
        
        const isValid = await invoke('is_cache_valid', { cacheTimestamp: cached.timestamp });
        if (!isValid) {
            await fetchUnsplashPhoto(true);
        }
    } catch (error) {
        console.error('Failed to check photo context:', error);
    }
}

// The control panel needs a token to change anything, and this screen is the only
// place it is ever shown. Displayed briefly at startup so a phone can be paired, and
// recallable with T - a kiosk has no other affordance for reading it.
async function showPairingCard(durationMs = 30000) {
    let info;
    try {
        info = await invoke('get_server_info');
    } catch (error) {
        console.error('Failed to read server info:', error);
        return;
    }

    let el = document.getElementById('pairing-card');
    if (!el) {
        el = document.createElement('div');
        el.id = 'pairing-card';
        document.body.appendChild(el);
    }

    const line = (className, text) => {
        const div = document.createElement('div');
        div.className = className;
        div.textContent = text;
        return div;
    };

    // Prefer the LAN address: 127.0.0.1 is useless to the phone you are pairing.
    const lanUrl = info.urls.find(url => !url.includes('127.0.0.1')) || info.urls[0];

    el.replaceChildren(
        line('pairing-title', 'Control panel'),
        line('pairing-url', lanUrl),
        line('pairing-label', 'Token'),
        line('pairing-token', info.token),
        line('pairing-hint', 'Press T to show or hide')
    );
    el.classList.remove('hidden');

    if (pairingTimeout) clearTimeout(pairingTimeout);
    if (durationMs > 0) {
        pairingTimeout = setTimeout(() => el.classList.add('hidden'), durationMs);
    }
}

function togglePairingCard() {
    const el = document.getElementById('pairing-card');
    if (!el || el.classList.contains('hidden')) {
        showPairingCard(0);
        return;
    }
    if (pairingTimeout) clearTimeout(pairingTimeout);
    el.classList.add('hidden');
}

// Initialize
(async function init() {
    // Before settings: applying typography needs the catalogue to resolve a font id to
    // a CSS stack, and the stylesheet URL comes from it too.
    await loadFontCatalogue();
    await loadSettings();

    // Show cached photo immediately
    const cached = getCachedPhoto();
    if (cached) {
        await displayPhoto(cached.photo);
    }

    // Start UI updates
    startTimeTicker();

    // Fetch location and weather (with retry logic built-in)
    window.userLocation = null;
    await fetchLocation();

    // Photo refresh check
    checkPhotoContext();
    setInterval(checkPhotoContext, 5 * 60 * 1000);

    // Listen for settings updates from HTTP API
    await window.__TAURI__.event.listen('settings-updated', async () => {
        await reloadSettings();
    });

    document.addEventListener('contextmenu', e => e.preventDefault());
    document.addEventListener('keydown', event => {
        if (event.key === 't' || event.key === 'T') togglePairingCard();
    });

    showPairingCard();
    setTimeout(applyDisplaySettings, 100);
})();

// Console commands
window.refreshPhoto = async function() {
    console.log('🔄 Manually refreshing photo...');
    if (!currentWeather && window.userLocation) {
        await updateWeather(window.userLocation);
    }
    await fetchUnsplashPhoto(true);
    console.log('✅ Photo refreshed!');
};

window.getSettings = () => invoke('get_settings').then(s => {
    console.group('%c⚙️ Idleview Settings', 'font-weight: bold; color: #4f46e5');

    console.group('📐 Units');
    console.table({
        temperature_unit: s.units.temperature_unit,
        time_format:      s.units.time_format,
        date_format:      s.units.date_format,
        wind_speed_unit:  s.units.wind_speed_unit,
    });
    console.groupEnd();

    console.group('🖥️ Display');
    console.table({
        show_clock:                    s.display.show_clock,
        show_date:                     s.display.show_date,
        show_weekday:                  s.display.show_weekday,
        show_temperature:              s.display.show_temperature,
        show_humidity_wind:            s.display.show_humidity_wind,
        show_precipitation_cloudiness: s.display.show_precipitation_cloudiness,
        show_sunrise_sunset:           s.display.show_sunrise_sunset,
        show_location:                 s.display.show_location,
        show_debug:                    s.display.show_debug,
    });
    console.groupEnd();

    console.group('🔤 Fonts');
    console.table({
        clock_font:         s.display.clock_font,
        clock_font_size:    s.display.clock_font_size,
        clock_font_weight:  s.display.clock_font_weight,
        weekday_font:       s.display.weekday_font,
        weekday_font_size:  s.display.weekday_font_size,
        weekday_font_weight: s.display.weekday_font_weight,
        date_font:          s.display.date_font,
        date_font_size:     s.display.date_font_size,
        date_font_weight:   s.display.date_font_weight,
    });
    console.groupEnd();

    console.group('📷 Photos');
    console.table({
        refresh_interval:      s.photos.refresh_interval,
        photo_quality:         s.photos.photo_quality,
        enable_festive_queries: s.photos.enable_festive_queries,
        custom_query:          s.photos.custom_query,
    });
    console.groupEnd();

    console.groupEnd();

    // The console echoes whatever we return, so hand back a copy without the API key
    // and control token rather than printing them into the log.
    const { secrets, ...withoutSecrets } = s;
    return withoutSecrets;
});
window.saveSettings = s => invoke('save_settings', { settings: s }).then(() => console.log('✅ Saved!'));
window.resetSettings = () => invoke('reset_settings').then(() => console.log('✅ Reset!'));
window.reloadSettings = reloadSettings;
window.showToken = () => showPairingCard(0);

// Listen for photo refresh events
window.__TAURI__.event.listen('refresh-photo', () => window.refreshPhoto());

console.log('%c🎨 Idleview', 'font-size: 14px; font-weight: bold; color: #4f46e5');
console.log('%cCommands: refreshPhoto() | getSettings() | saveSettings(obj) | resetSettings() | reloadSettings()', 'color: #64748b');

// The stylesheet URL is generated from the catalogue, so it requests exactly the fonts
// and weights that are on offer - no more, and never less.
function loadGoogleFonts(stylesheetUrl) {
    ['https://fonts.googleapis.com', 'https://fonts.gstatic.com'].forEach(origin => {
        const link = document.createElement('link');
        link.rel = 'preconnect';
        link.href = origin;
        if (origin.includes('gstatic')) link.crossOrigin = '';
        document.head.appendChild(link);
    });

    const link = document.createElement('link');
    link.rel = 'stylesheet';
    link.href = stylesheetUrl;
    document.head.appendChild(link);
}

async function loadFontCatalogue() {
    try {
        fontCatalogue = await invoke('get_font_catalogue');
        loadGoogleFonts(fontCatalogue.stylesheet);
    } catch (error) {
        console.error('Failed to load the font catalogue:', error);
    }
}

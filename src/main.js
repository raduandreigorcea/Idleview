const invoke = window.__TAURI__.core.invoke;

// Store state
let currentWeather = null;
let currentPhotoUrl = null;
let creditTimeout = null;
let debugInterval = null;
let prefetchedPhoto = null;
let userSettings = null;
let lastCacheValid = null;
let lastPhotoFetchError = null;
let timeInterval = null;
let timeTimeout = null;
let lastTimeHtml = null;
let lastDateHtml = null;
let lastDateKey = null;
let sunriseSunsetTimeFormat = null;
let sunriseSunsetIs12h = false;

const CLOCK_FONT_MAP = {
    roboto: "'Roboto', sans-serif",
    open_sans: "'Open Sans', sans-serif",
    google_sans: "'Google Sans', 'Product Sans', sans-serif",
    inter: "'Inter', sans-serif",
    montserrat: "'Montserrat', sans-serif",
    poppins: "'Poppins', sans-serif",
    lato: "'Lato', sans-serif",
    noto_sans_japanese: "'Noto Sans JP', sans-serif",
    arimo: "'Arimo', sans-serif",
    roboto_condensed: "'Roboto Condensed', sans-serif",
    unbounded: "'Unbounded', sans-serif",
    space_grotesk: "'Space Grotesk', sans-serif",
};

const CLOCK_FONT_WEIGHT_MAP = {
    thin: 200,
    light: 300,
    regular: 400,
    medium: 500,
    semibold: 600,
    bold: 700,
};

const WEEKDAY_FONT_MAP = {
    sacramento:     "'Sacramento', cursive",
    great_vibes:    "'Great Vibes', cursive",
    dancing_script: "'Dancing Script', cursive",
    pacifico:       "'Pacifico', cursive",
    satisfy:        "'Satisfy', cursive",
    pinyon_script:  "'Pinyon Script', cursive",
    alex_brush:     "'Alex Brush', cursive",
    kaushan_script: "'Kaushan Script', cursive",
    italianno:      "'Italianno', cursive",
};

function applyWeekdayTypographySettings() {
    const root = document.documentElement;
    if (!root) return;
    const display = userSettings?.display || {};

    // Weekday font family
    const weekdayKey = (display.weekday_font || 'great_vibes').toLowerCase();
    root.style.setProperty('--font-weekday', WEEKDAY_FONT_MAP[weekdayKey] || WEEKDAY_FONT_MAP.sacramento);

    // Weekday font size
    const wSize = Math.max(16, Math.min(120, Number(display.weekday_font_size) || 70));
    root.style.setProperty('--font-size-weekday', `${wSize}px`);

    // Weekday font weight
    const wWeight = CLOCK_FONT_WEIGHT_MAP[(display.weekday_font_weight || 'thin').toLowerCase()] ?? 200;
    root.style.setProperty('--font-weight-weekday', wWeight);

    // Date font family (checks both sans-serif and cursive/script pools)
    const dateKey = (display.date_font || 'kaushan_script').toLowerCase();
    root.style.setProperty('--font-date', CLOCK_FONT_MAP[dateKey] || WEEKDAY_FONT_MAP[dateKey] || CLOCK_FONT_MAP.space_grotesk);

    // Date font size
    const dSize = Math.max(16, Math.min(120, Number(display.date_font_size) || 40));
    root.style.setProperty('--font-size-date', `${dSize}px`);

    // Date font weight
    const dWeight = CLOCK_FONT_WEIGHT_MAP[(display.date_font_weight || 'medium').toLowerCase()] ?? 500;
    root.style.setProperty('--font-weight-date', dWeight);
}

function applyClockTypographySettings() {
    const root = document.documentElement;
    if (!root) return;

    const display = userSettings?.display || {};

    // Font family
    const configuredFont = (display.clock_font || 'roboto').toLowerCase();
    const resolvedFont = CLOCK_FONT_MAP[configuredFont] || CLOCK_FONT_MAP.roboto;
    root.style.setProperty('--font-time', resolvedFont);

    // Font size
    const configuredSize = Number(display.clock_font_size);
    const desktopSize = Number.isFinite(configuredSize)
        ? Math.max(120, Math.min(260, Math.round(configuredSize)))
        : 180;
    const mobileSize = Math.round(desktopSize * 0.67);
root.style.setProperty('--clock-size-desktop', `${desktopSize}px`);
        root.style.setProperty('--clock-size-mobile', `${mobileSize}px`);

    // Font weight
    const weightKey = (display.clock_font_weight || 'regular').toLowerCase();
    const resolvedWeight = CLOCK_FONT_WEIGHT_MAP[weightKey] ?? 400;
    root.style.setProperty('--font-weight-time', resolvedWeight);
}

// Simple element setters
const setText = (id, value) => {
    const el = document.getElementById(id);
    if (el && el.textContent !== value) el.textContent = value;
};

const setHTML = (id, value) => {
    const el = document.getElementById(id);
    if (el && el.innerHTML !== value) el.innerHTML = value;
};

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

// Update weather data
async function updateWeather(location) {
    try {
        const weather = await retryWithBackoff(() => invoke('get_weather', {
            latitude: location.latitude,
            longitude: location.longitude
        }));
        updateWeatherDisplay(weather);
        await fetchUnsplashPhoto();
    } catch (error) {
        console.error('Failed to fetch weather after retries:', error);
        // Schedule another attempt in 30 seconds
        setTimeout(() => updateWeather(location), 30000);
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
            } else {
                timeText = timeText;
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
                const dateHtml = `<span class="weekday">${timeData.day_of_week}</span><span class="date-value">${timeData.date}</span>`;
                if (dateHtml !== lastDateHtml) {
                    dateEl.innerHTML = dateHtml;
                    lastDateHtml = dateHtml;
                }
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

// Load and apply user settings
async function loadSettings() {
    try {
        userSettings = await invoke('get_settings');
        updateTimeFormatCache();
        applyDisplaySettings();
    } catch (error) {
        console.error('Failed to load settings:', error);
        userSettings = {
            units: { temperature_unit: 'celsius', time_format: '24h', date_format: 'mdy', wind_speed_unit: 'kmh' },
            display: {
                show_humidity_wind: true,
                show_precipitation_cloudiness: true,
                show_sunrise_sunset: true,
                show_location: true,
                  show_debug: false,
                  clock_font: 'roboto',
                clock_font_size: 180,
                clock_font_weight: 'regular',
                  weekday_font: 'great_vibes',
                  weekday_font_size: 70,
                  weekday_font_weight: 'thin',
                  date_font: 'kaushan_script',
                  date_font_size: 40,
                  date_font_weight: 'medium',
            },
            photos: { refresh_interval: 30, photo_quality: '80', enable_festive_queries: true }
        };
        updateTimeFormatCache();
    }
}

// Reload settings and refresh UI
async function reloadSettings() {
    await loadSettings();
    if (window.userLocation) {
        await updateWeather(window.userLocation);
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

    applyClockTypographySettings();
    applyWeekdayTypographySettings();
    
    const showSunriseSunset = userSettings.display.show_sunrise_sunset !== false;
    const showPrecipCloud = userSettings.display.show_precipitation_cloudiness !== false;
    const showHumidityWind = userSettings.display.show_humidity_wind !== false;
    const anyMetricsVisible = showSunriseSunset || showPrecipCloud || showHumidityWind;
    
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
    const mainWeatherStatus = document.querySelector('.main-weather-status');
    
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

// Helper: Get refresh interval in milliseconds
function getRefreshIntervalMs() {
    return (userSettings?.photos?.refresh_interval || 30) * 60 * 1000;
}

// Helper: Build photo query parameters
function buildPhotoQueryParams() {
    if (!currentWeather) return null;
    return {
        cloudcover: currentWeather.cloudcover,
        rain: currentWeather.rain,
        snowfall: currentWeather.snowfall,
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
async function displayPhoto(photo, timestamp = null, query = null) {
    currentPhotoUrl = photo.url;
    
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
    creditElement.innerHTML = `Photo by <a href="${photo.author_url}" target="_blank">${photo.author}</a> on <a href="https://unsplash.com" target="_blank">Unsplash</a>`;
    creditElement.classList.remove('hidden');
    
    if (creditTimeout) clearTimeout(creditTimeout);
    creditTimeout = setTimeout(() => creditElement.classList.add('hidden'), 10000);

    if (debugInterval) clearInterval(debugInterval);

    // Trigger Unsplash download
    if (photo.download_location) {
        invoke('trigger_unsplash_download', { downloadUrl: photo.download_location }).catch(() => {});
    }

    // Update HTTP API (fire-and-forget)
    fetch('http://localhost:8737/api/photo/current', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: photo.url, author: photo.author, author_url: photo.author_url })
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
                        const refreshInterval = getRefreshIntervalMs();
                        const cacheAge = Date.now() - cached.timestamp;
                        const timeUntilRefresh = Math.max(0, refreshInterval - cacheAge);
                        nextRefreshDisplay = Math.floor(timeUntilRefresh / 1000) + 's';
                    }

                    debugEl.innerHTML = `
                        <div>Photo cached: ${debugInfo.photo_age}</div>
                        <div>Query: ${debugInfo.query}</div>
                        <div>Time: ${debugInfo.time_of_day} (${debugInfo.time_source})</div>
                        <div>Season: ${debugInfo.season}</div>
                        <div>API Key: ${debugInfo.api_key_status} (${debugInfo.api_key_source})</div>
                        <div>Cache valid: ${lastCacheValid === null ? 'N/A' : lastCacheValid ? 'Yes' : 'No'}</div>
                        <div>Next refresh: ${nextRefreshDisplay}</div>
                        <div style="margin-top:8px; border-top:1px dashed currentColor; padding-top:8px;">
                            <div>Temp: ${debugInfo.temperature}</div>
                            <div>Rain: ${debugInfo.rain}</div>
                            <div>Snow: ${debugInfo.snowfall}</div>
                            <div>Clouds: ${debugInfo.cloudcover}</div>
                        </div>
                    `;
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
                await displayPhoto(cached.photo, cached.timestamp, cached.query);
                return;
            }
        }
        
        if (!currentWeather) {
            return;
        }
        
        if (prefetchedPhoto && !forceRefresh) {
            cachePhoto(prefetchedPhoto.photo, prefetchedPhoto.query);
            await displayPhoto(prefetchedPhoto.photo, Date.now(), prefetchedPhoto.query);
            prefetchedPhoto = null;
            lastPhotoFetchError = null;
            return;
        }
        
        const queryParams = buildPhotoQueryParams();
        if (!queryParams) return;
        
        const queryResult = await invoke('build_photo_query', queryParams);
        
        const photo = await fetchPhotoWithQuery(queryResult.query);
        
        cachePhoto(photo, queryResult.query);
        await displayPhoto(photo, Date.now(), queryResult.query);
        lastPhotoFetchError = null;
        
    } catch (error) {
        lastPhotoFetchError = error?.message || error?.toString() || 'Unknown error';
        console.error('Failed to fetch Unsplash photo:', error);
        const cached = getCachedPhoto();
        if (cached) await displayPhoto(cached.photo, cached.timestamp, cached.query);
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
        const refreshInterval = getRefreshIntervalMs();
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

// Initialize
(async function init() {
    await loadSettings();
    
    // Show cached photo immediately
    const cached = getCachedPhoto();
    if (cached) {
        await displayPhoto(cached.photo, cached.timestamp, cached.query);
    }
    
    // Start UI updates
    startTimeTicker();

    // Fetch location and weather (with retry logic built-in)
    window.userLocation = null;
    await fetchLocation();
    
    // Periodic weather refresh
    setInterval(() => {
        if (window.userLocation) {
            updateWeather(window.userLocation);
        }
    }, 15 * 60 * 1000);

    // Photo refresh check
    checkPhotoContext();
    setInterval(checkPhotoContext, 5 * 60 * 1000);
    
    // Listen for settings updates from HTTP API
    await window.__TAURI__.event.listen('settings-updated', async () => {
        await reloadSettings();
    });

    document.addEventListener('contextmenu', e => e.preventDefault());
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
    });
    console.groupEnd();

    console.groupEnd();
    return s;
});
window.saveSettings = s => invoke('save_settings', { settings: s }).then(() => console.log('✅ Saved!'));
window.resetSettings = () => invoke('reset_settings').then(s => { console.log('✅ Reset!', s); return s; });
window.reloadSettings = reloadSettings;

// Listen for photo refresh events
window.__TAURI__.event.listen('refresh-photo', () => window.refreshPhoto());

console.log('%c🎨 Idleview', 'font-size: 14px; font-weight: bold; color: #4f46e5');
console.log('%cCommands: refreshPhoto() | getSettings() | saveSettings(obj) | resetSettings() | reloadSettings()', 'color: #64748b');

function loadGoogleFonts() {
    ['https://fonts.googleapis.com', 'https://fonts.gstatic.com'].forEach(origin => {
        const link = document.createElement('link');
        link.rel = 'preconnect';
        link.href = origin;
        if (origin.includes('gstatic')) link.crossOrigin = '';
        document.head.appendChild(link);
    });
    const link = document.createElement('link');
    link.rel = 'stylesheet';
    // Cormorant Garamond loads separately with display=block so it never shows a fallback swap
    const cgLink = document.createElement('link');
    cgLink.rel = 'stylesheet';
    cgLink.href = 'https://fonts.googleapis.com/css2?family=Sacramento&family=Great+Vibes&family=Dancing+Script:wght@400;700&family=Pacifico&family=Satisfy&family=Pinyon+Script&family=Alex+Brush&family=Kaushan+Script&family=Italianno&display=block';
    document.head.appendChild(cgLink);

    link.href = 'https://fonts.googleapis.com/css2?family=Roboto:wght@100;300;400;500;700&family=Open+Sans:wght@300;400;500;600;700&family=Inter:wght@100;300;400;500;700&family=Montserrat:wght@100;200;300;400;500;700&family=Poppins:wght@100;200;300;400;500;700&family=Lato:wght@100;300;400;700&family=Noto+Sans+JP:wght@100;300;400;500;700&family=Arimo:wght@400;500;600;700&family=Roboto+Condensed:wght@100;300;400;500;700&family=Unbounded:wght@200;300;400;500;700&display=swap';
    document.head.appendChild(link);
}

loadGoogleFonts();

const invoke = window.__TAURI__.core.invoke;

// Store state
let currentWeather = null;
let currentPhotoUrl = null;
let debugInterval = null;
let creditTimeout = null; // Timer for photo credit auto-hide
let prefetchedPhoto = null; // Store prefetched photo
let userSettings = null; // Store user settings
let lastCacheValid = null;
let lastPhotoFetchError = null;
let lastNextRefreshMs = null;

// Helper to set text content on multiple elements with different suffixes
function setTextForAllThemes(baseId, value) {
    const suffixes = ['-minimal', '-geometric', '-sidebar'];
    suffixes.forEach(suffix => {
        const el = document.getElementById(baseId + suffix);
        if (el) el.textContent = value;
    });
}

// Helper to set innerHTML on multiple elements with different suffixes
function setHTMLForAllThemes(baseId, value) {
    const suffixes = ['-minimal', '-geometric', '-sidebar'];
    suffixes.forEach(suffix => {
        const el = document.getElementById(baseId + suffix);
        if (el) el.innerHTML = value;
    });
}

// Update weather display
function updateWeatherDisplay(weather) {
    const tempUnit = weather.temperature_unit === 'fahrenheit' ? '°F' : '°C';
    setTextForAllThemes('temp', `${Math.round(weather.temperature)} ${tempUnit}`);
    
    setTextForAllThemes('humidity', `${weather.humidity}%`);
    setTextForAllThemes('wind', `${Math.round(weather.wind_speed)} ${weather.wind_speed_label}`);
    setTextForAllThemes('cloudiness', `${weather.cloudcover}%`);

    // Update precipitation using Rust logic
    invoke('get_precipitation_display', { weather }).then(precip => {
        setTextForAllThemes('precipitation', precip.value);
    });

    // Update sunrise/sunset (use 12h or 24h based on settings)
    const sunrise = new Date(weather.sunrise);
    const sunset = new Date(weather.sunset);
    const timeFormat = userSettings?.units?.time_format === '12h' 
        ? { hour: 'numeric', minute: '2-digit', hour12: true }
        : { hour: '2-digit', minute: '2-digit', hour12: false };
    
    let sunriseText = sunrise.toLocaleTimeString('en-US', timeFormat);
    let sunsetText = sunset.toLocaleTimeString('en-US', timeFormat);
    
    // Make 12h format more compact: remove space before AM/PM and use lowercase
    if (userSettings?.units?.time_format === '12h') {
        sunriseText = sunriseText.replace(' AM', 'am').replace(' PM', 'pm');
        sunsetText = sunsetText.replace(' AM', 'am').replace(' PM', 'pm');
    }
    
    setTextForAllThemes('sunrise', sunriseText);
    setTextForAllThemes('sunset', sunsetText);

    currentWeather = weather;
}

// Fetch and display location
async function fetchLocation() {
    try {
        const location = await invoke('get_location');
        const locationText = location.city || `${location.latitude.toFixed(2)}°, ${location.longitude.toFixed(2)}°`;
        setTextForAllThemes('location', locationText);
        await updateWeather(location);
    } catch (error) {
        console.error('Failed to fetch location:', error);
        setTextForAllThemes('location', 'Unknown');
    }
}

// Update weather data
async function updateWeather(location) {
    try {
        const weather = await invoke('get_weather', {
            latitude: location.latitude,
            longitude: location.longitude
        });
        
        updateWeatherDisplay(weather);
        await fetchUnsplashPhoto();
    } catch (error) {
        console.error('Failed to fetch weather:', error);
    }
}

// Update CPU temperature
async function updateCPUTemp() {
    // CPU card doesn't exist in new layout - skip silently
    const cpuCard = document.querySelector('.cpu-card');
    if (!cpuCard) return;
    
    // Check if CPU temp should be shown in settings
    if (userSettings && !userSettings.display.show_cpu_temp) {
        cpuCard.style.display = 'none';
        return;
    }
    
    try {
        const temp = await invoke('get_cpu_temp');
        const cpuTempEl = document.getElementById('cpu-temp');
        
        if (temp.value > 0 && cpuTempEl) {
            cpuTempEl.textContent = temp.display;
            cpuCard.style.display = 'flex';
        } else {
            cpuCard.style.display = 'none';
        }
    } catch (error) {
        console.error('Failed to fetch CPU temp:', error);
        cpuCard.style.display = 'none';
    }
}

// Update time and date
async function updateTimeAndDate() {
    try {
        const timeData = await invoke('get_current_time');
        
        // Update minimal theme time (with styled AM/PM)
        const timeMinimal = document.getElementById('time-minimal');
        if (timeMinimal) {
            let timeText = timeData.time;
            if (timeText.includes('AM') || timeText.includes('PM')) {
                timeText = timeText.replace(/(AM|PM)/, '<span class="time-period">$1</span>');
                timeMinimal.innerHTML = timeText;
            } else {
                timeMinimal.textContent = timeText;
            }
        }
        
        // Update geometric theme time (separate period element)
        const timeGeometric = document.getElementById('time-geometric');
        const periodGeometric = document.getElementById('period-geometric');
        if (timeGeometric) {
            let timeText = timeData.time;
            let period = '';
            if (timeText.includes('AM')) {
                period = 'AM';
                timeText = timeText.replace(' AM', '').replace('AM', '');
            } else if (timeText.includes('PM')) {
                period = 'PM';
                timeText = timeText.replace(' PM', '').replace('PM', '');
            }
            timeGeometric.textContent = timeText;
            if (periodGeometric) periodGeometric.textContent = period;
        }
        
        // Update date for minimal theme
        const dateMinimal = document.getElementById('date-minimal');
        if (dateMinimal) {
            dateMinimal.innerHTML = `${timeData.day_of_week}<br>${timeData.date}`;
        }
        
        // Update date for geometric theme (separate day and date)
        const dayGeometric = document.getElementById('day-geometric');
        const dateGeometric = document.getElementById('date-geometric');
        if (dayGeometric) {
            dayGeometric.textContent = timeData.day_of_week;
        }
        if (dateGeometric) {
            dateGeometric.textContent = timeData.date;
        }
        
        // Update sidebar theme time
        const timeSidebar = document.getElementById('time-sidebar');
        if (timeSidebar) {
            let timeText = timeData.time;
            if (timeText.includes('AM') || timeText.includes('PM')) {
                timeText = timeText.replace(/(AM|PM)/, '<span class="sidebar-time-period">$1</span>');
                timeSidebar.innerHTML = timeText;
            } else {
                timeSidebar.textContent = timeText;
            }
        }
        
        // Update sidebar theme day and date (separate elements)
        const daySidebar = document.getElementById('day-sidebar');
        const dateSidebar = document.getElementById('date-sidebar');
        if (daySidebar) {
            daySidebar.textContent = timeData.day_of_week;
        }
        if (dateSidebar) {
            dateSidebar.textContent = timeData.date;
        }
    } catch (error) {
        console.error('Failed to update time:', error);
    }
}

// Load and apply user settings
async function loadSettings() {
    try {
        userSettings = await invoke('get_settings');
        console.log('Settings loaded:', userSettings);
        applyDisplaySettings();
    } catch (error) {
        console.error('Failed to load settings:', error);
        // Use defaults if settings fail to load
        userSettings = {
            units: {
                temperature_unit: 'celsius',
                time_format: '24h',
                date_format: 'mdy',
                wind_speed_unit: 'kmh'
            },
            display: {
                show_humidity_wind: true,
                show_precipitation_cloudiness: true,
                show_sunrise_sunset: true,
                show_cpu_temp: true,
                theme: 'default'
            },
            photos: {
                refresh_interval: 30,
                photo_quality: 'high'
            }
        };
    }
}

// Reload settings and refresh UI
async function reloadSettings() {
    console.log('🔄 Reloading settings...');
    await loadSettings();
    
    // Refresh all UI elements with new settings
    if (window.userLocation) {
        await updateWeather(window.userLocation);
    }
    await updateTimeAndDate();
    await updateCPUTemp();
    
    console.log('✅ Settings reloaded and UI updated!');
}

// Settings changes are now handled via 'settings-updated' events from the HTTP server
// This eliminates the need for polling and improves performance

// Apply display settings to show/hide cards
function applyDisplaySettings() {
    if (!userSettings) return;
    
    // Apply theme
    applyTheme(userSettings.display.theme || 'minimal');
    
    // Track visibility settings
    const showSunriseSunset = userSettings.display.show_sunrise_sunset !== false;
    const showPrecipCloud = userSettings.display.show_precipitation_cloudiness !== false;
    const showHumidityWind = userSettings.display.show_humidity_wind !== false;
    const anyMetricsVisible = showSunriseSunset || showPrecipCloud || showHumidityWind;
    
    // Apply to MINIMAL theme
    const minimalLayout = document.querySelector('.theme-minimal');
    if (minimalLayout) {
        const sunriseMetric = minimalLayout.querySelector('[data-metric="sunrise"]');
        const sunsetMetric = minimalLayout.querySelector('[data-metric="sunset"]');
        const precipMetric = minimalLayout.querySelector('[data-metric="precipitation"]');
        const cloudinessMetric = minimalLayout.querySelector('[data-metric="cloudiness"]');
        const humidityMetric = minimalLayout.querySelector('[data-metric="humidity"]');
        const windMetric = minimalLayout.querySelector('[data-metric="wind"]');
        
        if (sunriseMetric && sunsetMetric) {
            sunriseMetric.style.display = showSunriseSunset ? 'flex' : 'none';
            sunsetMetric.style.display = showSunriseSunset ? 'flex' : 'none';
        }
        if (precipMetric && cloudinessMetric) {
            precipMetric.style.display = showPrecipCloud ? 'flex' : 'none';
            cloudinessMetric.style.display = showPrecipCloud ? 'flex' : 'none';
        }
        if (humidityMetric && windMetric) {
            humidityMetric.style.display = showHumidityWind ? 'flex' : 'none';
            windMetric.style.display = showHumidityWind ? 'flex' : 'none';
        }
        
        const metricsGrid = minimalLayout.querySelector('.metrics-grid');
        const mainWeatherStatus = minimalLayout.querySelector('.main-weather-status');
        
        if (metricsGrid) {
            metricsGrid.style.display = anyMetricsVisible ? 'grid' : 'none';
        }
        if (mainWeatherStatus) {
            mainWeatherStatus.classList.toggle('no-metrics', !anyMetricsVisible);
        }
    }
    
    // Apply to GEOMETRIC theme
    const geometricLayout = document.querySelector('.theme-geometric');
    if (geometricLayout) {
        const sunriseChip = geometricLayout.querySelector('[data-metric="sunrise"]');
        const sunsetChip = geometricLayout.querySelector('[data-metric="sunset"]');
        const precipChip = geometricLayout.querySelector('[data-metric="precipitation"]');
        const cloudinessChip = geometricLayout.querySelector('[data-metric="cloudiness"]');
        const humidityChip = geometricLayout.querySelector('[data-metric="humidity"]');
        const windChip = geometricLayout.querySelector('[data-metric="wind"]');
        
        // Apply visibility based on settings
        if (sunriseChip) sunriseChip.style.display = showSunriseSunset ? 'flex' : 'none';
        if (sunsetChip) sunsetChip.style.display = showSunriseSunset ? 'flex' : 'none';
        if (precipChip) precipChip.style.display = showPrecipCloud ? 'flex' : 'none';
        if (cloudinessChip) cloudinessChip.style.display = showPrecipCloud ? 'flex' : 'none';
        if (humidityChip) humidityChip.style.display = showHumidityWind ? 'flex' : 'none';
        if (windChip) windChip.style.display = showHumidityWind ? 'flex' : 'none';
        
        // Hide the entire metrics grid if no metrics are visible
        const geoMetricsGrid = geometricLayout.querySelector('.geo-metrics-grid');
        if (geoMetricsGrid) {
            geoMetricsGrid.style.display = anyMetricsVisible ? 'grid' : 'none';
        }
    }
    
    // Apply to SIDEBAR theme
    const sidebarLayout = document.querySelector('.theme-sidebar');
    if (sidebarLayout) {
        const sunriseRow = sidebarLayout.querySelector('[data-metric="sunrise"]');
        const sunsetRow = sidebarLayout.querySelector('[data-metric="sunset"]');
        const precipRow = sidebarLayout.querySelector('[data-metric="precipitation"]');
        const cloudinessRow = sidebarLayout.querySelector('[data-metric="cloudiness"]');
        const humidityRow = sidebarLayout.querySelector('[data-metric="humidity"]');
        const windRow = sidebarLayout.querySelector('[data-metric="wind"]');
        
        // Apply visibility based on settings
        if (sunriseRow) sunriseRow.style.display = showSunriseSunset ? 'flex' : 'none';
        if (sunsetRow) sunsetRow.style.display = showSunriseSunset ? 'flex' : 'none';
        if (precipRow) precipRow.style.display = showPrecipCloud ? 'flex' : 'none';
        if (cloudinessRow) cloudinessRow.style.display = showPrecipCloud ? 'flex' : 'none';
        if (humidityRow) humidityRow.style.display = showHumidityWind ? 'flex' : 'none';
        if (windRow) windRow.style.display = showHumidityWind ? 'flex' : 'none';
        
        // Hide details section if no metrics visible
        const sidebarDetails = sidebarLayout.querySelector('.sidebar-details');
        if (sidebarDetails) {
            sidebarDetails.style.display = anyMetricsVisible ? 'block' : 'none';
        }
    }
    
    // Set debug panel position to opposite side of cards
    const debugEl = document.getElementById('debug');
    if (debugEl) {
        const cardPosition = userSettings.display.card_position || 'left';
        const debugPosition = cardPosition === 'left' ? 'right' : 'left';
        debugEl.setAttribute('data-position', debugPosition);
    }
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
        photo: photo,
        query: query,
        timestamp: Date.now()
    }));
}

// Apply theme to body element
function applyTheme(themeName) {
    // Map 'default' and 'light' to 'minimal' for backwards compatibility
    if (themeName === 'default' || themeName === 'light') {
        themeName = 'minimal';
    }
    const currentTheme = document.body.getAttribute('data-theme');
    if (currentTheme !== themeName) {
        document.body.setAttribute('data-theme', themeName);
        console.log(`🎨 Theme applied: ${themeName}`);
    }
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

// Helper: Fetch photo from Unsplash with a given query
async function fetchPhotoWithQuery(query) {
    const photo = await invoke('get_unsplash_photo', { 
        width: window.innerWidth, 
        height: window.innerHeight,
        query: query
    });
    return photo;
}

// Display photo
async function displayPhoto(photo, timestamp = null, query = null) {
    currentPhotoUrl = photo.url;
    
    // Preload the image to ensure it's fully loaded before displaying
    const img = new Image();
    
    // Wait for image to fully load and decode
    await new Promise((resolve, reject) => {
        img.onload = () => {
            // Use decode() for better performance if available
            if (img.decode) {
                img.decode()
                    .then(resolve)
                    .catch(() => resolve()); // Fallback if decode fails
            } else {
                resolve();
            }
        };
        img.onerror = reject;
        img.src = photo.url;
    }).catch(err => {
        console.error('Failed to load photo:', err);
    });
    
    // Now apply the background - image is fully loaded and decoded
    document.body.style.backgroundImage = `url('${photo.url}')`;
    document.body.style.backgroundSize = 'cover';
    document.body.style.backgroundRepeat = 'no-repeat';
    document.body.style.backgroundPosition = 'center';
    
    // Display photo credit
    let creditElement = document.getElementById('photo-credit');
    if (!creditElement) {
        creditElement = document.createElement('div');
        creditElement.id = 'photo-credit';
        document.body.appendChild(creditElement);
    }
    creditElement.innerHTML = `Photo by <a href="${photo.author_url}" target="_blank">${photo.author}</a> on <a href="https://unsplash.com" target="_blank">Unsplash</a>`;
    
    // Show credit and set timer to hide after 10 seconds
    creditElement.classList.remove('hidden');
    if (creditTimeout) clearTimeout(creditTimeout);
    creditTimeout = setTimeout(() => {
        creditElement.classList.add('hidden');
    }, 10000);

    // Clear previous intervals
    if (debugInterval) clearInterval(debugInterval);

    // Trigger Unsplash download endpoint
    if (photo.download_location) {
        try {
            await invoke('trigger_unsplash_download', { downloadUrl: photo.download_location });
        } catch (error) {
            console.error('Failed to trigger download:', error);
        }
    }

    // Send current photo to HTTP API for companion app
    try {
        await fetch('http://localhost:8737/api/photo/current', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                url: photo.url,
                author: photo.author,
                author_url: photo.author_url
            })
        });
    } catch (error) {
        // Silently fail - companion app might not be running
        console.debug('Could not update companion app photo:', error);
    }

    // Debug display
    const showDebug = userSettings?.display?.show_debug || false;
    if (showDebug) {
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
                        sunsetIso: currentWeather?.sunset
                    });
                    
                    // Calculate time until next refresh dynamically
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
                        <div>API Key: ${debugInfo.api_key_status} (${debugInfo.api_key_source})</div>
                        <div>Cache timestamp: ${cached?.timestamp ?? 'N/A'}</div>
                        <div>Last photo fetch: ${window.lastPhotoFetch ? new Date(window.lastPhotoFetch).toLocaleString() : 'N/A'}</div>
                        <div>Cache valid: ${lastCacheValid === null ? 'N/A' : lastCacheValid ? 'Yes' : 'No'}</div>
                        <div>Last fetch error: ${lastPhotoFetchError ?? 'None'}</div>
                        <div>Next refresh in: ${nextRefreshDisplay}</div>
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
        
        // Check cache validity using Rust
        if (!forceRefresh && cached) {
            lastCacheValid = await invoke('is_cache_valid', { 
                cacheTimestamp: cached.timestamp 
            });
            if (lastCacheValid) {
                await displayPhoto(cached.photo, cached.timestamp, cached.query);
                return;
            }
        }
        
        if (!currentWeather) {
            console.log('Waiting for weather data before fetching photo...');
            return;
        }
        
        // If we have a prefetched photo ready, use it immediately
        if (prefetchedPhoto && !forceRefresh) {
            console.log('Using prefetched photo');
            const nowTs = Date.now();
            cachePhoto(prefetchedPhoto.photo, prefetchedPhoto.query);
            await displayPhoto(prefetchedPhoto.photo, nowTs, prefetchedPhoto.query);
            prefetchedPhoto = null; // Clear prefetch
            return;
        }
        
        // Build query using Rust backend with weather data
        const queryParams = buildPhotoQueryParams();
        if (!queryParams) {
            console.log('Cannot build query without weather data');
            return;
        }
        
        const queryResult = await invoke('build_photo_query', queryParams);
        
        console.log(`📸 Fetching Photo | Query: "${queryResult.query}" | ${window.innerWidth}x${window.innerHeight} @ ${userSettings?.photos?.photo_quality || '85'}%`);
        
        const photo = await fetchPhotoWithQuery(queryResult.query);
        
        // Extract quality from URL
        const qualityMatch = photo.url.match(/[?&]q=(\d+)/);
        const actualQuality = qualityMatch ? qualityMatch[1] : 'unknown';
        
        console.log(`✅ Photo Ready | ${photo.author} | Quality: ${actualQuality}%`);
        
        const nowTs = Date.now();
        cachePhoto(photo, queryResult.query);
        await displayPhoto(photo, nowTs, queryResult.query);
        window.lastPhotoFetch = nowTs;
        
    } catch (error) {
        lastPhotoFetchError = error?.message || error?.toString() || 'Unknown error';
        console.error('Failed to fetch Unsplash photo:', error);
        // Fallback to cache
        const cached = getCachedPhoto();
        if (cached) {
            await displayPhoto(cached.photo, cached.timestamp, cached.query);
        }
    }
}

// Prefetch next photo in background (doesn't display it)
async function prefetchNextPhoto() {
    if (!currentWeather) return;
    
    try {
        console.log('Prefetching next photo in background...');
        
        const queryParams = buildPhotoQueryParams();
        if (!queryParams) return;
        
        const queryResult = await invoke('build_photo_query', queryParams);
        const photo = await fetchPhotoWithQuery(queryResult.query);
        
        // Store for later use
        prefetchedPhoto = {
            photo: photo,
            query: queryResult.query
        };
        
        console.log('Photo prefetched successfully (ready to display)');
    } catch (error) {
        console.error('Failed to prefetch photo:', error);
    }
}

// Check if photo context has changed
async function checkPhotoContext() {
    const cached = getCachedPhoto();
    if (!cached) {
        console.log('📸 No cached photo found');
        return;
    }
    
    try {
        const now = Date.now();
        const cacheAge = now - cached.timestamp;
        console.log(`📊 Cache age: ${Math.floor(cacheAge / 60000)}min`);
        
        // Get photo refresh interval from settings (convert minutes to milliseconds)
        const refreshInterval = getRefreshIntervalMs();
        const prefetchTime = refreshInterval - (1 * 60 * 1000); // 1 minute before expiry
        // Prefetch before expiry
        if (cacheAge >= prefetchTime && cacheAge < refreshInterval && !prefetchedPhoto) {
            await prefetchNextPhoto();
        }
        // Switch to new photo when cache expires
        lastNextRefreshMs = cached ? Math.max(0, refreshInterval - cacheAge) : null;
        const isValid = await invoke('is_cache_valid', { 
            cacheTimestamp: cached.timestamp 
        });
        
        if (!isValid) {
            console.log(`⏰ Cache expired (${userSettings?.photos?.refresh_interval || 30}min) | Refreshing...`);
            await fetchUnsplashPhoto(true); // Will use prefetched if available
        }
    } catch (error) {
        console.error('Failed to check photo context:', error);
    }
}

// Initialize
(async function init() {
    await loadSettings();
    
    // Show cached photo immediately if available (before fetching weather/location)
    const cached = getCachedPhoto();
    if (cached) {
        console.log('📷 Displaying cached photo immediately...');
        await displayPhoto(cached.photo, cached.timestamp, cached.query);
    }
    
    // Start UI updates immediately (independent of photo/weather)
    updateTimeAndDate();
    setInterval(updateTimeAndDate, 1000);

    updateCPUTemp();
    setInterval(updateCPUTemp, 10 * 1000);

    // Set up intervals - store location for weather updates
    window.userLocation = null;
    fetchLocation().then(() => {
        invoke('get_location').then(location => {
            window.userLocation = location;
            setInterval(() => updateWeather(window.userLocation), 15 * 60 * 1000);
        });
    });

    // Check photo context immediately on startup, then periodically
    console.log('🔄 Starting photo context check interval (5min)...');
    checkPhotoContext();
    const photoContextInterval = setInterval(() => {
        console.log('⏰ Photo context check triggered');
        checkPhotoContext();
    }, 5 * 60 * 1000);
    console.log('✅ Photo context interval started:', photoContextInterval);
    
    // Listen for settings updates from HTTP API instead of polling
    await window.__TAURI__.event.listen('settings-updated', async (event) => {
        console.log('⚡ Settings updated via API, reloading...');
        await reloadSettings();
    });

    document.addEventListener('contextmenu', (e) => e.preventDefault());
    
    // Apply display settings after a short delay to ensure DOM is ready
    setTimeout(applyDisplaySettings, 100);
})();

// Console command to refresh photo
window.refreshPhoto = async function() {
    console.log('🔄 Manually refreshing photo...');
    try {
        if (!currentWeather && window.userLocation) {
            console.log('⚠️ Weather not loaded yet, fetching weather first...');
            await updateWeather(window.userLocation);
        }
        
        console.log('📸 Fetching new photo with current context...');
        await fetchUnsplashPhoto(true);
        console.log('✅ Photo refreshed successfully!');
    } catch (error) {
        console.error('❌ Failed to refresh photo:', error);
    }
};

// Console command to get settings
window.getSettings = async function() {
    try {
        const settings = await invoke('get_settings');
        console.log('Current settings:', settings);
        return settings;
    } catch (error) {
        console.error('❌ Failed to get settings:', error);
    }
};

// Console command to save settings
window.saveSettings = async function(settings) {
    try {
        await invoke('save_settings', { settings });
        // Settings will be automatically reloaded by the checkSettingsChanges interval
        console.log('✅ Settings saved! Changes will apply automatically...');
        return true;
    } catch (error) {
        console.error('❌ Failed to save settings:', error);
        return false;
    }
};

// Console command to reset settings
window.resetSettings = async function() {
    try {
        const settings = await invoke('reset_settings');
        // Settings will be automatically reloaded by the checkSettingsChanges interval
        console.log('✅ Settings reset to defaults! Changes will apply automatically...', settings);
        return settings;
    } catch (error) {
        console.error('❌ Failed to reset settings:', error);
    }
};

// Console command to manually reload settings
window.reloadSettings = reloadSettings;

// Console command to change theme
window.setTheme = async function(themeName) {
    try {
        const settings = await invoke('get_settings');
        settings.display.theme = themeName;
        await invoke('save_settings', { settings });
        console.log(`✅ Theme changed to: ${themeName}`);
        await reloadSettings();
        return true;
    } catch (error) {
        console.error('❌ Failed to change theme:', error);
        return false;
    }
};

// Listen for photo refresh events from HTTP API
const { listen } = window.__TAURI__.event;
listen('refresh-photo', async () => {
    console.log('🔔 Photo refresh event received from API');
    await window.refreshPhoto();
});

console.log('%c🎨 Idleview Debug Console', 'font-size: 14px; font-weight: bold; color: #4f46e5');
console.log('%cCommands: refreshPhoto() | getSettings() | saveSettings(obj) | resetSettings() | reloadSettings() | setTheme(name)', 'color: #64748b');
console.log('%cThemes: "minimal" | "geometric" | "sidebar"', 'color: #64748b');
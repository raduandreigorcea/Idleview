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

// Update weather display
function updateWeatherDisplay(weather) {
    const tempUnit = weather.temperature_unit === 'fahrenheit' ? '°F' : '°C';
    document.getElementById('temp').textContent = `${Math.round(weather.temperature)} ${tempUnit}`;
    
    document.getElementById('humidity').textContent = `${weather.humidity}%`;
    document.getElementById('wind').textContent = `${Math.round(weather.wind_speed)} ${weather.wind_speed_label}`;
    document.getElementById('cloudiness').textContent = `${weather.cloudcover}%`;

    // Update precipitation using Rust logic
    invoke('get_precipitation_display', { weather }).then(precip => {
        document.getElementById('precip-icon').src = `assets/${precip.icon}`;
        document.getElementById('precip-label').textContent = precip.label;
        document.getElementById('precipitation').textContent = precip.value;
    });

    // Update sunrise/sunset (use 12h or 24h based on settings)
    const sunrise = new Date(weather.sunrise);
    const sunset = new Date(weather.sunset);
    const timeFormat = userSettings?.units?.time_format === '12h' 
        ? { hour: '2-digit', minute: '2-digit', hour12: true }
        : { hour: '2-digit', minute: '2-digit', hour12: false };
    
    document.getElementById('sunrise').textContent = sunrise.toLocaleTimeString('en-US', timeFormat);
    document.getElementById('sunset').textContent = sunset.toLocaleTimeString('en-US', timeFormat);

    currentWeather = weather;
}

// Fetch and display location
async function fetchLocation() {
    try {
        const location = await invoke('get_location');
        document.getElementById('location').textContent = 
            location.city || `${location.latitude.toFixed(2)}°, ${location.longitude.toFixed(2)}°`;
        await updateWeather(location);
    } catch (error) {
        console.error('Failed to fetch location:', error);
        document.getElementById('location').textContent = 'Unknown';
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
    // Check if CPU temp should be shown in settings
    if (userSettings && !userSettings.display.show_cpu_temp) {
        document.querySelector('.cpu-card').style.display = 'none';
        return;
    }
    
    try {
        const temp = await invoke('get_cpu_temp');
        const cpuCard = document.querySelector('.cpu-card');
        
        if (temp.value > 0) {
            document.getElementById('cpu-temp').textContent = temp.display;
            cpuCard.style.display = 'flex';
        } else {
            cpuCard.style.display = 'none';
        }
    } catch (error) {
        console.error('Failed to fetch CPU temp:', error);
        document.querySelector('.cpu-card').style.display = 'none';
    }
}

// Update time and date
async function updateTimeAndDate() {
    try {
        const timeData = await invoke('get_current_time');
        const timeElement = document.getElementById('time');
        timeElement.textContent = timeData.time;
        
        document.getElementById('date').innerHTML = `${timeData.day_of_week}<br>${timeData.date}`;
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
    applyTheme(userSettings.display.theme || 'default');
    
    // Apply card position to both card containers
    const cardPosition = userSettings.display.card_position || 'left';
    const cardWrapper = document.getElementById('card_wrapper');
    const weatherWrapper = document.getElementById('weather_details_wrapper');
    cardWrapper.setAttribute('data-position', cardPosition);
    weatherWrapper.setAttribute('data-position', cardPosition);
    
    // Set debug panel to opposite side of cards
    const debugEl = document.getElementById('debug');
    const debugPosition = cardPosition === 'left' ? 'right' : 'left';
    debugEl.setAttribute('data-position', debugPosition);
    
    const weatherCards = document.querySelectorAll('#weather_details_wrapper .card');
    
    // Card order: [Humidity+Wind], [Precipitation+Cloudiness], [Sunrise+Sunset], [CPU]
    if (weatherCards.length >= 3) {
        weatherCards[0].style.display = userSettings.display.show_humidity_wind ? 'grid' : 'none';
        weatherCards[1].style.display = userSettings.display.show_precipitation_cloudiness ? 'grid' : 'none';
        weatherCards[2].style.display = userSettings.display.show_sunrise_sunset ? 'grid' : 'none';
    }
    
    // CPU card visibility is handled in updateCPUTemp()
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
    const currentTheme = document.body.getAttribute('data-theme');
    if (currentTheme !== themeName) {
        document.body.setAttribute('data-theme', themeName);
        console.log(`🎨 Theme applied: ${themeName}`);
    }
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
                        const refreshInterval = (userSettings?.photos?.refresh_interval || 30) * 60 * 1000;
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
        const queryResult = await invoke('build_photo_query', {
            cloudcover: currentWeather.cloudcover,
            rain: currentWeather.rain,
            snowfall: currentWeather.snowfall,
            sunriseIso: currentWeather.sunrise,
            sunsetIso: currentWeather.sunset,
            enableFestive: userSettings?.photos?.enable_festive_queries ?? true
        });
        
        console.log(`📸 Fetching Photo | Query: "${queryResult.query}" | ${window.innerWidth}x${window.innerHeight} @ ${userSettings?.photos?.photo_quality || '85'}%`);
        
        const photo = await invoke('get_unsplash_photo', { 
            width: window.innerWidth, 
            height: window.innerHeight,
            query: queryResult.query
        });
        
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
        
        const queryResult = await invoke('build_photo_query', {
            cloudcover: currentWeather.cloudcover,
            rain: currentWeather.rain,
            snowfall: currentWeather.snowfall,
            sunriseIso: currentWeather.sunrise,
            sunsetIso: currentWeather.sunset,
            enableFestive: userSettings?.photos?.enable_festive_queries ?? true
        });
        
        const photo = await invoke('get_unsplash_photo', { 
            width: window.innerWidth, 
            height: window.innerHeight,
            query: queryResult.query
        });
        
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
        const refreshInterval = (userSettings?.photos?.refresh_interval || 30) * 60 * 1000;
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

    // Check photo context immediately on startup, then frequently
    // Use a 30 second interval to catch expiry within reasonable time
    console.log('🔄 Starting photo context check interval (30s)...');
    checkPhotoContext();
    const photoContextInterval = setInterval(() => {
        console.log('⏰ Photo context check triggered');
        checkPhotoContext();
    }, 30 * 1000);
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
console.log('%cThemes: "default" | "light" | "minimal"', 'color: #64748b');
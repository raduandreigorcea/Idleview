const invoke = window.__TAURI__.core.invoke;

// Toggle debug output
let SHOW_DEBUG = false;

// Store state
let currentWeather = null;
let currentPhotoUrl = null;
let debugInterval = null;
let prefetchedPhoto = null; // Store prefetched photo

// Update weather display
function updateWeatherDisplay(weather) {
    document.getElementById('temp').textContent = `${Math.round(weather.temperature)}° C`;
    document.getElementById('humidity').textContent = `${weather.humidity}%`;
    document.getElementById('wind').textContent = `${Math.round(weather.wind_speed)} km/h`;
    document.getElementById('cloudiness').textContent = `${weather.cloudcover}%`;

    // Update precipitation using Rust logic
    invoke('get_precipitation_display', { weather }).then(precip => {
        document.getElementById('precip-icon').src = `assets/${precip.icon}`;
        document.getElementById('precip-label').textContent = precip.label;
        document.getElementById('precipitation').textContent = precip.value;
    });

    // Update sunrise/sunset
    const sunrise = new Date(weather.sunrise);
    const sunset = new Date(weather.sunset);
    document.getElementById('sunrise').textContent = sunrise.toLocaleTimeString('en-US', { 
        hour: '2-digit', 
        minute: '2-digit', 
        hour12: false 
    });
    document.getElementById('sunset').textContent = sunset.toLocaleTimeString('en-US', { 
        hour: '2-digit', 
        minute: '2-digit', 
        hour12: false 
    });

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
    try {
        const temp = await invoke('get_cpu_temp');
        const cpuCard = document.querySelector('.cpu-card');
        
        if (temp > 0) {
            document.getElementById('cpu-temp').textContent = `${Math.round(temp)}°C`;
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
        document.getElementById('time').textContent = timeData.time;
        document.getElementById('date').innerHTML = `${timeData.day_of_week}<br>${timeData.date}`;
    } catch (error) {
        console.error('Failed to update time:', error);
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

// Display photo
async function displayPhoto(photo, timestamp = null, query = null) {
    currentPhotoUrl = photo.url;
    
    // Apply background image
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

    // Debug display
    if (SHOW_DEBUG) {
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
                    
                    debugEl.innerHTML = `
                        <div>Photo cached: ${debugInfo.photo_age}</div>
                        <div>Query: ${debugInfo.query}</div>
                        <div>Time: ${debugInfo.time_of_day} (${debugInfo.time_source})</div>
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
            const isValid = await invoke('is_cache_valid', { 
                cacheTimestamp: cached.timestamp 
            });
            
            if (isValid) {
                console.log('Using cached photo');
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
        
        console.log('Fetching new photo from Unsplash...');
        
        // Build query using Rust backend with weather data
        const queryResult = await invoke('build_photo_query', {
            cloudcover: currentWeather.cloudcover,
            rain: currentWeather.rain,
            snowfall: currentWeather.snowfall,
            sunriseIso: currentWeather.sunrise,
            sunsetIso: currentWeather.sunset
        });
        
        console.log('Photo query:', queryResult.query);
        
        const photo = await invoke('get_unsplash_photo', { 
            width: window.innerWidth, 
            height: window.innerHeight,
            query: queryResult.query
        });
        
        console.log('Photo fetched successfully');
        
        const nowTs = Date.now();
        cachePhoto(photo, queryResult.query);
        await displayPhoto(photo, nowTs, queryResult.query);
        
    } catch (error) {
        console.error('Failed to fetch Unsplash photo:', error);
        
        // Fallback to cache
        const cached = getCachedPhoto();
        if (cached) {
            console.log('Using cached photo as fallback');
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
            sunsetIso: currentWeather.sunset
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
    if (!cached) return;
    
    try {
        const now = Date.now();
        const cacheAge = now - cached.timestamp;
        const twentyNineMinutes = 29 * 60 * 1000; // 29 minutes
        const thirtyMinutes = 30 * 60 * 1000; // 30 minutes
        
        // At 29 minutes (1 min before expiry), prefetch the next photo
        if (cacheAge >= twentyNineMinutes && cacheAge < thirtyMinutes && !prefetchedPhoto) {
            console.log('Cache at 29min, prefetching next photo...');
            await prefetchNextPhoto();
        }
        
        // At 30 minutes, switch to the prefetched photo (or fetch if prefetch failed)
        const isValid = await invoke('is_cache_valid', { 
            cacheTimestamp: cached.timestamp 
        });
        
        if (!isValid) {
            console.log('Cache expired (30min), switching to new photo...');
            await fetchUnsplashPhoto(true); // Will use prefetched if available
        }
    } catch (error) {
        console.error('Failed to check photo context:', error);
    }
}

// Initialize
updateTimeAndDate();
setInterval(updateTimeAndDate, 1000);

updateCPUTemp();
setInterval(updateCPUTemp, 10 * 1000);

// Set up intervals - store location for weather updates
let userLocation = null;
fetchLocation().then(() => {
    invoke('get_location').then(location => {
        userLocation = location;
        setInterval(() => updateWeather(userLocation), 15 * 60 * 1000);
    });
});

setInterval(() => fetchUnsplashPhoto(true), 30 * 60 * 1000);
setInterval(checkPhotoContext, 5 * 60 * 1000);

document.addEventListener('contextmenu', (e) => e.preventDefault());

// Console command to refresh photo
window.refreshPhoto = async function() {
    console.log('🔄 Manually refreshing photo...');
    try {
        if (!currentWeather && userLocation) {
            console.log('⚠️ Weather not loaded yet, fetching weather first...');
            await updateWeather(userLocation);
        }
        
        console.log('📸 Fetching new photo with current context...');
        await fetchUnsplashPhoto(true);
        console.log('✅ Photo refreshed successfully!');
    } catch (error) {
        console.error('❌ Failed to refresh photo:', error);
    }
};

console.log('💡 Tip: Type refreshPhoto() in console to fetch a new background photo');
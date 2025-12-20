# Idleview

![Dashboard Screenshot](image.png)

A beautiful, fullscreen dashboard application built with Tauri that displays the time, date, weather, and stunning contextual background images from Unsplash. Perfect for Raspberry Pi displays, smart mirrors, or any always-on display setup.

## ✨ Features

###  **Smart Background Images**
- **Seasonal Awareness**: Automatically adjusts photos based on spring, summer, autumn, or winter
- **Holiday Themes**: Special imagery for Christmas, New Year, Halloween, and Easter
- **Time-Based Moods**: 
  - Dawn : Moody, atmospheric sunrise photos
  - Day : Bright, vibrant landscapes
  - Dusk : Golden hour, sunset vibes
  - Night : Dark, moody nightscapes
- **Weather Integration**: Shows cozy rain or snow photos when it's raining or snowing
- **Powered by Unsplash**: High-quality, curated photography

### 🌐 **HTTP Server & Control Panel**
- **Built-in Web Server**: REST API for remote configuration
- **Web Control Panel**: Beautiful, responsive interface for settings management
- **Network Access**: Control your display from any device on your local network
- **Real-time Updates**: Changes apply instantly via API
- **Cross-platform Settings**: Automatic platform-specific configuration storage

### **Real-Time Information**
- **Live Clock**: Updates every second with selected format
- **Current Date**: Clean, readable date display
- **Local Weather**: Real-time temperature in Celsius or Fahrenheit
- **Auto-Location**: Automatically detects your location via IP

###  **Performance**
- **Built with Tauri**: Native performance with minimal resource usage
- **Rust Backend**: Lightning-fast API calls and data processing
- **Fullscreen Mode**: Immersive, distraction-free display

## 🛠️ Technologies

- **[Tauri v2](https://tauri.app/)**: Cross-platform desktop framework
- **Rust**: Backend logic and API integration
- **[Axum](https://docs.rs/axum/)**: High-performance HTTP server
- **JavaScript**: Frontend interactivity
- **HTML/CSS**: Clean, modern UI
- **APIs**:
  - [Unsplash API](https://unsplash.com/developers): Background images
  - [Open-Meteo](https://open-meteo.com/): Weather data
  - [IP-API](http://ip-api.com/): Geolocation

---

**Made with ❤️ for Raspberry Pi enthusiasts**

# Theme API Documentation

## Available Themes
- `default` - Dark glassmorphic cards at top-right, weather at bottom-left
- `nest` - Google Nest-inspired with time/temp at bottom-left, weather pills at top-right
- `apple` - Apple Liquid Glass aesthetic with vibrant frosted glass, cards centered at top
- `clean` - No card backgrounds, just text with shadows - main cards at bottom-left, weather at bottom-right

## API Endpoints for Theme Management

### Get Current Theme
```javascript
// GET /api/settings
const response = await fetch('http://localhost:8737/api/settings');
const settings = await response.json();
console.log(settings.display.theme); // "default" or "nest"
```

### Change Theme (Full Update)
```javascript
// PUT /api/settings
const response = await fetch('http://localhost:8737/api/settings', {
  method: 'PUT',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    units: {
      temperature_unit: "celsius",
      time_format: "24h",
      date_format: "dmy",
      wind_speed_unit: "kmh"
    },
    display: {
      show_humidity_wind: true,
      show_precipitation_cloudiness: true,
      show_sunrise_sunset: true,
      show_cpu_temp: false,
      theme: "nest"  // ← Change theme here
    },
    photos: {
      refresh_interval: 30,
      photo_quality: 80
    }
  })
});
```

### Change Theme (Partial Update - Recommended)
```javascript
// PATCH /api/settings
const response = await fetch('http://localhost:8737/api/settings', {
  method: 'PATCH',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    display: {
      theme: "nest"  // Only update the theme
    }
  })
});

if (response.ok) {
  console.log('Theme changed successfully!');
}
```

## Vue Control Panel Implementation

### 1. Add Theme Selector to App.vue

```vue
<template>
  <div class="setting-group">
    <h3>Theme</h3>
    <SelectInput
      label="App Theme"
      v-model="settings.display.theme"
      :options="[
        { value: 'default', label: 'Default - Glass Cards' },
        { value: 'nest', label: 'Google Nest - Bottom/Top Layout' },
        { value: 'apple', label: 'Apple Liquid Glass - Vibrant Frosted' },
        { value: 'clean', label: 'Clean - Text Only, No Cards' }
      ]"
    />
  </div>
</template>

<script setup>
import { ref, watch, onMounted } from 'vue';

const settings = ref({
  display: {
    theme: 'default'
  }
});

// Load settings on mount
onMounted(async () => {
  const response = await fetch('/api/settings');
  settings.value = await response.json();
});

// Watch for changes and auto-save
watch(settings, async (newSettings) => {
  await fetch('/api/settings', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(newSettings)
  });
}, { deep: true });
</script>
```

### 2. Create ThemeSelector Component (Optional)

```vue
<!-- ThemeSelector.vue -->
<template>
  <div class="theme-selector">
    <div 
      v-for="theme in themes" 
      :key="theme.value"
      class="theme-card"
      :class="{ active: modelValue === theme.value }"
      @click="$emit('update:modelValue', theme.value)"
    >
      <div class="theme-preview" :data-theme="theme.value">
        <div class="preview-card"></div>
      </div>
      <span>{{ theme.label }}</span>
    </div>
  </div>
</template>

<script setup>
defineProps({
  modelValue: String
});

defineEmits(['update:modelValue']);

const themes = [
  { value: 'default', label: 'Default' },
  { value: 'nest', label: 'Google Nest' }
];
</script>

<style scoped>
.theme-selector {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
  gap: 1rem;
}

.theme-card {
  cursor: pointer;
  padding: 1rem;
  border: 2px solid transparent;
  border-radius: 8px;
  transition: all 0.2s;
}

.theme-card.active {
  border-color: #4f46e5;
  background: rgba(79, 70, 229, 0.1);
}

.theme-preview {
  width: 100%;
  height: 80px;
  border-radius: 6px;
  overflow: hidden;
  margin-bottom: 0.5rem;
}

.theme-preview[data-theme="default"] {
  background: linear-gradient(135deg, #1e293b 0%, #0f172a 100%);
}

.theme-preview[data-theme="nest"] {
  background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
}

.preview-card {
  width: 60%;
  height: 40px;
  margin: 20px auto;
  background: rgba(255, 255, 255, 0.15);
  border-radius: 8px;
  backdrop-filter: blur(10px);
}
</style>
```

## Testing in Browser Console

Open DevTools (F12) and run:

```javascript
// Switch to Nest theme
setTheme('nest')

// Switch back to default
setTheme('default')

// Check current theme
const settings = await getSettings();
console.log(settings.display.theme);
```

## cURL Examples

```bash
# Get current theme
curl http://localhost:8737/api/settings | jq '.display.theme'

# Change to Nest theme (PATCH)
curl -X PATCH http://localhost:8737/api/settings \
  -H "Content-Type: application/json" \
  -d '{"display":{"theme":"nest"}}'

# Change to default theme
curl -X PATCH http://localhost:8737/api/settings \
  -H "Content-Type: application/json" \
  -d '{"display":{"theme":"default"}}'
```

## PowerShell Examples

```powershell
# Get current theme
$settings = Invoke-RestMethod -Uri "http://localhost:8737/api/settings"
$settings.display.theme

# Change to Nest theme
$body = @{
  display = @{
    theme = "nest"
  }
} | ConvertTo-Json

Invoke-RestMethod -Uri "http://localhost:8737/api/settings" `
  -Method PATCH `
  -ContentType "application/json" `
  -Body $body

# Change to default theme
$body = @{
  display = @{
    theme = "default"
  }
} | ConvertTo-Json

Invoke-RestMethod -Uri "http://localhost:8737/api/settings" `
  -Method PATCH `
  -ContentType "application/json" `
  -Body $body
```

---

## Copilot Prompt for Vue Theme Selector

```
Create a theme selector component for my Idleview Vue control panel that:

1. Displays available themes with visual previews
2. Uses the API endpoint PATCH http://localhost:8737/api/settings
3. Updates the settings.display.theme field
4. Available themes: "default" (dark glass) and "nest" (purple gradient with white cards)
5. Shows a preview card for each theme with representative colors
6. Highlights the currently selected theme
7. Auto-saves when theme is changed

The component should integrate with the existing settings reactive object and use the watch functionality to auto-save via PATCH request.
```

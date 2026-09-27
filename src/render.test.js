import { beforeEach, expect, it } from 'vitest';
import { render } from './render.js';
import html from './index.html?raw';

// The shape idleview_core::View serialises to - here, the screenshot.
const view = () => ({
    time: '10:42',
    period: null,
    weekday: 'Sunday',
    date: 'Apr 26, 2026',
    location: 'Bucharest',
    weather: {
        temperature: '18 °C', sunrise: '06:13', sunset: '20:12',
        precip_icon: 'umbrella.svg', precip_label: 'Precip', precip_value: 'Clear',
        humidity: '50%', wind: '12 km/h', clouds: '0%',
    },
    photo: null,
    show: {
        show_clock: true, show_date: true, show_weekday: true, show_location: true,
        show_temperature: true, show_sunrise_sunset: true,
        show_precipitation_cloudiness: true, show_humidity_wind: true,
    },
});

const text = (id) => document.getElementById(id).textContent;
const hidden = (selector) => document.querySelector(selector).hidden;

beforeEach(() => {
    document.body.innerHTML = new DOMParser().parseFromString(html, 'text/html').body.innerHTML;
});

it('places every field of the view', () => {
    render(view());
    expect(text('time-value')).toBe('10:42');
    expect(hidden('#time-period')).toBe(true);
    expect(text('weekday')).toBe('Sunday');
    expect(text('date-value')).toBe('Apr 26, 2026');
    expect(text('location')).toBe('Bucharest');
    expect(text('temp')).toBe('18 °C');
    expect(text('sunrise')).toBe('06:13');
    expect(text('precipitation')).toBe('Clear');
    expect(text('cloudiness')).toBe('0%');
});

it('hides what the settings switch off, and the dock when it would be empty', () => {
    const v = view();
    v.period = 'PM';
    v.show.show_weekday = false;
    v.show.show_location = false;
    v.show.show_sunrise_sunset = false;
    render(v);

    expect(text('time-period')).toBe('PM');
    expect(hidden('#time-period')).toBe(false);
    expect(hidden('#weekday')).toBe(true);
    expect(hidden('#date')).toBe(false);
    expect(hidden('.location-badge')).toBe(true);
    expect(hidden('[data-metric="sunrise"]')).toBe(true);
    expect(hidden('[data-metric="wind"]')).toBe(false);

    Object.keys(v.show).forEach((key) => { if (key !== 'show_clock') v.show[key] = false; });
    render(v);
    expect(hidden('.bottom-section')).toBe(true);
    expect(hidden('#date')).toBe(true);
});

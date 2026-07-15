import { describe, it, expect } from 'vitest';
import {
    photoSettingsSignature,
    getPrecipState,
    refreshIntervalMs,
    DEFAULT_REFRESH_INTERVAL_MINUTES,
} from './photo-settings.js';

describe('photoSettingsSignature', () => {
    const base = {
        photos: { custom_query: '', enable_festive_queries: true, photo_quality: '80', refresh_interval: 30 },
    };

    it('changes when custom_query changes, so a new query forces a refetch', () => {
        const withQuery = { photos: { ...base.photos, custom_query: 'kyoto temple' } };
        expect(photoSettingsSignature(withQuery)).not.toBe(photoSettingsSignature(base));
    });

    it('changes when festive queries are toggled', () => {
        const off = { photos: { ...base.photos, enable_festive_queries: false } };
        expect(photoSettingsSignature(off)).not.toBe(photoSettingsSignature(base));
    });

    it('changes when photo quality changes', () => {
        const hq = { photos: { ...base.photos, photo_quality: '100' } };
        expect(photoSettingsSignature(hq)).not.toBe(photoSettingsSignature(base));
    });

    it('ignores refresh_interval, which does not change which photo is correct', () => {
        const slower = { photos: { ...base.photos, refresh_interval: 60 } };
        expect(photoSettingsSignature(slower)).toBe(photoSettingsSignature(base));
    });

    it('ignores unrelated settings like fonts', () => {
        const restyled = { ...base, display: { clock_font: 'inter' } };
        expect(photoSettingsSignature(restyled)).toBe(photoSettingsSignature(base));
    });

    it('is stable across repeated calls and tolerates missing settings', () => {
        expect(photoSettingsSignature(base)).toBe(photoSettingsSignature(base));
        expect(photoSettingsSignature(undefined)).toBe(photoSettingsSignature({}));
    });
});

describe('getPrecipState', () => {
    it('reports snow from measured snowfall or a snow weathercode', () => {
        expect(getPrecipState({ snowfall: 2, weathercode: 0 })).toBe('snow');
        expect(getPrecipState({ snowfall: 0, weathercode: 73 })).toBe('snow');
        expect(getPrecipState({ snowfall: 0, weathercode: 86 })).toBe('snow');
    });

    it('reports rain from measured rain or a rain weathercode', () => {
        expect(getPrecipState({ rain: 1.2, weathercode: 0 })).toBe('rain');
        expect(getPrecipState({ rain: 0, weathercode: 61 })).toBe('rain');
        expect(getPrecipState({ rain: 0, weathercode: 95 })).toBe('rain');
    });

    it('prefers snow over rain when both are indicated', () => {
        expect(getPrecipState({ rain: 1, snowfall: 1, weathercode: 71 })).toBe('snow');
    });

    it('reports none for clear weather or missing data', () => {
        expect(getPrecipState({ rain: 0, snowfall: 0, weathercode: 0 })).toBe('none');
        expect(getPrecipState({})).toBe('none');
    });
});

describe('refreshIntervalMs', () => {
    it('converts the configured interval from minutes to milliseconds', () => {
        expect(refreshIntervalMs({ photos: { refresh_interval: 15 } })).toBe(15 * 60 * 1000);
    });

    it('falls back to the default when unset or zero', () => {
        const fallback = DEFAULT_REFRESH_INTERVAL_MINUTES * 60 * 1000;
        expect(refreshIntervalMs(undefined)).toBe(fallback);
        expect(refreshIntervalMs({ photos: {} })).toBe(fallback);
        expect(refreshIntervalMs({ photos: { refresh_interval: 0 } })).toBe(fallback);
    });
});

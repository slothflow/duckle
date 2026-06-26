import { loadPersisted, savePersisted } from './persistence';

// Global UI font size, persisted locally (like the theme / Dives toggle) and
// applied by setting the --app-font-size CSS variable on the root element. All
// inheriting UI text scales from this. Bounded so the layout stays usable.
export const DEFAULT_FONT_SIZE = 13;
export const MIN_FONT_SIZE = 11;
export const MAX_FONT_SIZE = 18;

export function getFontSize(): number {
    const v = loadPersisted('appFontSize', DEFAULT_FONT_SIZE);
    return typeof v === 'number' && v >= MIN_FONT_SIZE && v <= MAX_FONT_SIZE
        ? v
        : DEFAULT_FONT_SIZE;
}

export function applyFontSize(px: number): void {
    document.documentElement.style.setProperty('--app-font-size', `${px}px`);
}

// Clamp, persist and apply in one call; returns the value actually used.
export function setFontSize(px: number): number {
    const clamped = Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, Math.round(px)));
    savePersisted('appFontSize', clamped);
    applyFontSize(clamped);
    return clamped;
}

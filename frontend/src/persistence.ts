const KEY_PREFIX = 'duckle:v1:';

export function loadPersisted<T>(key: string, fallback: T): T {
    try {
        const raw = localStorage.getItem(KEY_PREFIX + key);
        if (raw == null) return fallback;
        return JSON.parse(raw) as T;
    } catch {
        return fallback;
    }
}

export function savePersisted<T>(key: string, value: T): void {
    try {
        localStorage.setItem(KEY_PREFIX + key, JSON.stringify(value));
    } catch {
        /* quota exceeded / localStorage disabled - silently drop */
    }
}

export function clearPersisted(): void {
    try {
        const keys: string[] = [];
        for (let i = 0; i < localStorage.length; i++) {
            const k = localStorage.key(i);
            if (k && k.startsWith(KEY_PREFIX)) keys.push(k);
        }
        for (const k of keys) localStorage.removeItem(k);
    } catch {
        /* ignore */
    }
}

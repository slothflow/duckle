import { open as tauriOpen, save as tauriSave } from '@tauri-apps/plugin-dialog';

export type FileFilter = { name: string; extensions: string[] };

function isTauriContext(): boolean {
    if (typeof window === 'undefined') return false;
    return (
        '__TAURI_INTERNALS__' in window ||
        '__TAURI__' in window ||
        '__TAURI_IPC__' in window
    );
}

export function isTauri(): boolean {
    return isTauriContext();
}

export async function pickFile(opts?: {
    filters?: FileFilter[];
    title?: string;
}): Promise<string | null> {
    if (isTauriContext()) {
        try {
            const result = await tauriOpen({
                multiple: false,
                directory: false,
                filters: opts?.filters,
                title: opts?.title,
            });
            if (Array.isArray(result)) return result[0] ?? null;
            return result ?? null;
        } catch (err) {
            console.error('Tauri file picker failed', err);
            return null;
        }
    }
    return pickFileBrowser(opts?.filters);
}

export async function pickSavePath(opts?: {
    defaultPath?: string;
    filters?: FileFilter[];
    title?: string;
}): Promise<string | null> {
    if (isTauriContext()) {
        try {
            return await tauriSave({
                defaultPath: opts?.defaultPath,
                filters: opts?.filters,
                title: opts?.title,
            });
        } catch (err) {
            console.error('Tauri save picker failed', err);
            return null;
        }
    }
    return pickSaveBrowser(opts?.defaultPath);
}

function pickFileBrowser(filters?: FileFilter[]): Promise<string | null> {
    return new Promise(resolve => {
        const input = document.createElement('input');
        input.type = 'file';
        if (filters) {
            const exts = filters.flatMap(f => f.extensions).filter(e => e !== '*');
            if (exts.length) input.accept = exts.map(e => '.' + e).join(',');
        }
        input.style.display = 'none';
        input.onchange = () => {
            const file = input.files?.[0];
            resolve(file ? file.name : null);
            document.body.removeChild(input);
        };
        // Cancel detection — focus returns without change
        const cancelHandler = () => {
            setTimeout(() => {
                if (!input.files || input.files.length === 0) {
                    resolve(null);
                    if (input.parentNode) document.body.removeChild(input);
                }
            }, 300);
            window.removeEventListener('focus', cancelHandler);
        };
        window.addEventListener('focus', cancelHandler, { once: true });
        document.body.appendChild(input);
        input.click();
    });
}

function pickSaveBrowser(defaultPath?: string): Promise<string | null> {
    const result = window.prompt('Save as (file path):', defaultPath ?? '');
    return Promise.resolve(result || null);
}

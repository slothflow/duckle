import { useEffect, useState } from 'react';
import { Copy, Minus, Square, X } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTauri } from '../tauri-dialog';

/**
 * Minimize / maximize / close controls for the frameless window,
 * rendered into Duckle's own topbar. Only shows under Tauri - in the
 * browser the OS chrome (or none) applies.
 */
export default function WindowControls() {
    const [maximized, setMaximized] = useState(false);

    useEffect(() => {
        if (!isTauri()) return;
        const win = getCurrentWindow();
        let unlisten: (() => void) | undefined;
        void win.isMaximized().then(setMaximized).catch(() => {});
        win.onResized(() => {
            void win.isMaximized().then(setMaximized).catch(() => {});
        })
            .then(u => {
                unlisten = u;
            })
            .catch(() => {});
        return () => unlisten?.();
    }, []);

    if (!isTauri()) return null;
    const win = getCurrentWindow();

    return (
        <div className="win-controls">
            <button
                type="button"
                className="win-ctl"
                title="Minimize"
                aria-label="Minimize"
                onClick={() => void win.minimize()}
            >
                <Minus size={15} />
            </button>
            <button
                type="button"
                className="win-ctl"
                title={maximized ? 'Restore' : 'Maximize'}
                aria-label={maximized ? 'Restore' : 'Maximize'}
                onClick={() => void win.toggleMaximize()}
            >
                {maximized ? <Copy size={11} /> : <Square size={11} />}
            </button>
            <button
                type="button"
                className="win-ctl win-ctl-close"
                title="Close"
                aria-label="Close"
                onClick={() => void win.close()}
            >
                <X size={15} />
            </button>
        </div>
    );
}

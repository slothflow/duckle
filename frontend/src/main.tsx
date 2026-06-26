import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource-variable/inter';
import './i18n';            // bootstraps i18next; sets document.dir for RTL
import App from './App';
import { ShareView } from './ShareView';
import { ThemeProvider } from './theme';
import { isTauri } from './tauri-dialog';
import { applyFontSize, getFontSize } from './font-size';
import './styles.css';

// Apply the saved UI font size before first paint so the app renders at the
// user's chosen size with no flash of the default.
applyFontSize(getFontSize());

const rootEl = document.getElementById('root');
if (!rootEl) {
    throw new Error('Root element #root not found');
}

// Standalone read-only share routes: /dive/<id> and /dash/<id>. Anything else
// is the full editor.
const shareMatch = window.location.pathname.match(/^\/(dive|dash)\/(.+?)\/?$/);

createRoot(rootEl).render(
    <StrictMode>
        <ThemeProvider>
            {shareMatch ? (
                <ShareView kind={shareMatch[1] as 'dive' | 'dash'} id={decodeURIComponent(shareMatch[2])} />
            ) : (
                <App />
            )}
        </ThemeProvider>
    </StrictMode>,
);

// The Tauri window launches hidden (visible:false) so users never see a
// white flash or unstyled paint. Reveal it once the UI has rendered.
if (isTauri()) {
    const reveal = async () => {
        try {
            const { getCurrentWindow } = await import('@tauri-apps/api/window');
            const win = getCurrentWindow();
            await win.show();
            await win.setFocus();
        } catch {
            /* not in Tauri / API unavailable - window is already visible */
        }
    };
    // Two RAFs ≈ after first paint commit.
    requestAnimationFrame(() => requestAnimationFrame(() => void reveal()));
}

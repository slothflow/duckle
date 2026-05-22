import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource-variable/inter';
import App from './App';
import { ThemeProvider } from './theme';
import { isTauri } from './tauri-dialog';
import './styles.css';

const rootEl = document.getElementById('root');
if (!rootEl) {
    throw new Error('Root element #root not found');
}

createRoot(rootEl).render(
    <StrictMode>
        <ThemeProvider>
            <App />
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

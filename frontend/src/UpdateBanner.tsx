import { useEffect, useState } from 'react';
import { checkForUpdate, type UpdateInfo } from './tauri-bridge';
import { openExternal } from './tauri-io';

/**
 * Thin "a newer build is available" bar shown at the top of the window when
 * the backend's update check (build time vs the latest GitHub release asset for
 * this OS) reports a newer binary. Dismissible per session. No-op in the
 * browser build or when offline / already current.
 *
 * Preview: a freshly built local binary is newer than the published release, so
 * the banner won't show for real. To review its UI/UX, set
 * `localStorage.setItem('duckle.previewUpdateBanner', '1')` in devtools and
 * reload - it then renders with the real fetched release info if any, otherwise
 * a representative placeholder.
 */
export function UpdateBanner() {
    const [info, setInfo] = useState<UpdateInfo | null>(null);
    const [dismissed, setDismissed] = useState(false);
    const preview =
        typeof window !== 'undefined' &&
        window.localStorage?.getItem('duckle.previewUpdateBanner') === '1';

    useEffect(() => {
        let cancelled = false;
        const timer = setTimeout(() => {
            checkForUpdate().then((r) => {
                if (!cancelled && r && r.update_available) setInfo(r);
            });
        }, 3000);
        return () => {
            cancelled = true;
            clearTimeout(timer);
        };
    }, []);

    const show = preview || (info && info.update_available);
    if (!show || dismissed) return null;

    const display: UpdateInfo = info ?? {
        update_available: true,
        current_build: 'this build',
        latest_tag: 'v0.1.0-hotfix2',
        latest_date: null,
        asset_name: null,
        release_url: 'https://github.com/SouravRoy-ETL/duckle/releases',
        download_url: null,
        error: null,
    };
    const url = display.release_url ?? display.download_url ?? null;
    const tag = display.latest_tag ? ` ${display.latest_tag}` : '';

    return (
        <div className="update-banner" role="status">
            <span className="update-banner-icon" aria-hidden="true">
                ⬆
            </span>
            <span className="update-banner-text">
                A newer Duckle build{tag} is available. You're on {display.current_build}.
            </span>
            <button
                type="button"
                className="update-banner-cta"
                disabled={!url}
                onClick={() => {
                    if (url) void openExternal(url);
                }}
            >
                Get the update
            </button>
            <button
                type="button"
                className="update-banner-dismiss"
                aria-label="Dismiss"
                title="Dismiss"
                onClick={() => setDismissed(true)}
            >
                ×
            </button>
        </div>
    );
}

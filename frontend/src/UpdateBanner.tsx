import { useEffect, useState } from 'react';
import {
    checkForUpdate,
    selfUpdate,
    type SelfUpdateProgress,
    type UpdateInfo,
} from './tauri-bridge';
import { openExternal } from './tauri-io';

/**
 * Thin "a newer build is available" bar shown at the top of the window when
 * the backend's update check (build time vs the latest GitHub release asset for
 * this OS) reports a newer binary. Dismissible per session. No-op in the
 * browser build or when offline / already current.
 *
 * "Update now" downloads + verifies the new build and swaps it over the running
 * binary in place, then restarts - no manual download, no pile of duplicate
 * installers. "Get the update" stays as a manual fallback (opens the releases
 * page).
 *
 * Preview: a freshly built local binary is newer than the published release, so
 * the banner won't show for real. To review its UI/UX, press Ctrl+Shift+U to
 * force it on (works in release builds, no devtools needed); it renders with
 * the real fetched release info if any, otherwise a representative placeholder.
 */
export function UpdateBanner() {
    const [info, setInfo] = useState<UpdateInfo | null>(null);
    const [dismissed, setDismissed] = useState(false);
    const [forced, setForced] = useState(
        typeof window !== 'undefined' &&
            window.localStorage?.getItem('duckle.previewUpdateBanner') === '1',
    );
    const [progress, setProgress] = useState<SelfUpdateProgress | null>(null);
    const [updating, setUpdating] = useState(false);
    const [updateErr, setUpdateErr] = useState<string | null>(null);

    useEffect(() => {
        let cancelled = false;
        const timer = setTimeout(() => {
            checkForUpdate().then((r) => {
                if (!cancelled && r && r.update_available) setInfo(r);
            });
        }, 3000);
        // Ctrl+Shift+U force-toggles the banner so the upgrade UX can be
        // reviewed on a local build (newer than any release, so it never fires
        // for real) without needing devtools.
        const onKey = (e: KeyboardEvent) => {
            if (e.ctrlKey && e.shiftKey && (e.key === 'U' || e.key === 'u')) {
                e.preventDefault();
                setForced((v) => !v);
                setDismissed(false);
            }
        };
        window.addEventListener('keydown', onKey);
        return () => {
            cancelled = true;
            clearTimeout(timer);
            window.removeEventListener('keydown', onKey);
        };
    }, []);

    const show = forced || (info && info.update_available);
    if (!show || dismissed) return null;

    const display: UpdateInfo = info ?? {
        update_available: true,
        current_build: 'this build',
        latest_tag: 'v0.1.0-hotfix2',
        latest_date: null,
        asset_name: null,
        release_url: 'https://github.com/ducklelabs/duckle/releases',
        download_url: null,
        error: null,
    };
    // The releases page doubles as the changelog (release notes per tag).
    const changelogUrl = display.release_url ?? display.download_url ?? null;
    const tag = display.latest_tag ? ` ${display.latest_tag}` : '';
    // Only offer in-place update when the backend resolved a real download for
    // this OS (a forced/preview banner has none).
    const canSelfUpdate = Boolean(info?.download_url);

    const progressLabel = (p: SelfUpdateProgress): string => {
        switch (p.phase) {
            case 'downloading':
                return p.total
                    ? `Downloading ${Math.round((p.received / p.total) * 100)}%`
                    : `Downloading ${Math.round(p.received / 1_000_000)} MB`;
            case 'verifying':
                return 'Verifying...';
            case 'installing':
                return 'Installing...';
            case 'ready':
                return 'Restarting...';
        }
    };

    const runUpdate = async () => {
        setUpdating(true);
        setUpdateErr(null);
        setProgress({ phase: 'downloading', received: 0 });
        try {
            await selfUpdate(setProgress);
            // On success the backend restarts the app; if the promise resolves
            // first, show the restarting state.
            setProgress({ phase: 'ready' });
        } catch (e) {
            const msg = e instanceof Error ? e.message : String(e);
            setUpdateErr(msg || 'Update failed.');
            setUpdating(false);
        }
    };

    return (
        <div className="update-banner" role="status">
            <span className="update-banner-icon" aria-hidden="true">
                ⬆
            </span>
            <span className="update-banner-text">
                {updating && progress ? (
                    progressLabel(progress)
                ) : updateErr ? (
                    <>Update failed: {updateErr} Open the releases page to download manually.</>
                ) : (
                    <>
                        A newer Duckle build{tag} is available. You're on {display.current_build}.
                    </>
                )}
            </span>
            {!updating && canSelfUpdate ? (
                <button type="button" className="update-banner-cta" onClick={() => void runUpdate()}>
                    Update now
                </button>
            ) : null}
            {!updating ? (
                <button
                    type="button"
                    className="update-banner-cta update-banner-cta-secondary"
                    disabled={!changelogUrl}
                    onClick={() => {
                        if (changelogUrl) void openExternal(changelogUrl);
                    }}
                >
                    View Changelog
                </button>
            ) : null}
            {!updating ? (
                <button
                    type="button"
                    className="update-banner-dismiss"
                    aria-label="Dismiss"
                    title="Dismiss"
                    onClick={() => setDismissed(true)}
                >
                    ×
                </button>
            ) : null}
        </div>
    );
}

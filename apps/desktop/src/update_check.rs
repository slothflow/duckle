//! "Update available" check against Duckle's own GitHub releases.
//!
//! The binary is stamped at build time with `DUCKLE_BUILD_EPOCH` (see
//! build.rs). On launch the frontend calls `check_for_update`, which fetches
//! the most recent release carrying this OS/arch asset and compares that
//! asset's upload time to the running build. A newer asset (by more than a
//! margin that covers the build->upload delay) means a re-roll or a new tag is
//! out, so the frontend shows a dismissible "upgrade" banner with a download
//! link. This catches BOTH new release tags AND re-uploaded binaries on the
//! same tag (Duckle re-rolls assets onto the hotfix tag without moving it).
//!
//! Distribution is raw executables (no installer / signed-update channel), so
//! this is a check-and-prompt, not a silent self-replacing updater - the user
//! downloads the new binary from the linked release.

use serde::Serialize;
use std::time::Duration;

/// Duckle's own repository - the source of releases to check against (NOT the
/// user's workspace remote).
const REPO: &str = "ducklelabs/duckle";

/// An asset must be newer than the running build by at least this much to count
/// as an update. The release job runs ~14 min before its asset is uploaded, so
/// a freshly downloaded binary's build time is always somewhat before the asset
/// it shipped in; this margin (plus clock skew headroom) stops the app from
/// flagging itself as out of date. Real new releases for users land hours or
/// days apart, well beyond this.
const NEWER_MARGIN_SECS: i64 = 3600;

#[derive(Debug, Serialize)]
pub struct UpdateInfo {
    /// True when a strictly-newer release asset for this OS is available.
    pub update_available: bool,
    /// Human-readable build time of the running binary (or "unknown").
    pub current_build: String,
    /// Latest release tag seen (e.g. "v0.1.0-hotfix2").
    pub latest_tag: Option<String>,
    /// The latest asset's upload time (RFC3339), for display.
    pub latest_date: Option<String>,
    /// The release asset name matched for this OS/arch.
    pub asset_name: Option<String>,
    /// Browser URL of the release page (open-in-browser on the banner).
    pub release_url: Option<String>,
    /// Direct download URL of the new binary for this OS/arch.
    pub download_url: Option<String>,
    /// Non-fatal diagnostic (offline, rate-limited, unsupported platform).
    pub error: Option<String>,
}

impl UpdateInfo {
    fn base() -> Self {
        UpdateInfo {
            update_available: false,
            current_build: epoch_to_human(build_epoch()),
            latest_tag: None,
            latest_date: None,
            asset_name: None,
            release_url: None,
            download_url: None,
            error: None,
        }
    }
}

/// The release asset name for this OS + arch. Must match the `asset` names in
/// .github/workflows/release.yml.
fn os_asset() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("Duckle-windows-x64.exe"),
        ("windows", "aarch64") => Some("Duckle-windows-arm64.exe"),
        ("macos", "x86_64") => Some("Duckle-macos-x64"),
        ("macos", "aarch64") => Some("Duckle-macos-arm64"),
        ("linux", "x86_64") => Some("Duckle-linux-x64"),
        ("linux", "aarch64") => Some("Duckle-linux-arm64"),
        _ => None,
    }
}

/// Build time stamped by build.rs (0 when un-stamped, e.g. older binaries or a
/// dev build that never re-ran the script).
fn build_epoch() -> i64 {
    option_env!("DUCKLE_BUILD_EPOCH")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn epoch_to_human(epoch: i64) -> String {
    if epoch <= 0 {
        return "unknown".into();
    }
    chrono::DateTime::from_timestamp(epoch, 0)
        .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// GitHub API base. Hardcoded in real builds so the update source can never be
/// redirected by the environment (a security property). The `update-selftest`
/// feature - compiled OUT of releases - lets a local test point the check at a
/// localhost fake-release via DUCKLE_UPDATE_API_BASE.
#[cfg(not(feature = "update-selftest"))]
fn api_base() -> String {
    "https://api.github.com".to_string()
}
#[cfg(feature = "update-selftest")]
fn api_base() -> String {
    std::env::var("DUCKLE_UPDATE_API_BASE").unwrap_or_else(|_| "https://api.github.com".to_string())
}

/// Fetch the latest release that ships this OS's asset and decide whether it
/// is newer than the running build. Network / parse errors are returned as a
/// non-fatal `error` with `update_available = false` so the UI just stays
/// quiet when offline.
pub fn check() -> UpdateInfo {
    let mut info = UpdateInfo::base();
    let build = build_epoch();

    let Some(asset) = os_asset() else {
        info.error = Some("unsupported platform for auto-update".into());
        return info;
    };
    info.asset_name = Some(asset.to_string());

    // List releases (newest first) rather than /releases/latest so prereleases
    // and same-tag re-rolls are both visible.
    let url = format!("{}/repos/{REPO}/releases?per_page=10", api_base());
    let resp = duckle_duckdb_engine::tls::http_agent()
        .get(&url)
        .set("User-Agent", "duckle-app")
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(8))
        .call();
    let body: serde_json::Value = match resp {
        Ok(r) => match r.into_json() {
            Ok(v) => v,
            Err(e) => {
                info.error = Some(format!("update check parse error: {e}"));
                return info;
            }
        },
        Err(e) => {
            info.error = Some(format!("update check unavailable: {e}"));
            return info;
        }
    };

    let releases = body.as_array().cloned().unwrap_or_default();
    for rel in &releases {
        if rel.get("draft").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let Some(assets) = rel.get("assets").and_then(|v| v.as_array()) else {
            continue;
        };
        let Some(a) = assets
            .iter()
            .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(asset))
        else {
            continue;
        };
        // First release (newest) that carries our asset is the one to compare.
        let updated = a.get("updated_at").and_then(|v| v.as_str()).unwrap_or("");
        info.latest_tag = rel.get("tag_name").and_then(|v| v.as_str()).map(String::from);
        info.latest_date = Some(updated.to_string());
        // Always send users to the releases page (not a pinned per-tag URL) so
        // they land on the current/native release listing regardless of how the
        // release was rolled.
        info.release_url = Some(format!("https://github.com/{REPO}/releases"));
        info.download_url = a
            .get("browser_download_url")
            .and_then(|v| v.as_str())
            .map(String::from);
        let asset_epoch = chrono::DateTime::parse_from_rfc3339(updated)
            .map(|d| d.timestamp())
            .unwrap_or(0);
        if build > 0 && asset_epoch > 0 && asset_epoch - build > NEWER_MARGIN_SECS {
            info.update_available = true;
        }
        return info;
    }

    // No release carried our asset (e.g. nothing published yet).
    info.error.get_or_insert_with(|| "no matching release asset found".into());
    info
}

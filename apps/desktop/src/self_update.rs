//! In-app self-update for the raw single-file Duckle executable.
//!
//! Downloads the latest release binary for this OS/arch, verifies it against
//! the release `SHA256SUMS.txt`, and swaps it over the currently-running
//! executable so the user never has to manually download a new build (which is
//! what produces a pile of `Duckle (3).exe` files). The caller restarts the app
//! afterwards. A locked Windows .exe cannot be overwritten but CAN be renamed
//! aside, which is how the swap works (same trick as `write_if_changed`).

use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Progress {
    Downloading { received: u64, total: Option<u64> },
    Verifying,
    Installing,
    Ready,
}

fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent("duckle")
        .use_preconfigured_tls(duckle_duckdb_engine::tls::build_client_config())
        .build()
        .map_err(|e| format!("http client: {e}"))
}

fn download(
    client: &reqwest::blocking::Client,
    url: &str,
    mut on_progress: impl FnMut(Progress),
) -> Result<Vec<u8>, String> {
    let mut resp = client
        .get(url)
        .send()
        .map_err(|e| format!("download {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("download {url}: HTTP {}", resp.status()));
    }
    let total = resp.content_length();
    let mut buf: Vec<u8> = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut chunk = [0u8; 65536];
    loop {
        let n = resp.read(&mut chunk).map_err(|e| format!("read body: {e}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        on_progress(Progress::Downloading {
            received: buf.len() as u64,
            total,
        });
    }
    // Guard against a truncated transfer (proxy cut the stream early).
    if let Some(t) = total {
        if buf.len() as u64 != t {
            return Err(format!(
                "download truncated ({} of {} bytes); update aborted",
                buf.len(),
                t
            ));
        }
    }
    Ok(buf)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Find the hex checksum for `asset_name` in a SHA256SUMS body. Each line is
/// `<64-hex><whitespace>[*]<filename>` (the `*` is sha256sum's binary marker).
fn checksum_for(sums: &str, asset_name: &str) -> Option<String> {
    for line in sums.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.splitn(2, char::is_whitespace);
        let hex = it.next()?.trim();
        let name = it.next()?.trim().trim_start_matches('*').trim();
        if name == asset_name && hex.len() == 64 {
            return Some(hex.to_ascii_lowercase());
        }
    }
    None
}

/// Replace the running executable with `new_bytes`, returning its path. On a
/// locked Windows .exe a direct overwrite fails, so the live exe is renamed
/// aside (`<exe>.old-<pid>`, swept on a later launch) and the new one moved in;
/// the original is restored on failure.
fn swap_current_exe(new_bytes: &[u8]) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let dir = exe.parent().ok_or("running exe has no parent directory")?;
    let tmp = dir.join(format!(".duckle-update-{}.tmp", std::process::id()));
    std::fs::write(&tmp, new_bytes).map_err(|e| format!("write {}: {}", tmp.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    // Unix lets us rename over a running binary directly.
    if std::fs::rename(&tmp, &exe).is_ok() {
        return Ok(exe);
    }
    // Windows / locked: move the live exe aside, then the new one into place.
    let aside = exe.with_extension(format!("old-{}", std::process::id()));
    std::fs::rename(&exe, &aside).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not move the current exe aside: {e}")
    })?;
    if let Err(e) = std::fs::rename(&tmp, &exe) {
        let _ = std::fs::rename(&aside, &exe); // restore
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("could not install the new exe: {e}"));
    }
    let _ = std::fs::remove_file(&aside); // no-op while locked; swept later
    Ok(exe)
}

/// Best-effort removal of leftover `.old-*` and `.duckle-update-*.tmp` files
/// next to the exe (the displaced binary unlocks once its process exits).
pub fn sweep_leftovers(exe_dir: &Path) {
    let Ok(rd) = std::fs::read_dir(exe_dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let stale = name.starts_with(".duckle-update-")
            || p.extension()
                .and_then(|s| s.to_str())
                .map(|e| e.starts_with("old-"))
                .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// Download + verify + swap the latest release over the running exe.
pub fn run(on_progress: impl FnMut(Progress)) -> Result<(), String> {
    let info = crate::update_check::check();
    if !info.update_available {
        return Err(info
            .error
            .unwrap_or_else(|| "Duckle is already up to date.".into()));
    }
    let download_url = info
        .download_url
        .ok_or("No download is available for this platform.")?;
    let asset_name = info
        .asset_name
        .ok_or("No release asset matches this platform.")?;
    fetch_verify_swap(&download_url, &asset_name, on_progress)
}

/// Download `download_url`, verify the bytes against the `SHA256SUMS.txt` in the
/// same release directory, then swap them over the running exe. Split out from
/// `run` so a feature-gated self-test can exercise the real download + verify +
/// swap against a local fake-release without touching GitHub.
pub(crate) fn fetch_verify_swap(
    download_url: &str,
    asset_name: &str,
    mut on_progress: impl FnMut(Progress),
) -> Result<(), String> {
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        sweep_leftovers(&dir);
    }

    let client = http_client()?;

    // Verify against the release's SHA256SUMS (same release directory). Fail
    // closed: if the release predates checksums we refuse rather than install
    // an unverified binary.
    let sums_url = download_url
        .rsplit_once('/')
        .map(|(base, _)| format!("{base}/SHA256SUMS.txt"))
        .ok_or("malformed download URL")?;
    let sums = client
        .get(&sums_url)
        .send()
        .ok()
        .filter(|r| r.status().is_success())
        .and_then(|r| r.text().ok())
        .ok_or("This release ships no SHA256SUMS, so the update can't be verified. Use Get the update to download it manually.")?;
    let want = checksum_for(&sums, asset_name)
        .ok_or_else(|| format!("no checksum for {asset_name} in SHA256SUMS"))?;

    let bytes = download(&client, download_url, &mut on_progress)?;
    if (bytes.len() as u64) < 1_000_000 {
        return Err(format!(
            "the downloaded update is implausibly small ({} bytes); aborting",
            bytes.len()
        ));
    }

    on_progress(Progress::Verifying);
    let got = sha256_hex(&bytes);
    if got != want {
        return Err(format!(
            "checksum mismatch for {asset_name} (expected {want}, got {got}); update aborted"
        ));
    }

    on_progress(Progress::Installing);
    swap_current_exe(&bytes)?;
    on_progress(Progress::Ready);
    Ok(())
}

/// Feature-gated headless self-test (NOT compiled into releases). Drives the
/// real `fetch_verify_swap` against the URL/asset in the environment so the
/// download + SHA256SUMS verify + locked-exe swap can be verified end-to-end
/// against a local fake-release. Swaps THIS process's exe, so run it on a
/// throwaway copy. Exits the process with the result.
#[cfg(feature = "update-selftest")]
pub fn selftest_main() -> ! {
    let url = std::env::var("DUCKLE_SELFTEST_DOWNLOAD_URL")
        .expect("set DUCKLE_SELFTEST_DOWNLOAD_URL");
    let asset = std::env::var("DUCKLE_SELFTEST_ASSET").expect("set DUCKLE_SELFTEST_ASSET");
    match fetch_verify_swap(&url, &asset, |p| eprintln!("progress: {p:?}")) {
        Ok(()) => {
            println!("SELFTEST_OK");
            std::process::exit(0);
        }
        Err(e) => {
            println!("SELFTEST_ERR: {e}");
            std::process::exit(3);
        }
    }
}

/// Feature-gated headless drive of the FULL `run()` (check -> download -> verify
/// -> swap), i.e. exactly what the "Update now" button's command does minus the
/// restart. Point check at a local fake-release with DUCKLE_UPDATE_API_BASE.
/// Swaps THIS process's exe, so run it on a throwaway copy.
#[cfg(feature = "update-selftest")]
pub fn selftest_run_main() -> ! {
    match run(|p| eprintln!("progress: {p:?}")) {
        Ok(()) => {
            println!("SELFTEST_RUN_OK");
            std::process::exit(0);
        }
        Err(e) => {
            println!("SELFTEST_RUN_ERR: {e}");
            std::process::exit(4);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sha256sums_lines() {
        let a = "1".repeat(64);
        let b = "2".repeat(64);
        let sums = format!(
            "garbage line\n{a}  Duckle-windows-x64.exe\n{b} *Duckle-linux-x64\n# comment\n"
        );
        assert_eq!(
            checksum_for(&sums, "Duckle-windows-x64.exe").as_deref(),
            Some(a.as_str())
        );
        // tolerates the sha256sum '*' binary marker
        assert_eq!(checksum_for(&sums, "Duckle-linux-x64").as_deref(), Some(b.as_str()));
        assert_eq!(checksum_for(&sums, "Duckle-macos-arm64"), None);
    }

    #[test]
    fn sha256_of_empty_is_known() {
        assert!(sha256_hex(b"").starts_with("e3b0c44298fc1c14"));
    }
}

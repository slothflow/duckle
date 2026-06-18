//! Shared TLS trust configuration for Duckle's HTTP clients.
//!
//! ureq (REST / cloud-API connectors) and reqwest (the desktop engine
//! downloads) both default to the bundled Mozilla root set (webpki-roots),
//! which ignores the operating-system trust store. Behind a TLS-inspecting
//! corporate proxy (Zscaler, Netskope, ...) that re-signs every certificate
//! with its own CA, that CA lives only in the OS store, so the handshake
//! fails with `UnknownIssuer`.
//!
//! We build ONE rustls client config whose root store is the union of:
//!   1. the bundled Mozilla roots (identical to the previous default), plus
//!   2. the OS native trust store (adds the corporate inspection CA), plus
//!   3. an optional explicit PEM bundle pointed at by `DUCKLE_CA_CERT`.
//!
//! It is a strict superset of the old trust set, so non-corporate users see
//! no behavioural change: everything that validated before still validates.
//! The OS store and env bundle are best-effort - a missing or unreadable
//! source just leaves the bundled roots in place.

use std::sync::{Arc, OnceLock};

/// Assemble the union root store: bundled Mozilla roots, the OS native store,
/// and an optional `DUCKLE_CA_CERT` PEM bundle.
fn build_root_store() -> rustls::RootCertStore {
    let mut roots = rustls::RootCertStore::empty();

    // 1. Bundled Mozilla roots - the prior default on every platform, so no
    //    machine loses trust it had before.
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    // 2. OS trust store - adds enterprise / proxy-inspection CAs. Best effort.
    match rustls_native_certs::load_native_certs() {
        Ok(certs) => {
            let _ = roots.add_parsable_certificates(certs);
        }
        Err(e) => {
            eprintln!("duckle: could not read OS certificate store: {e}");
        }
    }

    // 3. Optional explicit PEM bundle, for split-tunnel setups or where the
    //    proxy CA is handed out as a file rather than installed in the store.
    if let Ok(path) = std::env::var("DUCKLE_CA_CERT") {
        if !path.is_empty() {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let mut rd = std::io::BufReader::new(&bytes[..]);
                    let extra: Vec<_> = rustls_pemfile::certs(&mut rd)
                        .filter_map(Result::ok)
                        .collect();
                    let _ = roots.add_parsable_certificates(extra);
                }
                Err(e) => eprintln!("duckle: DUCKLE_CA_CERT unreadable ({path}): {e}"),
            }
        }
    }

    roots
}

/// Build a fresh rustls client config trusting bundled + OS-native (+ optional
/// `DUCKLE_CA_CERT`) roots. reqwest consumes this via `use_preconfigured_tls`.
pub fn build_client_config() -> rustls::ClientConfig {
    // Match ureq's provider (ring) so we add no second crypto backend and
    // avoid depending on a process-wide default provider being installed.
    rustls::ClientConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
        .with_safe_default_protocol_versions()
        .expect("ring provider supports TLS 1.2 + 1.3")
        .with_root_certificates(build_root_store())
        .with_no_client_auth()
}

/// Read an HTTP/HTTPS proxy URL from the environment. Prefers Duckle's own var
/// (so a user can point Duckle at a proxy without changing global env), then the
/// conventional HTTPS_PROXY / ALL_PROXY / HTTP_PROXY (any case). Unlike reqwest,
/// ureq does NOT pick these up on its own, so behind a corporate proxy every
/// REST / cloud-API call would connect directly and time out (os error 10060,
/// issue #80). The URL may include credentials, e.g. http://user:pass@host:8080.
pub fn proxy_url_from_env() -> Option<String> {
    for key in [
        "DUCKLE_HTTPS_PROXY",
        "DUCKLE_PROXY",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// A process-wide ureq agent using the merged trust config above. The agent
/// is internally reference-counted, so cloning it per request is cheap; we
/// build it once and hand out clones.
pub fn http_agent() -> ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT
        .get_or_init(|| {
            let mut builder =
                ureq::AgentBuilder::new().tls_config(Arc::new(build_client_config()));
            // Honor a configured proxy so REST / cloud connectors work behind a
            // corporate proxy instead of timing out on a direct connect (#80).
            if let Some(url) = proxy_url_from_env() {
                match ureq::Proxy::new(&url) {
                    Ok(p) => builder = builder.proxy(p),
                    Err(e) => eprintln!("duckle: ignoring invalid proxy '{url}': {e}"),
                }
            }
            builder.build()
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_store_is_a_superset_of_bundled_roots() {
        // The merged store must contain at least every bundled Mozilla root,
        // so non-corporate users never lose trust they had before.
        let bundled = webpki_roots::TLS_SERVER_ROOTS.len();
        let merged = build_root_store().roots.len();
        assert!(
            merged >= bundled,
            "merged roots ({merged}) dropped below bundled roots ({bundled})"
        );
    }

    #[test]
    fn agent_builds() {
        let _ = http_agent();
    }

    #[test]
    fn proxy_env_prefers_duckle_var() {
        // The Duckle-specific var wins over the conventional ones so a user can
        // point Duckle at a proxy without changing global env. (Best-effort
        // env-mutation test; the value is harmless - it is never connected to.)
        std::env::set_var("DUCKLE_HTTPS_PROXY", "http://proxy.example:8080");
        assert_eq!(
            proxy_url_from_env().as_deref(),
            Some("http://proxy.example:8080")
        );
        std::env::remove_var("DUCKLE_HTTPS_PROXY");
    }
}

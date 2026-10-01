//! Apple system-trust verification for iOS WebSockets and their TLS proxy tunnels.
//! iOS has no Unix root-certificate directory. Keep the platform trust policy and
//! layer explicitly configured CA certificates into the existing verifier.

use codex_utils_rustls_provider::ensure_rustls_crypto_provider;
use rustls::ClientConfig;
use rustls::pki_types::CertificateDer;
use rustls_platform_verifier::Verifier;
use std::sync::Arc;

pub(crate) fn build_ios_platform_tls_config(
    extra_roots: Vec<CertificateDer<'static>>,
) -> Result<Arc<ClientConfig>, rustls::Error> {
    ensure_rustls_crypto_provider();
    let builder = ClientConfig::builder();
    let verifier = Verifier::new_with_extra_roots(extra_roots, builder.crypto_provider().clone())?;
    Ok(Arc::new(
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth(),
    ))
}

#[cfg(test)]
#[path = "ios_tls_tests.rs"]
mod tests;

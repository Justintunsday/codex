//! Drive real TLS handshakes without ports, environment changes or trust-store writes.
use super::build_ios_platform_tls_config;
use rustls::ClientConnection;
use rustls::ServerConnection;
use rustls::pki_types::CertificateDer;
use std::sync::Arc;

fn handshake(
    roots: Vec<CertificateDer<'static>>,
    host: &'static str,
    key: rcgen::CertifiedKey<rcgen::KeyPair>,
) -> Result<(), rustls::Error> {
    let mut client = ClientConnection::new(
        build_ios_platform_tls_config(roots)?,
        host.try_into().expect("server name"),
    )?;
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![key.cert.der().clone()], key.signing_key.into())?;
    let mut server = ServerConnection::new(Arc::new(server_config))?;
    for _ in 0..10 {
        let mut records = Vec::new();
        client.write_tls(&mut records).expect("client records");
        server
            .read_tls(&mut records.as_slice())
            .expect("read client records");
        server.process_new_packets()?;
        records.clear();
        server.write_tls(&mut records).expect("server records");
        client
            .read_tls(&mut records.as_slice())
            .expect("read server records");
        client.process_new_packets()?;
        if !client.is_handshaking() && !server.is_handshaking() {
            return Ok(());
        }
    }
    panic!("TLS handshake did not complete");
}

#[test]
fn ios_tls_rejects_an_untrusted_server() {
    let key = rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("certificate");
    assert!(matches!(
        handshake(Vec::new(), "localhost", key),
        Err(rustls::Error::InvalidCertificate(_))
    ));
}

#[test]
fn ios_tls_accepts_an_explicit_anchor_with_matching_hostname() {
    let key = rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("certificate");
    handshake(vec![key.cert.der().clone()], "localhost", key).expect("trusted TLS handshake");
}

#[test]
fn ios_tls_rejects_a_hostname_mismatch_even_with_an_explicit_anchor() {
    let key = rcgen::generate_simple_self_signed(vec!["localhost".into()]).expect("certificate");
    let result = handshake(vec![key.cert.der().clone()], "wrong.example", key);
    assert!(
        matches!(result, Err(rustls::Error::InvalidCertificate(_))),
        "{result:?}"
    );
}

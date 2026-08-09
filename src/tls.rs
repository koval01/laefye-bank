//! Самоподписанный серт. Без HTTPS браузер passkey не даст.

use std::fs;
use std::path::Path;

use axum_server::tls_rustls::RustlsConfig;
use rcgen::{CertificateParams, KeyPair, SanType};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// Loads `cert.pem`/`key.pem` or generates a localhost self-signed pair.
pub async fn load_or_generate_rustls_config(
    cert_dir: &str,
) -> Result<RustlsConfig, Box<dyn std::error::Error + Send + Sync>> {
    let dir = Path::new(cert_dir);
    fs::create_dir_all(dir)?;

    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");

    if !(cert_path.exists() && key_path.exists()) {
        tracing::info!("генерируем самоподписанный сертификат в {cert_dir}/");
        generate_self_signed(&cert_path, &key_path)?;
    }

    Ok(RustlsConfig::from_pem_file(&cert_path, &key_path).await?)
}

fn generate_self_signed(
    cert_path: &Path,
    key_path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut params = CertificateParams::new(vec!["localhost".into()])?;
    params.subject_alt_names = vec![
        SanType::DnsName("localhost".try_into()?),
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
    ];

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    fs::write(cert_path, cert.pem())?;
    fs::write(key_path, key_pair.serialize_pem())?;

    let _ = CertificateDer::from(cert.der().to_vec());
    let _ = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));

    Ok(())
}

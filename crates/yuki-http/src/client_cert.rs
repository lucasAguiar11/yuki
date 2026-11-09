//! Client certificate (mTLS) identity and the TLS backend that presents it.
//!
//! Servers like IIS (and many Brazilian government APIs) ask for the
//! certificate only after the request arrives, by renegotiating; rustls never
//! renegotiates, so those handshakes fail. macOS's TLS (native-tls) does, but
//! only takes PKCS#8 keys: PKCS#1/SEC1 keys stay on rustls.

use yuki_core::YukiError;

/// The identity and whether it needs the native backend.
pub(crate) fn identity(pem: &[u8]) -> Result<(reqwest::Identity, bool), YukiError> {
    let built = match pkcs8_parts(&String::from_utf8_lossy(pem)) {
        Some((certs, key)) => {
            reqwest::Identity::from_pkcs8_pem(certs.as_bytes(), key.as_bytes()).map(|i| (i, true))
        }
        None => reqwest::Identity::from_pem(pem).map(|i| (i, false)),
    };
    built.map_err(|e| {
        let root = std::iter::successors(Some(&e as &dyn std::error::Error), |s| s.source());
        YukiError::Http(format!("client certificate: {}", root.last().unwrap_or(&e)))
    })
}

/// Certificates and PKCS#8 key as clean PEM (text outside the blocks, like
/// openssl's `Bag Attributes`, dropped: macOS rejects it). `None` without
/// both, or with a PKCS#1/SEC1 key.
fn pkcs8_parts(text: &str) -> Option<(String, String)> {
    let (mut certs, mut key) = (String::new(), None);
    for block in text.split("-----BEGIN ").skip(1) {
        let Some((label, rest)) = block.split_once("-----") else {
            continue;
        };
        let end = format!("-----END {label}-----");
        let Some(body) = rest.find(&end).map(|at| &rest[..at]) else {
            continue;
        };
        let pem = format!("-----BEGIN {label}-----{body}{end}\n");
        match label {
            "CERTIFICATE" => certs.push_str(&pem),
            "PRIVATE KEY" => key = Some(pem),
            "RSA PRIVATE KEY" | "EC PRIVATE KEY" => return None,
            _ => {}
        }
    }
    Some(certs).filter(|c| !c.is_empty()).zip(key)
}

#[cfg(test)]
mod tests {
    use super::pkcs8_parts;

    #[test]
    fn splits_chain_and_pkcs8_key_dropping_bag_attributes() {
        let pem = "Bag Attributes\n    localKeyID: 01\n-----BEGIN CERTIFICATE-----\nA\n-----END CERTIFICATE-----\n\
                   subject=CN=ca\n-----BEGIN CERTIFICATE-----\nB\n-----END CERTIFICATE-----\n\
                   Bag Attributes\n-----BEGIN PRIVATE KEY-----\nK\n-----END PRIVATE KEY-----\n";
        let (certs, key) = pkcs8_parts(pem).unwrap();
        assert_eq!(
            certs,
            "-----BEGIN CERTIFICATE-----\nA\n-----END CERTIFICATE-----\n\
             -----BEGIN CERTIFICATE-----\nB\n-----END CERTIFICATE-----\n"
        );
        assert_eq!(
            key,
            "-----BEGIN PRIVATE KEY-----\nK\n-----END PRIVATE KEY-----\n"
        );
    }

    #[test]
    fn other_key_formats_and_missing_parts_stay_on_rustls() {
        let cert = "-----BEGIN CERTIFICATE-----\nA\n-----END CERTIFICATE-----\n";
        let rsa = "-----BEGIN RSA PRIVATE KEY-----\nK\n-----END RSA PRIVATE KEY-----\n";
        let pkcs8 = "-----BEGIN PRIVATE KEY-----\nK\n-----END PRIVATE KEY-----\n";
        assert!(pkcs8_parts(&format!("{cert}{rsa}")).is_none());
        assert!(pkcs8_parts(cert).is_none());
        assert!(pkcs8_parts(pkcs8).is_none());
    }
}

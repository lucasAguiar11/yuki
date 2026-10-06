//! Builds a tonic `Channel` from a `host:port` target. `tls` selects h2 over
//! TLS (with system roots) vs plaintext h2c.

use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use yuki_core::YukiError;

pub async fn build(target: &str, tls: bool) -> Result<Channel, YukiError> {
    let scheme = if tls { "https" } else { "http" };
    let uri = format!("{scheme}://{target}");
    let mut endpoint =
        Endpoint::from_shared(uri).map_err(|e| YukiError::Grpc(format!("invalid target: {e}")))?;
    if tls {
        endpoint = endpoint
            .tls_config(ClientTlsConfig::new().with_native_roots())
            .map_err(|e| YukiError::Grpc(format!("tls: {e}")))?;
    }
    endpoint
        .connect()
        .await
        .map_err(|e| YukiError::Grpc(format!("connect to {target}: {e}")))
}

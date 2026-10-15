//! The assistant's HTTPS transport: one streaming POST, certificates from the operating system.
//!
//! `solidcraft_assistant` decides where it may talk (https, or plain http to the local machine
//! or a host the person allowed); this only carries the request. No redirects are followed, so a
//! key in a header never travels to a host the person did not configure.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use solidcraft_assistant::net::{Cancel, HttpError, HttpRequest, MAX_ERROR_BODY};

/// Response bytes accepted from one request.
const MAX_RESPONSE: u64 = 64 << 20;

/// POST `req` and hand the response body to `sink` as it arrives; `sink` returns `false` to stop.
pub fn stream(req: &HttpRequest, cancel: &Cancel, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<(), HttpError> {
    if cancel.is_cancelled() {
        return Err(HttpError::Cancelled);
    }
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(false)
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_connect(Some(Duration::from_secs(10)))
        // A model may think a long time before the first token.
        .timeout_recv_response(Some(Duration::from_secs(180)))
        .timeout_recv_body(Some(Duration::from_secs(20 * 60)))
        .user_agent("SolidCraft-assistant")
        .build()
        .into();
    let mut builder = agent.post(&req.url);
    for (name, value) in &req.headers {
        builder = builder.header(name.as_str(), value.expose());
    }
    let mut response = builder.send(&req.body[..]).map_err(|e| HttpError::Transport(e.to_string()))?;
    let code = response.status().as_u16();
    if !(200..300).contains(&code) {
        let retry_after = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
        let body = response.body_mut().with_config().limit(MAX_ERROR_BODY as u64).read_to_string().unwrap_or_default();
        return Err(HttpError::Status { code, retry_after, body });
    }
    let mut reader = response.body_mut().with_config().limit(MAX_RESPONSE).reader();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        if cancel.is_cancelled() {
            return Err(HttpError::Cancelled);
        }
        let n = reader.read(&mut buf).map_err(|e| HttpError::Transport(e.to_string()))?;
        if n == 0 {
            return Ok(());
        }
        if !sink(buf.get(..n).unwrap_or_default()) {
            return Ok(());
        }
    }
}

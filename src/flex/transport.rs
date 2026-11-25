//! HTTP transport abstraction.
//!
//! The Flex client depends on this trait rather than a concrete HTTP library, so the
//! send/poll/parse logic can be tested without network access (see `tests/flex_flow.rs`).

use std::time::Duration;

// Report generation happens between HTTP requests. This budget covers each exchange,
// including download time, and a peer sending partial data cannot reset it.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// An error from the underlying HTTP transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError(pub String);

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TransportError {}

/// Performs an HTTP GET, returning the response body as text.
///
/// `params` are query-string parameters; implementations are responsible for URL-encoding them.
#[allow(async_fn_in_trait)]
pub trait HttpGet {
    async fn get(&self, url: &str, params: &[(&str, &str)]) -> Result<String, TransportError>;
}

/// Allow a shared reference to a transport to be used as a transport, so a single transport can
/// be shared (e.g. handed to a client while still inspectable in tests).
impl<T: HttpGet> HttpGet for &T {
    async fn get(&self, url: &str, params: &[(&str, &str)]) -> Result<String, TransportError> {
        (**self).get(url, params).await
    }
}

/// Production transport backed by `reqwest` with rustls TLS.
#[derive(Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    /// Build a transport with a fixed user agent and a 60-second deadline per request.
    pub fn new() -> Result<Self, TransportError> {
        Self::with_timeout(REQUEST_TIMEOUT)
    }

    fn with_timeout(timeout: Duration) -> Result<Self, TransportError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("ibkr-flex-mcp/", env!("CARGO_PKG_VERSION")))
            .timeout(timeout)
            .build()
            .map_err(|err| http_error("building HTTP client", err))?;
        Ok(Self { client })
    }
}

impl HttpGet for ReqwestTransport {
    async fn get(&self, url: &str, params: &[(&str, &str)]) -> Result<String, TransportError> {
        let response = self
            .client
            .get(url)
            .query(params)
            .send()
            .await
            .map_err(|err| http_error("request failed", err))?
            .error_for_status()
            .map_err(|err| http_error("HTTP status error", err))?;
        response
            .text()
            .await
            .map_err(|err| http_error("reading response body", err))
    }
}

fn http_error(context: &str, error: reqwest::Error) -> TransportError {
    // Keep remote status text and request URLs with their Flex token out of errors.
    let detail = match error.status() {
        Some(status) => status.as_u16().to_string(),
        None if error.is_timeout() => "operation timed out".into(),
        None => error.without_url().to_string(),
    };
    TransportError(format!("{context}: {detail}"))
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    #[tokio::test]
    async fn deadline_includes_a_stalled_response_body_and_redacts_token() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/GetStatement", listener.local_addr().unwrap());
        let server = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0; 1024];
            assert!(stream.read(&mut buf).await.unwrap() > 0);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort")
                .await
                .unwrap();
            // Keep the incomplete response open until the client gives up.
            std::future::pending::<()>().await;
        };

        let transport = ReqwestTransport::with_timeout(Duration::from_millis(500)).unwrap();
        let err = tokio::select! {
            result = transport.get(&url, &[("t", "SECRET_TOKEN")]) => result.unwrap_err(),
            () = server => unreachable!(),
            () = tokio::time::sleep(Duration::from_secs(5)) => panic!("request did not time out"),
        };

        let message = err.to_string();
        assert!(message.contains("reading response body"), "{message}");
        assert!(message.contains("timed out"), "{message}");
        assert!(!message.contains("SECRET_TOKEN"), "{message}");
    }
}

//! HTTP transport abstraction.
//!
//! The Flex client depends on this trait rather than a concrete HTTP library, so the
//! send/poll/parse logic can be tested without network access (see `tests/flex_flow.rs`).
//! The real `reqwest`-backed implementation is added in the next slice.

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
    /// Build a transport with a fixed user agent and the default rustls TLS stack.
    pub fn new() -> Result<Self, TransportError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("ibkr-flex-mcp/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| TransportError(format!("building HTTP client: {e}")))?;
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
            .map_err(|e| TransportError(format!("request failed: {e}")))?
            .error_for_status()
            .map_err(|e| TransportError(format!("HTTP status error: {e}")))?;
        response
            .text()
            .await
            .map_err(|e| TransportError(format!("reading response body: {e}")))
    }
}

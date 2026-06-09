//! The functional core: a transport-agnostic Interactive Brokers Flex Web Service v3 client.
//!
//! Protocol (read-only; the Flex Web Service cannot place orders):
//!   1. SendRequest  -> a reference code     (host: ndcdyn.interactivebrokers.com)
//!   2. GetStatement -> poll until ready     (host: gdcdyn.interactivebrokers.com)

mod parse;
pub mod transport;

pub use parse::FlexError;

use parse::{classify_get_response, parse_send_response, GetOutcome};
use std::time::Duration;
use transport::HttpGet;

/// SendRequest endpoint (generates a statement, returns a reference code).
pub const SEND_URL: &str =
    "https://ndcdyn.interactivebrokers.com/AccountManagement/FlexWebService/SendRequest";
/// GetStatement endpoint (retrieves the generated statement by reference code).
pub const GET_URL: &str =
    "https://gdcdyn.interactivebrokers.com/AccountManagement/FlexWebService/GetStatement";
/// Flex Web Service protocol version.
pub const API_VERSION: &str = "3";

/// A fetched Flex statement: the raw report XML plus the identifiers used to retrieve it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlexStatement {
    pub query_id: String,
    pub reference_code: String,
    pub raw_xml: String,
}

/// Runs the Flex Web Service two-step protocol over an injected [`HttpGet`] transport.
pub struct FlexClient<T: HttpGet> {
    transport: T,
    send_url: String,
    get_url: String,
    version: String,
    max_retries: u32,
    retry_delay: Duration,
}

impl<T: HttpGet> FlexClient<T> {
    /// Create a client against the production IBKR endpoints with sensible polling defaults.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            send_url: SEND_URL.to_string(),
            get_url: GET_URL.to_string(),
            version: API_VERSION.to_string(),
            max_retries: 8,
            retry_delay: Duration::from_secs(5),
        }
    }

    /// Override the endpoints (used by tests to point at a fake transport's expectations).
    pub fn with_urls(mut self, send_url: impl Into<String>, get_url: impl Into<String>) -> Self {
        self.send_url = send_url.into();
        self.get_url = get_url.into();
        self
    }

    /// Override the polling policy.
    pub fn with_retry(mut self, max_retries: u32, retry_delay: Duration) -> Self {
        self.max_retries = max_retries;
        self.retry_delay = retry_delay;
        self
    }

    /// Fetch a statement: send the request, then poll until the statement is ready.
    pub async fn fetch_statement(
        &self,
        token: &str,
        query_id: &str,
    ) -> Result<FlexStatement, FlexError> {
        let reference_code = self.send_request(token, query_id).await?;

        for _ in 0..self.max_retries {
            tokio::time::sleep(self.retry_delay).await;
            match self.get_statement(token, &reference_code).await? {
                GetOutcome::Statement(raw_xml) => {
                    return Ok(FlexStatement {
                        query_id: query_id.to_string(),
                        reference_code,
                        raw_xml,
                    })
                }
                GetOutcome::InProgress => continue,
            }
        }

        Err(FlexError::NotReady(self.max_retries))
    }

    async fn send_request(&self, token: &str, query_id: &str) -> Result<String, FlexError> {
        let body = self
            .transport
            .get(
                &self.send_url,
                &[("t", token), ("q", query_id), ("v", &self.version)],
            )
            .await
            .map_err(|e| FlexError::Transport(e.to_string()))?;
        parse_send_response(&body)
    }

    async fn get_statement(
        &self,
        token: &str,
        reference_code: &str,
    ) -> Result<GetOutcome, FlexError> {
        let body = self
            .transport
            .get(
                &self.get_url,
                &[("t", token), ("q", reference_code), ("v", &self.version)],
            )
            .await
            .map_err(|e| FlexError::Transport(e.to_string()))?;
        classify_get_response(&body)
    }
}

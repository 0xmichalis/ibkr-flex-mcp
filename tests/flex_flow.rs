//! End-to-end flow tests for the Flex client using an injected fake transport — no network.

use ibkr_flex_mcp::flex::transport::{HttpGet, TransportError};
use ibkr_flex_mcp::flex::{FlexClient, FlexError, GET_URL, SEND_URL};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

/// A recorded request: the URL and its query parameters.
type RecordedCall = (String, Vec<(String, String)>);

/// Records every request and replays a queued sequence of response bodies.
struct FakeTransport {
    responses: Mutex<VecDeque<String>>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl FakeTransport {
    fn new(responses: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().map(String::from).collect()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl HttpGet for FakeTransport {
    async fn get(&self, url: &str, params: &[(&str, &str)]) -> Result<String, TransportError> {
        self.calls.lock().unwrap().push((
            url.to_string(),
            params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ));
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| TransportError("fake: no more responses queued".into()))
    }
}

const SEND_OK: &str = r#"<FlexStatementResponse><Status>Success</Status><ReferenceCode>REF123</ReferenceCode></FlexStatementResponse>"#;
const IN_PROGRESS: &str = r#"<FlexStatementResponse><Status>Warn</Status><ErrorCode>1019</ErrorCode><ErrorMessage>Statement generation in progress.</ErrorMessage></FlexStatementResponse>"#;
const STATEMENT: &str = r#"<FlexQueryResponse queryName="Activity" type="AF"><FlexStatements count="1"><FlexStatement accountId="U1234567"><OpenPositions/></FlexStatement></FlexStatements></FlexQueryResponse>"#;

#[tokio::test]
async fn fetches_statement_after_polling_through_in_progress() {
    let transport = FakeTransport::new([SEND_OK, IN_PROGRESS, STATEMENT]);
    let client = FlexClient::new(transport).with_retry(5, Duration::ZERO);

    let statement = client
        .fetch_statement("my-token", "Q789")
        .await
        .expect("should fetch statement");

    assert_eq!(statement.query_id, "Q789");
    assert_eq!(statement.reference_code, "REF123");
    assert_eq!(statement.raw_xml, STATEMENT);
}

#[tokio::test]
async fn sends_flex_protocol_params_to_correct_endpoints() {
    // Share the fake by reference so the client can use it while we inspect it afterwards.
    let fake = FakeTransport::new([SEND_OK, STATEMENT]);
    let client = FlexClient::new(&fake).with_retry(5, Duration::ZERO);
    client.fetch_statement("my-token", "Q789").await.unwrap();

    let calls = fake.calls();
    assert_eq!(calls.len(), 2);

    let (send_url, send_params) = &calls[0];
    assert_eq!(send_url, SEND_URL);
    assert!(send_params.contains(&("t".to_string(), "my-token".to_string())));
    assert!(send_params.contains(&("q".to_string(), "Q789".to_string())));
    assert!(send_params.contains(&("v".to_string(), "3".to_string())));

    let (get_url, get_params) = &calls[1];
    assert_eq!(get_url, GET_URL);
    assert!(get_params.contains(&("t".to_string(), "my-token".to_string())));
    assert!(get_params.contains(&("q".to_string(), "REF123".to_string())));
}

#[tokio::test]
async fn gives_up_after_max_retries_when_never_ready() {
    let transport = FakeTransport::new([SEND_OK, IN_PROGRESS, IN_PROGRESS, IN_PROGRESS]);
    let client = FlexClient::new(transport).with_retry(3, Duration::ZERO);

    let err = client
        .fetch_statement("my-token", "Q789")
        .await
        .expect_err("should not be ready");

    assert_eq!(err, FlexError::NotReady(3));
}

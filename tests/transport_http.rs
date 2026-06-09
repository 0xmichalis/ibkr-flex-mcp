//! Tests for the real reqwest-backed transport, against a local mock HTTP server.

use ibkr_flex_mcp::flex::transport::{HttpGet, ReqwestTransport};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn get_sends_query_params_and_returns_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/SendRequest"))
        .and(query_param("t", "my-token"))
        .and(query_param("q", "Q789"))
        .and(query_param("v", "3"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<FlexStatementResponse/>"))
        .mount(&server)
        .await;

    let transport = ReqwestTransport::new().unwrap();
    let url = format!("{}/SendRequest", server.uri());
    let body = transport
        .get(&url, &[("t", "my-token"), ("q", "Q789"), ("v", "3")])
        .await
        .unwrap();

    assert_eq!(body, "<FlexStatementResponse/>");
}

#[tokio::test]
async fn non_success_status_is_a_transport_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let transport = ReqwestTransport::new().unwrap();
    let err = transport.get(&server.uri(), &[]).await.unwrap_err();

    assert!(err.to_string().contains("500"), "error was: {err}");
}

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

#[tokio::test]
async fn status_error_redacts_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let transport = ReqwestTransport::new().unwrap();
    let url = format!("{}/SendRequest", server.uri());
    let err = transport
        .get(&url, &[("t", "SECRET_TOKEN"), ("q", "Q1"), ("v", "3")])
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(!msg.contains("SECRET_TOKEN"), "error leaked token: {msg}");
    assert!(msg.contains("500"), "error missing status: {msg}");
}

#[tokio::test]
async fn send_error_redacts_token() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let transport = ReqwestTransport::new().unwrap();
    let url = format!("http://{}/SendRequest", addr);
    let err = transport
        .get(&url, &[("t", "SECRET_TOKEN"), ("q", "Q1"), ("v", "3")])
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(!msg.contains("SECRET_TOKEN"), "error leaked token: {msg}");
    assert!(msg.contains("request failed"), "error missing kind: {msg}");
}

#[tokio::test]
async fn body_error_redacts_token() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort")
            .unwrap();
    });

    let transport = ReqwestTransport::new().unwrap();
    let url = format!("http://{}/SendRequest", addr);
    let err = transport
        .get(&url, &[("t", "SECRET_TOKEN"), ("q", "Q1"), ("v", "3")])
        .await
        .unwrap_err();

    let msg = err.to_string();
    assert!(!msg.contains("SECRET_TOKEN"), "error leaked token: {msg}");
    assert!(
        msg.contains("reading response body"),
        "error missing kind: {msg}"
    );
}

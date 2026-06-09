//! Live integration test against the real IBKR Flex Web Service.
//!
//! This exercises the full core path — `FlexClient::fetch_statement` over the real
//! `ReqwestTransport` (SendRequest -> poll GetStatement -> parse), then `parse_positions`.
//!
//! It is SKIPPED (a no-op pass) unless both `IBKR_FLEX_TOKEN` and `IBKR_FLEX_QUERY_ID`
//! are set. Provide them via the process environment or a `.env` file in the repo root
//! (gitignored), then run:
//!
//!     cargo test --test live_flex -- --nocapture
//!
//! Note: a real fetch can take tens of seconds (IBKR generates the statement and the
//! client polls between attempts).

use ibkr_flex_mcp::flex::transport::ReqwestTransport;
use ibkr_flex_mcp::flex::{parse_positions, FlexClient};

fn live_credentials() -> Option<(String, String)> {
    // Allow a repo-root .env to supply the credentials, mirroring the binary.
    let _ = dotenvy::dotenv();
    let token = std::env::var("IBKR_FLEX_TOKEN")
        .ok()
        .filter(|v| !v.is_empty())?;
    let query_id = std::env::var("IBKR_FLEX_QUERY_ID")
        .ok()
        .filter(|v| !v.is_empty())?;
    Some((token, query_id))
}

#[tokio::test]
async fn live_fetch_returns_a_real_statement() {
    let Some((token, query_id)) = live_credentials() else {
        eprintln!(
            "SKIP live_fetch_returns_a_real_statement: set IBKR_FLEX_TOKEN and \
             IBKR_FLEX_QUERY_ID (env or .env) to run the live Flex fetch"
        );
        return;
    };

    let client = FlexClient::new(ReqwestTransport::new().expect("build reqwest transport"));

    let statement = client
        .fetch_statement(&token, &query_id)
        .await
        .expect("live Flex fetch should succeed with valid credentials");

    // Parse positions with the same library code the `flex_positions` MCP tool uses.
    let positions = parse_positions(&statement.raw_xml).expect("positions should parse");

    // Captured by default; visible with `cargo test --test live_flex -- --nocapture`.
    // NOTE: positions are your real account data — only shown with --nocapture.
    println!(
        "live Flex statement: query_id={} reference_code={} bytes={} positions={}",
        statement.query_id,
        statement.reference_code,
        statement.raw_xml.len(),
        positions.len(),
    );
    let fmt = |v: Option<f64>| v.map(|x| format!("{x}")).unwrap_or_else(|| "-".to_string());
    for p in &positions {
        println!(
            "  {:<10} qty={} avgPx={} markPx={} value={} {} uPnl={}",
            p.symbol,
            fmt(p.quantity),
            fmt(p.cost_basis_price),
            fmt(p.mark_price),
            fmt(p.position_value),
            p.currency.as_deref().unwrap_or("-"),
            fmt(p.unrealized_pnl),
        );
    }
    if positions.is_empty() {
        println!("  (no open positions — enable the 'Open Positions' section in your Flex Query)");
    }

    assert_eq!(
        statement.query_id, query_id,
        "statement should echo the query id"
    );
    assert!(
        !statement.reference_code.is_empty(),
        "a reference code should have been issued"
    );
    assert!(
        statement.raw_xml.contains("<FlexQueryResponse"),
        "expected a FlexQueryResponse document, got: {}",
        &statement.raw_xml[..statement.raw_xml.len().min(200)]
    );
}

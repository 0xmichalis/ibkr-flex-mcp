//! Live integration test against the real IBKR Flex Web Service.
//!
//! This exercises the full core path — `FlexClient::fetch_statement` over the real
//! `ReqwestTransport` (SendRequest -> poll GetStatement -> parse) against IBKR.
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
use ibkr_flex_mcp::flex::FlexClient;

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

    // Captured by default; visible with `cargo test --test live_flex -- --nocapture`.
    // NOTE: positions are your real account data — only shown with --nocapture.
    println!(
        "live Flex statement: query_id={} reference_code={} bytes={}",
        statement.query_id,
        statement.reference_code,
        statement.raw_xml.len()
    );
    print_positions(&statement.raw_xml);

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

/// Extract and print the `<OpenPosition>` rows from a Flex statement. Generic over whichever
/// attributes the query includes; falls back to listing the sections present if there are none.
fn print_positions(xml: &str) {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;
    use std::collections::{BTreeMap, BTreeSet};

    let mut reader = Reader::from_str(xml);
    let mut count = 0usize;
    let mut sections = BTreeSet::new();

    println!("open positions:");
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                sections.insert(name.clone());
                if name == "OpenPosition" {
                    count += 1;
                    let attrs: BTreeMap<String, String> = e
                        .attributes()
                        .flatten()
                        .map(|a| {
                            (
                                String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                                String::from_utf8_lossy(&a.value).into_owned(),
                            )
                        })
                        .collect();
                    let g = |k: &str| attrs.get(k).map(String::as_str).unwrap_or("-");
                    println!(
                        "  {:<10} qty={:>14} value={:>16} {:<3} unrealizedPnl={}",
                        g("symbol"),
                        g("position"),
                        g("positionValue"),
                        g("currency"),
                        g("fifoPnlUnrealized"),
                    );
                }
            }
            Err(e) => {
                eprintln!("XML parse error: {e}");
                break;
            }
            _ => {}
        }
    }

    println!("total open positions: {count}");
    if count == 0 {
        println!("  (no <OpenPosition> elements; sections present: {sections:?})");
        println!(
            "  enable the 'Open Positions' section in your Flex Query if you expected positions"
        );
    }
}

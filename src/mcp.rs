//! MCP server exposing the read-only Flex Query tools over stdio (via `rmcp`).
//!
//! The server holds a configured token + query id and a [`FlexClient`], and exposes two
//! read-only tools: `flex_run_query` (raw statement XML) and `flex_positions` (parsed,
//! structured open positions). There is deliberately no order-placement tool — the Flex Web
//! Service cannot trade.

use rmcp::model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler};

use crate::flex::transport::ReqwestTransport;
use crate::flex::{parse_positions, FlexClient, FlexError, FlexStatement};

/// The MCP server: a Flex client plus the credentials identifying which report to fetch.
pub struct FlexServer {
    client: FlexClient<ReqwestTransport>,
    token: String,
    query_id: String,
}

impl FlexServer {
    pub fn new(client: FlexClient<ReqwestTransport>, token: String, query_id: String) -> Self {
        Self {
            client,
            token,
            query_id,
        }
    }
}

#[tool_router]
impl FlexServer {
    #[tool(
        name = "flex_run_query",
        description = "Fetch the configured Interactive Brokers Flex Query report (read-only \
                       account data: positions, trades, cash, NAV, ...). Returns the raw Flex XML. \
                       This tool cannot place, modify, or cancel orders.",
        annotations(read_only_hint = true)
    )]
    async fn flex_run_query(&self) -> Result<CallToolResult, ErrorData> {
        let result = self
            .client
            .fetch_statement(&self.token, &self.query_id)
            .await;
        Ok(statement_to_result(result))
    }

    #[tool(
        name = "flex_positions",
        description = "Fetch the configured Flex Query report and return your open positions as \
                       structured JSON (symbol, quantity, mark price, position value, cost basis, \
                       unrealized P&L). Read-only; requires the Open Positions section enabled on \
                       the query.",
        annotations(read_only_hint = true)
    )]
    async fn flex_positions(&self) -> Result<CallToolResult, ErrorData> {
        let result = self
            .client
            .fetch_statement(&self.token, &self.query_id)
            .await;
        Ok(positions_to_result(result))
    }
}

#[tool_handler]
impl ServerHandler for FlexServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Read-only access to Interactive Brokers account data via the Flex Web Service. \
                 `flex_run_query` returns your configured Flex Query report as raw XML; \
                 `flex_positions` returns your open positions as structured JSON. \
                 This server cannot place, modify, or cancel orders.",
            )
    }
}

/// Map a Flex fetch outcome to an MCP tool result. A failure is reported as a tool-level error
/// (`is_error = true`) so the model sees the message, rather than a protocol error.
fn statement_to_result(result: Result<FlexStatement, FlexError>) -> CallToolResult {
    match result {
        Ok(statement) => CallToolResult::success(vec![Content::text(statement.raw_xml)]),
        Err(err) => CallToolResult::error(vec![Content::text(format!("Flex query failed: {err}"))]),
    }
}

/// Map a Flex fetch outcome to structured open positions. Fetch and parse failures are reported
/// as tool-level errors so the model sees the message.
fn positions_to_result(result: Result<FlexStatement, FlexError>) -> CallToolResult {
    let statement = match result {
        Ok(statement) => statement,
        Err(err) => {
            return CallToolResult::error(vec![Content::text(format!("Flex query failed: {err}"))])
        }
    };
    match parse_positions(&statement.raw_xml) {
        Ok(positions) => CallToolResult::structured(serde_json::json!({ "positions": positions })),
        Err(err) => CallToolResult::error(vec![Content::text(format!(
            "parsing positions failed: {err}"
        ))]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_statement_maps_to_success_with_raw_xml() {
        let statement = FlexStatement {
            query_id: "Q1".into(),
            reference_code: "REF".into(),
            raw_xml: "<FlexQueryResponse/>".into(),
        };

        let result = statement_to_result(Ok(statement));

        assert_eq!(result.is_error, Some(false));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("<FlexQueryResponse/>"), "json: {json}");
    }

    #[test]
    fn error_maps_to_tool_error_with_message() {
        let result = statement_to_result(Err(FlexError::NotReady(3)));

        assert_eq!(result.is_error, Some(true));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("not ready after 3"), "json: {json}");
    }

    fn statement(raw_xml: &str) -> FlexStatement {
        FlexStatement {
            query_id: "Q1".into(),
            reference_code: "REF".into(),
            raw_xml: raw_xml.into(),
        }
    }

    #[test]
    fn positions_map_to_structured_json() {
        let xml = r#"<FlexQueryResponse><FlexStatements><FlexStatement><OpenPositions><OpenPosition symbol="AAPL" position="100" currency="USD" /></OpenPositions></FlexStatement></FlexStatements></FlexQueryResponse>"#;

        let result = positions_to_result(Ok(statement(xml)));

        assert_ne!(result.is_error, Some(true));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("structuredContent"), "json: {json}");
        assert!(
            json.contains("AAPL") && json.contains("\"quantity\":100"),
            "json: {json}"
        );
    }

    #[test]
    fn no_positions_section_yields_empty_list_not_error() {
        let xml = r#"<FlexQueryResponse><FlexStatements><FlexStatement><AccountInformation name="x"/></FlexStatement></FlexStatements></FlexQueryResponse>"#;

        let result = positions_to_result(Ok(statement(xml)));

        assert_ne!(result.is_error, Some(true));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"positions\":[]"), "json: {json}");
    }

    #[test]
    fn fetch_error_maps_to_tool_error() {
        let result = positions_to_result(Err(FlexError::NotReady(2)));
        assert_eq!(result.is_error, Some(true));
    }
}

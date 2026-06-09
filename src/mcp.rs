//! MCP server exposing the read-only Flex Query tool over stdio (via `rmcp`).
//!
//! The server holds a configured token + query id and a [`FlexClient`]. Its single tool,
//! `flex_run_query`, fetches the configured Flex report and returns the raw statement XML.
//! There is deliberately no order-placement tool — the Flex Web Service cannot trade.

use rmcp::model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler};

use crate::flex::transport::ReqwestTransport;
use crate::flex::{FlexClient, FlexError, FlexStatement};

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
                 The tool `flex_run_query` returns your configured Flex Query report as XML. \
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
}

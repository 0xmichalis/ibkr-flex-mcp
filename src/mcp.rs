//! MCP server exposing the read-only Flex Query tools over stdio (via `rmcp`).
//!
//! The server holds a configured token + query id and a [`FlexClient`], and exposes four
//! read-only tools: `flex_run_query` (raw statement XML), `flex_positions` (parsed, structured
//! open positions), `flex_trades` (parsed, structured executions, narrowable by symbol, date
//! and level of detail) and `flex_cash` (parsed, structured per-currency cash balances). There
//! is deliberately no order-placement tool — the Flex Web Service cannot trade.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler};

use crate::flex::transport::ReqwestTransport;
use crate::flex::{
    parse_cash_summary, parse_positions, select_trades, FlexClient, FlexError, FlexStatement,
    TradeFilter,
};

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

    #[tool(
        name = "flex_trades",
        description = "Fetch the configured Flex Query report and return executed trades as \
                       structured JSON (symbol, date, buy/sell, open/close, quantity, price, \
                       commission, cost, realized P&L) — the authoritative lot history behind \
                       your positions' cost basis. Read-only; requires the Trades section \
                       enabled on the query, and only covers the query's configured period. \
                       A full year of executions is a large response: narrow it with `symbol`, \
                       `since`/`until` (YYYYMMDD), `level_of_detail` (use EXECUTION for \
                       individual fills) or `limit`. The reply reports `matched` alongside \
                       `returned`, so a `limit`-capped result is distinguishable from a \
                       complete one.",
        annotations(read_only_hint = true)
    )]
    async fn flex_trades(
        &self,
        Parameters(filter): Parameters<TradeFilter>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = self
            .client
            .fetch_statement(&self.token, &self.query_id)
            .await;
        Ok(trades_to_result(result, &filter))
    }

    #[tool(
        name = "flex_cash",
        description = "Fetch the configured Flex Query report and return cash balances as \
                       structured JSON: one row per currency (starting/ending/settled cash) \
                       plus a BASE_SUMMARY row totalling everything in the base currency, and \
                       the account row naming that base currency. Read-only; requires the Cash \
                       Report section (with currency breakout) and ideally Account Information \
                       enabled on the query.",
        annotations(read_only_hint = true)
    )]
    async fn flex_cash(&self) -> Result<CallToolResult, ErrorData> {
        let result = self
            .client
            .fetch_statement(&self.token, &self.query_id)
            .await;
        Ok(cash_to_result(result))
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
                 `flex_positions` returns your open positions as structured JSON; \
                 `flex_trades` returns the executions behind them (lot history); \
                 `flex_cash` returns per-currency cash balances and the base currency. \
                 This server cannot place, modify, or cancel orders.",
            )
    }
}

/// Map a Flex fetch outcome to an MCP tool result. A failure is reported as a tool-level error
/// (`is_error = true`) so the model sees the message, rather than a protocol error.
fn statement_to_result(result: Result<FlexStatement, FlexError>) -> CallToolResult {
    match result {
        Ok(statement) => CallToolResult::success(vec![ContentBlock::text(statement.raw_xml)]),
        Err(err) => CallToolResult::error(vec![ContentBlock::text(format!(
            "Flex query failed: {err}"
        ))]),
    }
}

/// Map a Flex fetch outcome to structured open positions. Fetch and parse failures are reported
/// as tool-level errors so the model sees the message.
fn positions_to_result(result: Result<FlexStatement, FlexError>) -> CallToolResult {
    rows_to_result(result, "positions", parse_positions)
}

/// Map a Flex fetch outcome to the selected trades, on the same terms as [`positions_to_result`].
/// Unlike positions this returns an object rather than a bare list, so `matched`/`returned` can
/// travel with the rows.
fn trades_to_result(
    result: Result<FlexStatement, FlexError>,
    filter: &TradeFilter,
) -> CallToolResult {
    let statement = match result {
        Ok(statement) => statement,
        Err(err) => {
            return CallToolResult::error(vec![ContentBlock::text(format!(
                "Flex query failed: {err}"
            ))])
        }
    };
    match select_trades(&statement.raw_xml, filter) {
        Ok(selection) => match serde_json::to_value(&selection) {
            Ok(value) => CallToolResult::structured(value),
            Err(err) => CallToolResult::error(vec![ContentBlock::text(format!(
                "serialising trades failed: {err}"
            ))]),
        },
        Err(err) => CallToolResult::error(vec![ContentBlock::text(format!(
            "parsing trades failed: {err}"
        ))]),
    }
}

/// Map a Flex fetch outcome to the cash summary, on the same terms as [`positions_to_result`].
/// Returns an object (account + per-currency rows) rather than a bare list.
fn cash_to_result(result: Result<FlexStatement, FlexError>) -> CallToolResult {
    let statement = match result {
        Ok(statement) => statement,
        Err(err) => {
            return CallToolResult::error(vec![ContentBlock::text(format!(
                "Flex query failed: {err}"
            ))])
        }
    };
    match parse_cash_summary(&statement.raw_xml) {
        Ok(summary) => match serde_json::to_value(&summary) {
            Ok(value) => CallToolResult::structured(value),
            Err(err) => CallToolResult::error(vec![ContentBlock::text(format!(
                "serialising cash failed: {err}"
            ))]),
        },
        Err(err) => CallToolResult::error(vec![ContentBlock::text(format!(
            "parsing cash failed: {err}"
        ))]),
    }
}

/// Fetch outcome -> `{ <section>: [...] }` structured content, with failures as tool-level errors.
fn rows_to_result<T: serde::Serialize>(
    result: Result<FlexStatement, FlexError>,
    section: &str,
    parse: impl Fn(&str) -> Result<Vec<T>, FlexError>,
) -> CallToolResult {
    let statement = match result {
        Ok(statement) => statement,
        Err(err) => {
            return CallToolResult::error(vec![ContentBlock::text(format!(
                "Flex query failed: {err}"
            ))])
        }
    };
    match parse(&statement.raw_xml) {
        Ok(rows) => CallToolResult::structured(serde_json::json!({ section: rows })),
        Err(err) => CallToolResult::error(vec![ContentBlock::text(format!(
            "parsing {section} failed: {err}"
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

    const TWO_TRADES: &str = r#"<FlexQueryResponse><FlexStatements><FlexStatement><Trades>
        <Trade symbol="AAPL" tradeDate="20260115" buySell="BUY" quantity="15" tradePrice="150.00" />
        <Trade symbol="MSFT" tradeDate="20260220" buySell="BUY" quantity="7" />
    </Trades></FlexStatement></FlexStatements></FlexQueryResponse>"#;

    #[test]
    fn trades_map_to_structured_json() {
        let result = trades_to_result(Ok(statement(TWO_TRADES)), &TradeFilter::default());

        assert_ne!(result.is_error, Some(true));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("structuredContent"), "json: {json}");
        assert!(
            json.contains("AAPL") && json.contains("\"quantity\":15"),
            "json: {json}"
        );
    }

    #[test]
    fn trades_filter_narrows_the_response_and_reports_both_counts() {
        let filter = TradeFilter {
            symbol: Some("msft".into()),
            ..Default::default()
        };

        let result = trades_to_result(Ok(statement(TWO_TRADES)), &filter);

        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("AAPL"), "filtered symbol leaked: {json}");
        assert!(json.contains("MSFT"), "json: {json}");
        assert!(
            json.contains("\"matched\":1") && json.contains("\"returned\":1"),
            "json: {json}"
        );
    }

    #[test]
    fn no_trades_section_yields_empty_list_not_error() {
        let xml = r#"<FlexQueryResponse><FlexStatements><FlexStatement><OpenPositions/></FlexStatement></FlexStatements></FlexQueryResponse>"#;

        let result = trades_to_result(Ok(statement(xml)), &TradeFilter::default());

        assert_ne!(result.is_error, Some(true));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"trades\":[]"), "json: {json}");
        assert!(json.contains("\"matched\":0"), "json: {json}");
    }

    #[test]
    fn trades_fetch_error_maps_to_tool_error() {
        let result = trades_to_result(Err(FlexError::NotReady(2)), &TradeFilter::default());
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn cash_maps_to_structured_json() {
        let xml = r#"<FlexQueryResponse><FlexStatements><FlexStatement>
            <AccountInformation accountId="U1" currency="CHF" />
            <CashReport><CashReportCurrency currency="USD" endingCash="7100.25" endingSettledCash="7000.00" /></CashReport>
        </FlexStatement></FlexStatements></FlexQueryResponse>"#;

        let result = cash_to_result(Ok(statement(xml)));

        assert_ne!(result.is_error, Some(true));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("structuredContent"), "json: {json}");
        assert!(
            json.contains("\"ending_cash\":7100.25") && json.contains("\"base_currency\":\"CHF\""),
            "json: {json}"
        );
    }

    #[test]
    fn no_cash_section_yields_empty_list_not_error() {
        let xml = r#"<FlexQueryResponse><FlexStatements><FlexStatement><OpenPositions/></FlexStatement></FlexStatements></FlexQueryResponse>"#;

        let result = cash_to_result(Ok(statement(xml)));

        assert_ne!(result.is_error, Some(true));
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"cash\":[]"), "json: {json}");
    }

    #[test]
    fn cash_fetch_error_maps_to_tool_error() {
        let result = cash_to_result(Err(FlexError::NotReady(2)));
        assert_eq!(result.is_error, Some(true));
    }
}

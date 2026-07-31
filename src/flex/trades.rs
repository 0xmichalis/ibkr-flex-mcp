//! Parse executed trades out of a Flex statement (`<Trade>` rows).
//!
//! The Trades section is the authoritative lot history: every execution, with the quantity,
//! price, commission and open/close indicator IBKR used to compute cost basis. As with
//! positions, a Flex query only emits the fields it is configured to include, so every field
//! beyond `symbol` is optional.
//!
//! A Trades section can be configured at several levels of detail (execution, order, symbol
//! summary). All of them arrive as `<Trade>` rows distinguished by `levelOfDetail`, which is
//! exposed verbatim so callers can avoid double counting.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::rows::{parse_rows, Attrs};
use super::FlexError;

/// A single trade row from a Flex statement.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Trade {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// IBKR execution id (`tradeID`); absent on aggregated rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_date: Option<String>,
    /// Execution timestamp (`dateTime`), in the query's configured date/time format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_time: Option<String>,
    /// Target settlement date (`settleDateTarget`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle_date: Option<String>,
    /// `EXECUTION`, `ORDER`, `SYMBOL_SUMMARY`, ... — rows at different levels overlap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level_of_detail: Option<String>,
    /// `BUY` or `SELL`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buy_sell: Option<String>,
    /// `O` (opening) or `C` (closing) — which lots this trade created or consumed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_close: Option<String>,
    /// Signed quantity: positive for buys, negative for sells.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_price: Option<f64>,
    /// Quantity x price x multiplier (IBKR `tradeMoney`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_money: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proceeds: Option<f64>,
    /// Commission charged (IBKR `ibCommission`; negative = paid).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commission: Option<f64>,
    /// Proceeds net of commission and taxes (IBKR `netCash`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net_cash: Option<f64>,
    /// Basis added by (or removed from) the position by this trade (IBKR `cost`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    /// Realized P&L on closing trades (IBKR `fifoPnlRealized`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realized_pnl: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exchange: Option<String>,
    /// IBKR note codes (e.g. `A` assignment, `Ex` exercise, `P` partial execution).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Which trades to return. Every field is optional; an absent field narrows nothing.
///
/// A whole year of executions is a large payload, and a caller that only wants one holding's
/// lot history should not have to receive — or truncate — the rest.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TradeFilter {
    /// Only trades in this symbol, matched case-insensitively and exactly. `NHY` therefore
    /// excludes the separately listed `NHYo`.
    pub symbol: Option<String>,
    /// Earliest trade date to include, as `YYYYMMDD`, inclusive.
    pub since: Option<String>,
    /// Latest trade date to include, as `YYYYMMDD`, inclusive.
    pub until: Option<String>,
    /// Only rows at this level of detail, e.g. `EXECUTION`. Matched case-insensitively.
    pub level_of_detail: Option<String>,
    /// Return at most this many trades, keeping the most recent. `matched` still reports how
    /// many there were, so a capped result is never mistaken for a complete one.
    pub limit: Option<usize>,
}

/// Trades selected from a statement, plus the counts needed to detect a capped result.
#[derive(Debug, PartialEq, Serialize)]
pub struct TradeSelection {
    pub trades: Vec<Trade>,
    /// How many trades matched the filter, before `limit`.
    pub matched: usize,
    /// How many are in `trades`. Less than `matched` means `limit` truncated the result.
    pub returned: usize,
}

impl TradeFilter {
    /// A date filter excludes rows with no `tradeDate`: absence is not proof of being in range.
    fn matches(&self, trade: &Trade) -> bool {
        let symbol_ok = self
            .symbol
            .as_ref()
            .is_none_or(|s| trade.symbol.eq_ignore_ascii_case(s));
        let level_ok = self.level_of_detail.as_ref().is_none_or(|want| {
            trade
                .level_of_detail
                .as_ref()
                .is_some_and(|got| got.eq_ignore_ascii_case(want))
        });
        let since_ok = self
            .since
            .as_ref()
            .is_none_or(|s| trade.trade_date.as_ref().is_some_and(|d| d >= s));
        let until_ok = self
            .until
            .as_ref()
            .is_none_or(|u| trade.trade_date.as_ref().is_some_and(|d| d <= u));

        symbol_ok && level_ok && since_ok && until_ok
    }
}

/// Parse all `<Trade>` rows from a Flex statement XML document.
pub fn parse_trades(xml: &str) -> Result<Vec<Trade>, FlexError> {
    parse_rows(xml, "Trade", trade_from)
}

/// Parse and narrow a statement's trades.
///
/// Results are ordered by trade date ascending. IBKR does *not* emit `<Trade>` rows in date
/// order, so sorting is what makes `limit` mean "the most recent" rather than "whichever
/// happened to be last in the file".
pub fn select_trades(xml: &str, filter: &TradeFilter) -> Result<TradeSelection, FlexError> {
    let mut trades: Vec<Trade> = parse_trades(xml)?
        .into_iter()
        .filter(|t| filter.matches(t))
        .collect();

    // Stable, so rows sharing a date keep statement order (and undated rows sort first).
    trades.sort_by(|a, b| a.trade_date.cmp(&b.trade_date));

    let matched = trades.len();
    if let Some(limit) = filter.limit {
        if matched > limit {
            trades.drain(..matched - limit);
        }
    }

    Ok(TradeSelection {
        returned: trades.len(),
        matched,
        trades,
    })
}

fn trade_from(a: &Attrs) -> Trade {
    Trade {
        symbol: a.text("symbol").unwrap_or_default(),
        description: a.text("description"),
        asset_category: a.text("assetCategory"),
        currency: a.text("currency"),
        trade_id: a.text("tradeID"),
        trade_date: a.text("tradeDate"),
        date_time: a.text("dateTime"),
        settle_date: a.text("settleDateTarget"),
        level_of_detail: a.text("levelOfDetail"),
        buy_sell: a.text("buySell"),
        open_close: a.text("openCloseIndicator"),
        quantity: a.num("quantity"),
        trade_price: a.num("tradePrice"),
        trade_money: a.num("tradeMoney"),
        proceeds: a.num("proceeds"),
        commission: a.num("ibCommission"),
        net_cash: a.num("netCash"),
        cost: a.num("cost"),
        realized_pnl: a.num("fifoPnlRealized"),
        exchange: a.text("exchange"),
        notes: a.text("notes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_TRADES: &str = r#"<FlexQueryResponse queryName="Q" type="AF"><FlexStatements count="1"><FlexStatement accountId="U1"><Trades>
        <Trade accountId="U1" currency="USD" symbol="AAPL" description="APPLE INC" assetCategory="STK" tradeID="7788" tradeDate="20260115" dateTime="20260115;103005" settleDateTarget="20260117" levelOfDetail="EXECUTION" buySell="BUY" openCloseIndicator="O" quantity="15" tradePrice="150.00" tradeMoney="2250.00" proceeds="-2250.00" ibCommission="-1.00" netCash="-2251.00" cost="2251.00" fifoPnlRealized="0" exchange="NASDAQ" notes="P" />
        <Trade accountId="U1" currency="USD" symbol="AAPL" assetCategory="STK" tradeDate="20260220" buySell="SELL" openCloseIndicator="C" quantity="-5" tradePrice="180.00" fifoPnlRealized="149.50" />
    </Trades></FlexStatement></FlexStatements></FlexQueryResponse>"#;

    #[test]
    fn parses_each_trade_with_available_fields() {
        let trades = parse_trades(WITH_TRADES).unwrap();
        assert_eq!(trades.len(), 2);

        let buy = &trades[0];
        assert_eq!(buy.symbol, "AAPL");
        assert_eq!(buy.description.as_deref(), Some("APPLE INC"));
        assert_eq!(buy.currency.as_deref(), Some("USD"));
        assert_eq!(buy.trade_id.as_deref(), Some("7788"));
        assert_eq!(buy.trade_date.as_deref(), Some("20260115"));
        assert_eq!(buy.date_time.as_deref(), Some("20260115;103005"));
        assert_eq!(buy.settle_date.as_deref(), Some("20260117"));
        assert_eq!(buy.level_of_detail.as_deref(), Some("EXECUTION"));
        assert_eq!(buy.buy_sell.as_deref(), Some("BUY"));
        assert_eq!(buy.open_close.as_deref(), Some("O"));
        assert_eq!(buy.quantity, Some(15.0));
        assert_eq!(buy.trade_price, Some(150.0));
        assert_eq!(buy.trade_money, Some(2250.0));
        assert_eq!(buy.proceeds, Some(-2250.0));
        assert_eq!(buy.commission, Some(-1.0));
        assert_eq!(buy.net_cash, Some(-2251.0));
        assert_eq!(buy.cost, Some(2251.0));
        assert_eq!(buy.realized_pnl, Some(0.0));
        assert_eq!(buy.exchange.as_deref(), Some("NASDAQ"));
        assert_eq!(buy.notes.as_deref(), Some("P"));
    }

    #[test]
    fn leaves_unselected_fields_as_none_and_signs_sells_negative() {
        let trades = parse_trades(WITH_TRADES).unwrap();
        let sell = &trades[1];
        assert_eq!(sell.quantity, Some(-5.0));
        assert_eq!(sell.open_close.as_deref(), Some("C"));
        assert_eq!(sell.realized_pnl, Some(149.5));
        assert_eq!(sell.description, None);
        assert_eq!(sell.commission, None);
        assert_eq!(sell.trade_id, None);
    }

    #[test]
    fn returns_empty_when_no_trades_section() {
        let xml = r#"<FlexQueryResponse><FlexStatements count="1"><FlexStatement accountId="U1"><OpenPositions/></FlexStatement></FlexStatements></FlexQueryResponse>"#;
        assert_eq!(parse_trades(xml).unwrap(), vec![]);
    }

    /// Deliberately out of date order: IBKR does not emit trades chronologically.
    const MIXED: &str = r#"<FlexQueryResponse><FlexStatements><FlexStatement><Trades>
        <Trade symbol="NHY" tradeDate="20260301" levelOfDetail="EXECUTION" quantity="10" />
        <Trade symbol="NHY" tradeDate="20260101" levelOfDetail="EXECUTION" quantity="20" />
        <Trade symbol="NHYo" tradeDate="20260201" levelOfDetail="EXECUTION" quantity="30" />
        <Trade symbol="nhy" tradeDate="20260601" levelOfDetail="SYMBOL_SUMMARY" quantity="40" />
        <Trade symbol="MSFT" quantity="50" />
    </Trades></FlexStatement></FlexStatements></FlexQueryResponse>"#;

    fn select(filter: TradeFilter) -> TradeSelection {
        select_trades(MIXED, &filter).unwrap()
    }

    fn quantities(sel: &TradeSelection) -> Vec<f64> {
        sel.trades.iter().filter_map(|t| t.quantity).collect()
    }

    #[test]
    fn no_filter_returns_everything_sorted_by_trade_date() {
        let sel = select(TradeFilter::default());
        assert_eq!(sel.matched, 5);
        assert_eq!(sel.returned, 5);
        // The undated row sorts first, then ascending by date — not statement order.
        assert_eq!(quantities(&sel), vec![50.0, 20.0, 30.0, 10.0, 40.0]);
    }

    #[test]
    fn symbol_matches_case_insensitively_but_exactly() {
        let sel = select(TradeFilter {
            symbol: Some("nhy".into()),
            ..Default::default()
        });
        // "nhy" matches "NHY" and "nhy", but never the separately listed "NHYo".
        assert_eq!(quantities(&sel), vec![20.0, 10.0, 40.0]);
    }

    #[test]
    fn date_bounds_are_inclusive_and_exclude_undated_rows() {
        let sel = select(TradeFilter {
            since: Some("20260101".into()),
            until: Some("20260301".into()),
            ..Default::default()
        });
        // MSFT has no tradeDate, so it cannot be shown to fall in range.
        assert_eq!(quantities(&sel), vec![20.0, 30.0, 10.0]);
    }

    #[test]
    fn level_of_detail_filters_out_aggregate_rows() {
        let sel = select(TradeFilter {
            level_of_detail: Some("execution".into()),
            ..Default::default()
        });
        assert_eq!(quantities(&sel), vec![20.0, 30.0, 10.0]);
    }

    #[test]
    fn limit_keeps_the_most_recent_and_reports_the_full_count() {
        let sel = select(TradeFilter {
            level_of_detail: Some("EXECUTION".into()),
            limit: Some(2),
            ..Default::default()
        });
        assert_eq!(quantities(&sel), vec![30.0, 10.0]); // newest two, not the last two in the file
        assert_eq!(sel.matched, 3, "matched must reveal the truncation");
        assert_eq!(sel.returned, 2);
    }

    #[test]
    fn limit_larger_than_the_result_changes_nothing() {
        let sel = select(TradeFilter {
            limit: Some(99),
            ..Default::default()
        });
        assert_eq!(sel.matched, 5);
        assert_eq!(sel.returned, 5);
    }

    #[test]
    fn filters_combine_and_can_match_nothing() {
        let sel = select(TradeFilter {
            symbol: Some("NHY".into()),
            since: Some("20270101".into()),
            ..Default::default()
        });
        assert_eq!(sel.matched, 0);
        assert!(sel.trades.is_empty());
    }
}

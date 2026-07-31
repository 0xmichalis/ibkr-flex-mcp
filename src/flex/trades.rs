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

use serde::Serialize;

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

/// Parse all `<Trade>` rows from a Flex statement XML document.
pub fn parse_trades(xml: &str) -> Result<Vec<Trade>, FlexError> {
    parse_rows(xml, "Trade", trade_from)
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
}

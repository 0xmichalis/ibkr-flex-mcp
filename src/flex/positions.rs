//! Parse open positions out of a Flex statement (`<OpenPosition>` rows).
//!
//! Flex queries only emit the fields the query is configured to include, so every field beyond
//! `symbol` is optional. Parsing is tolerant: unknown/missing attributes are simply absent.

use serde::Serialize;

use super::rows::{parse_rows, Attrs};
use super::FlexError;

/// A single open position from a Flex statement.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Position {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mark_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_value: Option<f64>,
    /// Average cost per share (IBKR `costBasisPrice`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_basis_price: Option<f64>,
    /// Total cost basis (IBKR `costBasisMoney`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_basis: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unrealized_pnl: Option<f64>,
}

/// Parse all `<OpenPosition>` rows from a Flex statement XML document.
pub fn parse_positions(xml: &str) -> Result<Vec<Position>, FlexError> {
    parse_rows(xml, "OpenPosition", position_from)
}

fn position_from(a: &Attrs) -> Position {
    Position {
        symbol: a.text("symbol").unwrap_or_default(),
        description: a.text("description"),
        asset_category: a.text("assetCategory"),
        currency: a.text("currency"),
        quantity: a.num("position"),
        mark_price: a.num("markPrice"),
        position_value: a.num("positionValue"),
        cost_basis_price: a.num("costBasisPrice"),
        cost_basis: a.num("costBasisMoney"),
        unrealized_pnl: a.num("fifoPnlUnrealized"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_POSITIONS: &str = r#"<FlexQueryResponse queryName="Q" type="AF"><FlexStatements count="1"><FlexStatement accountId="U1"><OpenPositions>
        <OpenPosition accountId="U1" currency="USD" symbol="AAPL" description="APPLE INC" assetCategory="STK" position="100" markPrice="185.00" positionValue="18500.00" costBasisPrice="150.00" costBasisMoney="15000.00" fifoPnlUnrealized="3500.00" />
        <OpenPosition accountId="U1" currency="EUR" symbol="SAP" description="SAP SE" assetCategory="STK" position="-50" markPrice="120.00" positionValue="-6000.00" />
    </OpenPositions></FlexStatement></FlexStatements></FlexQueryResponse>"#;

    #[test]
    fn parses_each_open_position_with_available_fields() {
        let positions = parse_positions(WITH_POSITIONS).unwrap();
        assert_eq!(positions.len(), 2);

        let aapl = &positions[0];
        assert_eq!(aapl.symbol, "AAPL");
        assert_eq!(aapl.description.as_deref(), Some("APPLE INC"));
        assert_eq!(aapl.currency.as_deref(), Some("USD"));
        assert_eq!(aapl.quantity, Some(100.0));
        assert_eq!(aapl.mark_price, Some(185.0));
        assert_eq!(aapl.position_value, Some(18500.0));
        assert_eq!(aapl.cost_basis_price, Some(150.0)); // average cost per share
        assert_eq!(aapl.cost_basis, Some(15000.0));
        assert_eq!(aapl.unrealized_pnl, Some(3500.0));
    }

    #[test]
    fn leaves_unselected_fields_as_none() {
        let positions = parse_positions(WITH_POSITIONS).unwrap();
        let sap = &positions[1];
        assert_eq!(sap.symbol, "SAP");
        assert_eq!(sap.quantity, Some(-50.0)); // shorts parse as negative
        assert_eq!(sap.mark_price, Some(120.0));
        assert_eq!(sap.cost_basis_price, None);
        assert_eq!(sap.cost_basis, None);
        assert_eq!(sap.unrealized_pnl, None);
    }

    #[test]
    fn returns_empty_when_no_open_positions_section() {
        let xml = r#"<FlexQueryResponse><FlexStatements count="1"><FlexStatement accountId="U1"><AccountInformation accountId="U1" name="Jane Doe"/></FlexStatement></FlexStatements></FlexQueryResponse>"#;
        assert_eq!(parse_positions(xml).unwrap(), vec![]);
    }
}

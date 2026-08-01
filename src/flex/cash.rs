//! Parse cash balances out of a Flex statement (`<CashReportCurrency>` rows), plus the
//! `<AccountInformation>` row that names the base currency.
//!
//! The Cash Report section emits one row per currency, and — when the query breaks out
//! currencies — a `BASE_SUMMARY` row totalling everything in the base currency
//! (`levelOfDetail="BaseCurrency"`). As elsewhere, a Flex query only emits the fields it is
//! configured to include, so every field beyond `currency` is optional.

use serde::Serialize;

use super::rows::{parse_rows, Attrs};
use super::FlexError;

/// One currency's cash balances from a Flex statement.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CashBalance {
    /// ISO currency, or `BASE_SUMMARY` for the base-currency total row.
    pub currency: String,
    /// `Currency` or `BaseCurrency` — tells the per-currency rows from the base-currency total.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level_of_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starting_cash: Option<f64>,
    /// Total cash at period end (IBKR `endingCash`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ending_cash: Option<f64>,
    /// Settled portion of `ending_cash` (IBKR `endingSettledCash`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ending_settled_cash: Option<f64>,
}

/// The account identity row (`<AccountInformation>`), chiefly for the base currency.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AccountInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The account's base currency (IBKR `currency`), which `BASE_SUMMARY` rows are stated in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_currency: Option<String>,
}

/// Cash balances plus the account row, as one bounded payload.
#[derive(Debug, PartialEq, Serialize)]
pub struct CashSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountInfo>,
    pub cash: Vec<CashBalance>,
}

/// Parse the Cash Report and Account Information sections from a Flex statement XML document.
pub fn parse_cash_summary(xml: &str) -> Result<CashSummary, FlexError> {
    let account = parse_rows(xml, "AccountInformation", account_from)?
        .into_iter()
        .next();
    let cash = parse_rows(xml, "CashReportCurrency", balance_from)?;
    Ok(CashSummary { account, cash })
}

fn balance_from(a: &Attrs) -> CashBalance {
    CashBalance {
        currency: a.text("currency").unwrap_or_default(),
        level_of_detail: a.text("levelOfDetail"),
        from_date: a.text("fromDate"),
        to_date: a.text("toDate"),
        starting_cash: a.num("startingCash"),
        ending_cash: a.num("endingCash"),
        ending_settled_cash: a.num("endingSettledCash"),
    }
}

fn account_from(a: &Attrs) -> AccountInfo {
    AccountInfo {
        account_id: a.text("accountId"),
        name: a.text("name"),
        base_currency: a.text("currency"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_CASH: &str = r#"<FlexQueryResponse queryName="Q" type="AF"><FlexStatements count="1"><FlexStatement accountId="U1">
        <AccountInformation accountId="U1" name="Jane Doe" currency="CHF" />
        <CashReport>
            <CashReportCurrency accountId="U1" currency="BASE_SUMMARY" levelOfDetail="BaseCurrency" fromDate="20250801" toDate="20260731" startingCash="10000.00" endingCash="12345.67" endingSettledCash="12000.00" />
            <CashReportCurrency accountId="U1" currency="CHF" levelOfDetail="Currency" fromDate="20250801" toDate="20260731" startingCash="5000.00" endingCash="6000.50" endingSettledCash="6000.50" />
            <CashReportCurrency accountId="U1" currency="USD" levelOfDetail="Currency" endingCash="7100.25" />
        </CashReport>
    </FlexStatement></FlexStatements></FlexQueryResponse>"#;

    #[test]
    fn parses_each_currency_row_with_available_fields() {
        let summary = parse_cash_summary(WITH_CASH).unwrap();
        assert_eq!(summary.cash.len(), 3);

        let base = &summary.cash[0];
        assert_eq!(base.currency, "BASE_SUMMARY");
        assert_eq!(base.level_of_detail.as_deref(), Some("BaseCurrency"));
        assert_eq!(base.from_date.as_deref(), Some("20250801"));
        assert_eq!(base.to_date.as_deref(), Some("20260731"));
        assert_eq!(base.starting_cash, Some(10000.0));
        assert_eq!(base.ending_cash, Some(12345.67));
        assert_eq!(base.ending_settled_cash, Some(12000.0));
    }

    #[test]
    fn leaves_unselected_fields_as_none() {
        let summary = parse_cash_summary(WITH_CASH).unwrap();
        let usd = &summary.cash[2];
        assert_eq!(usd.currency, "USD");
        assert_eq!(usd.ending_cash, Some(7100.25));
        assert_eq!(usd.starting_cash, None);
        assert_eq!(usd.ending_settled_cash, None);
    }

    #[test]
    fn parses_the_account_information_row() {
        let summary = parse_cash_summary(WITH_CASH).unwrap();
        let account = summary.account.expect("account row should parse");
        assert_eq!(account.account_id.as_deref(), Some("U1"));
        assert_eq!(account.name.as_deref(), Some("Jane Doe"));
        assert_eq!(account.base_currency.as_deref(), Some("CHF"));
    }

    #[test]
    fn missing_sections_yield_empty_summary_not_error() {
        let xml = r#"<FlexQueryResponse><FlexStatements count="1"><FlexStatement accountId="U1"><OpenPositions/></FlexStatement></FlexStatements></FlexQueryResponse>"#;
        let summary = parse_cash_summary(xml).unwrap();
        assert_eq!(summary.account, None);
        assert_eq!(summary.cash, vec![]);
    }
}

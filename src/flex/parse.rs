//! Pure parsing of Flex Web Service control responses.
//!
//! XML is parsed with `quick-xml`, a pull parser that performs **no DTD or entity expansion**,
//! so XXE and entity-expansion ("billion laughs") attacks are absent by construction.

use serde::Deserialize;
use thiserror::Error;

/// The marker IBKR returns from GetStatement while a statement is still being generated.
const IN_PROGRESS_MARKER: &str = "Statement generation in progress";

/// Errors surfaced by the Flex client.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FlexError {
    /// IBKR returned a non-success control response (bad token, expired, query error, ...).
    #[error("Flex API error: {0}")]
    Api(String),
    /// A response could not be parsed, or a required field was missing.
    #[error("Flex response malformed: {0}")]
    Parse(String),
    /// The statement was still being generated after the configured number of poll attempts.
    #[error("statement not ready after {0} attempts")]
    NotReady(u32),
    /// The underlying HTTP transport failed.
    #[error("transport error: {0}")]
    Transport(String),
}

/// A Flex Web Service control response (`<FlexStatementResponse>`): the envelope returned by
/// SendRequest, and by GetStatement when the statement is not (yet) available.
#[derive(Debug, Deserialize)]
struct ControlResponse {
    #[serde(rename = "Status")]
    status: Option<String>,
    #[serde(rename = "ReferenceCode")]
    reference_code: Option<String>,
    #[serde(rename = "ErrorCode")]
    error_code: Option<String>,
    #[serde(rename = "ErrorMessage")]
    error_message: Option<String>,
}

/// Outcome of a single GetStatement poll.
#[derive(Debug, PartialEq, Eq)]
pub enum GetOutcome {
    /// Statement still generating; poll again.
    InProgress,
    /// The statement is ready; the payload is the raw report XML.
    Statement(String),
}

/// Parse a SendRequest response and return the reference code used to poll for the statement.
pub fn parse_send_response(xml: &str) -> Result<String, FlexError> {
    let resp: ControlResponse = quick_xml::de::from_str(xml)
        .map_err(|e| FlexError::Parse(format!("SendRequest response: {e}")))?;

    match resp.status.as_deref() {
        Some("Success") => {}
        Some(other) => {
            return Err(FlexError::Api(format!(
                "SendRequest status {other}: {} - {}",
                resp.error_code.unwrap_or_default(),
                resp.error_message.unwrap_or_default()
            )))
        }
        None => return Err(FlexError::Parse("missing Status element".into())),
    }

    resp.reference_code
        .filter(|c| !c.is_empty())
        .ok_or_else(|| FlexError::Parse("no reference code in response".into()))
}

/// Classify a GetStatement response body: still generating, an error, or the actual statement.
///
/// The real statement is a `<FlexQueryResponse>` document; we treat anything that is not an
/// in-progress marker or a recognised error envelope as the statement payload.
pub fn classify_get_response(body: &str) -> Result<GetOutcome, FlexError> {
    if body.contains(IN_PROGRESS_MARKER) {
        return Ok(GetOutcome::InProgress);
    }

    // A control envelope carrying an error code (e.g. expired token, invalid reference code).
    if let Ok(resp) = quick_xml::de::from_str::<ControlResponse>(body) {
        if resp.status.as_deref() != Some("Success") {
            if let Some(code) = resp.error_code {
                return Err(FlexError::Api(format!(
                    "GetStatement error {code}: {}",
                    resp.error_message.unwrap_or_default()
                )));
            }
        }
    }

    Ok(GetOutcome::Statement(body.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEND_SUCCESS: &str = r#"<FlexStatementResponse timestamp="2026-06-09"><Status>Success</Status><ReferenceCode>REF123</ReferenceCode><Url>https://gdcdyn.interactivebrokers.com/x/GetStatement</Url></FlexStatementResponse>"#;

    #[test]
    fn send_success_yields_reference_code() {
        assert_eq!(parse_send_response(SEND_SUCCESS).unwrap(), "REF123");
    }

    #[test]
    fn send_failure_surfaces_api_error() {
        let xml = r#"<FlexStatementResponse><Status>Fail</Status><ErrorCode>1012</ErrorCode><ErrorMessage>Token has expired</ErrorMessage></FlexStatementResponse>"#;
        let err = parse_send_response(xml).unwrap_err();
        assert!(matches!(err, FlexError::Api(_)), "got {err:?}");
        assert!(err.to_string().contains("1012"));
        assert!(err.to_string().contains("Token has expired"));
    }

    #[test]
    fn send_success_without_reference_code_is_parse_error() {
        let xml = r#"<FlexStatementResponse><Status>Success</Status></FlexStatementResponse>"#;
        assert!(matches!(
            parse_send_response(xml).unwrap_err(),
            FlexError::Parse(_)
        ));
    }

    #[test]
    fn get_in_progress_is_detected() {
        let xml = r#"<FlexStatementResponse><Status>Warn</Status><ErrorCode>1019</ErrorCode><ErrorMessage>Statement generation in progress. Try again shortly.</ErrorMessage></FlexStatementResponse>"#;
        assert_eq!(classify_get_response(xml).unwrap(), GetOutcome::InProgress);
    }

    #[test]
    fn get_statement_payload_is_returned_verbatim() {
        let xml = r#"<FlexQueryResponse queryName="Activity" type="AF"><FlexStatements count="1"><FlexStatement accountId="U1234567"><OpenPositions/></FlexStatement></FlexStatements></FlexQueryResponse>"#;
        assert_eq!(
            classify_get_response(xml).unwrap(),
            GetOutcome::Statement(xml.to_string())
        );
    }

    #[test]
    fn get_error_envelope_surfaces_api_error() {
        let xml = r#"<FlexStatementResponse><Status>Fail</Status><ErrorCode>1003</ErrorCode><ErrorMessage>Statement could not be generated</ErrorMessage></FlexStatementResponse>"#;
        let err = classify_get_response(xml).unwrap_err();
        assert!(matches!(err, FlexError::Api(_)), "got {err:?}");
        assert!(err.to_string().contains("1003"));
    }
}

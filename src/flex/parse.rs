//! Pure parsing of Flex Web Service control responses.
//!
//! XML is parsed with `quick-xml`, a pull parser that performs **no DTD or entity expansion**,
//! so XXE and entity-expansion ("billion laughs") attacks are absent by construction.

use quick_xml::{Reader, events::Event};
use serde::Deserialize;
use thiserror::Error;

/// The Flex v3 error code for a statement that is still being generated.
const IN_PROGRESS_CODE: &str = "1019";

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

impl FlexError {
    pub(super) fn redact_token(mut self, token: &str) -> Self {
        let message = match &mut self {
            Self::Api(message) | Self::Parse(message) | Self::Transport(message) => message,
            Self::NotReady(_) => return self,
        };
        // A remote error or an injected transport may echo the token from the request.
        if !token.is_empty() && message.contains(token) {
            *message = message.replace(token, "[redacted]");
        }
        self
    }
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
    if !matches!(response_root(xml), Some(ResponseRoot::Control)) {
        return Err(FlexError::Parse(
            "SendRequest expected a FlexStatementResponse XML envelope".into(),
        ));
    }

    let resp: ControlResponse = quick_xml::de::from_str(xml)
        .map_err(|e| FlexError::Parse(format!("SendRequest response: {e}")))?;

    match resp.status.as_deref() {
        Some("Success") => {}
        Some(other) => {
            return Err(FlexError::Api(format!(
                "SendRequest status {other}: {} - {}",
                resp.error_code.unwrap_or_default(),
                resp.error_message.unwrap_or_default()
            )));
        }
        None => return Err(FlexError::Parse("missing Status element".into())),
    }

    resp.reference_code
        .filter(|code| !code.trim().is_empty())
        .ok_or_else(|| FlexError::Parse("no reference code in response".into()))
}

/// Classify the response by its XML root. The report body can contain arbitrary field text.
pub fn classify_get_response(body: String) -> Result<GetOutcome, FlexError> {
    match response_root(&body) {
        Some(ResponseRoot::Statement) => return Ok(GetOutcome::Statement(body)),
        Some(ResponseRoot::Control) => {}
        None => {
            return Err(FlexError::Parse(
                "GetStatement expected a FlexQueryResponse or FlexStatementResponse XML envelope"
                    .into(),
            ));
        }
    }

    let resp: ControlResponse = quick_xml::de::from_str(&body)
        .map_err(|e| FlexError::Parse(format!("GetStatement response: {e}")))?;

    match (
        resp.status.as_deref(),
        resp.error_code.as_deref().map(str::trim),
    ) {
        (Some("Success"), _) => Err(FlexError::Parse(
            "GetStatement returned a success status without a statement".into(),
        )),
        (_, None | Some("")) => Err(FlexError::Parse(
            "GetStatement control response is missing an error code".into(),
        )),
        (_, Some(IN_PROGRESS_CODE)) => Ok(GetOutcome::InProgress),
        (_, Some(code)) => Err(FlexError::Api(format!(
            "GetStatement error {code}: {}",
            resp.error_message.unwrap_or_default()
        ))),
    }
}

enum ResponseRoot {
    Control,
    Statement,
}

fn response_root(xml: &str) -> Option<ResponseRoot> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event().ok()? {
            Event::Start(root) | Event::Empty(root) => {
                return match root.name().as_ref() {
                    b"FlexStatementResponse" => Some(ResponseRoot::Control),
                    b"FlexQueryResponse" => Some(ResponseRoot::Statement),
                    _ => None,
                };
            }
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) | Event::DocType(_) => {}
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

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
        assert_matches!(err, FlexError::Api(_));
        assert!(err.to_string().contains("1012"));
        assert!(err.to_string().contains("Token has expired"));
    }

    #[test]
    fn send_success_without_reference_code_is_parse_error() {
        for xml in [
            "<FlexStatementResponse><Status>Success</Status></FlexStatementResponse>",
            "<FlexStatementResponse><Status>Success</Status><ReferenceCode> </ReferenceCode></FlexStatementResponse>",
        ] {
            assert_matches!(parse_send_response(xml).unwrap_err(), FlexError::Parse(_));
        }
    }

    #[test]
    fn send_requires_the_control_envelope() {
        let xml =
            "<WrongRoot><Status>Success</Status><ReferenceCode>REF123</ReferenceCode></WrongRoot>";
        assert_matches!(parse_send_response(xml).unwrap_err(), FlexError::Parse(_));
    }

    #[test]
    fn get_in_progress_is_detected() {
        let xml = "<FlexStatementResponse><Status>Warn</Status><ErrorCode>1019</ErrorCode></FlexStatementResponse>";
        assert_eq!(
            classify_get_response(xml.into()).unwrap(),
            GetOutcome::InProgress
        );
    }

    #[test]
    fn get_statement_payload_is_returned_verbatim() {
        let xml = "\u{feff}<?xml version=\"1.0\"?>\n<!-- export -->\n<FlexQueryResponse queryName=\"Statement generation in progress\"><FlexStatements count=\"0\"/></FlexQueryResponse>";
        assert_eq!(
            classify_get_response(xml.into()).unwrap(),
            GetOutcome::Statement(xml.to_string())
        );
    }

    #[test]
    fn get_error_envelope_surfaces_api_error() {
        let xml = r#"<FlexStatementResponse><Status>Fail</Status><ErrorCode>1003</ErrorCode><ErrorMessage>Statement could not be generated</ErrorMessage></FlexStatementResponse>"#;
        let err = classify_get_response(xml.into()).unwrap_err();
        assert_matches!(err, FlexError::Api(_));
        assert!(err.to_string().contains("1003"));
    }

    #[test]
    fn unexpected_or_malformed_get_response_does_not_complete_the_poll() {
        for xml in [
            "<html><body>Unexpected gateway response</body></html>",
            "<FlexStatementResponse><Status>Warn</Status><ErrorCode>1019</ErrorCode>",
            "<FlexStatementResponse><Status>Fail</Status></FlexStatementResponse>",
            "<FlexStatementResponse><Status>Success</Status></FlexStatementResponse>",
        ] {
            assert_matches!(
                classify_get_response(xml.into()).unwrap_err(),
                FlexError::Parse(_)
            );
        }
    }
}

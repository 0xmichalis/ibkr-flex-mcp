//! Pure parsing of Flex Web Service control responses.
//!
//! XML is parsed with `quick-xml`, a pull parser that performs **no DTD or entity expansion**,
//! so XXE and entity-expansion ("billion laughs") attacks are absent by construction.

use quick_xml::{Reader, events::Event, name::QName};
use serde::Deserialize;
use thiserror::Error;

/// The Flex v3 error code for a statement that is still being generated.
const IN_PROGRESS_CODE: &str = "1019";
const CONTROL_ROOT: &[u8] = b"FlexStatementResponse";

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
    let mut reader = Reader::from_str(xml);
    let Some(ResponseRoot::Control { open }) = response_root(&mut reader) else {
        return Err(FlexError::Parse(
            "SendRequest expected a FlexStatementResponse XML envelope".into(),
        ));
    };

    let resp = parse_control_response(xml, reader, open, "SendRequest")?;

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
    let mut reader = Reader::from_str(&body);
    let open = match response_root(&mut reader) {
        Some(ResponseRoot::Statement) => return Ok(GetOutcome::Statement(body)),
        Some(ResponseRoot::Control { open }) => open,
        None => {
            return Err(FlexError::Parse(
                "GetStatement expected a FlexQueryResponse or FlexStatementResponse XML envelope"
                    .into(),
            ));
        }
    };

    let resp = parse_control_response(&body, reader, open, "GetStatement")?;

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
    Control { open: bool },
    Statement,
}

fn response_root(reader: &mut Reader<&[u8]>) -> Option<ResponseRoot> {
    reader.config_mut().trim_text(true);
    loop {
        let event = reader.read_event().ok()?;
        let open = matches!(event, Event::Start(_));
        match event {
            Event::Start(root) | Event::Empty(root) => {
                return match root.name().as_ref() {
                    CONTROL_ROOT => Some(ResponseRoot::Control { open }),
                    b"FlexQueryResponse" => Some(ResponseRoot::Statement),
                    _ => None,
                };
            }
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) | Event::DocType(_) => {}
            _ => return None,
        }
    }
}

fn parse_control_response(
    xml: &str,
    mut reader: Reader<&[u8]>,
    root_open: bool,
    endpoint: &str,
) -> Result<ControlResponse, FlexError> {
    // Serde stops at the first root. Check the rest of the control document before using it.
    reader.config_mut().check_comments = true;
    let xml_error = |err| FlexError::Parse(format!("{endpoint} response: {err}"));
    if root_open {
        reader.read_to_end(QName(CONTROL_ROOT)).map_err(xml_error)?;
    }

    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Comment(comment) if valid_xml_chars(&comment) => {}
            Event::PI(pi) if valid_pi_target(pi.target()) && valid_xml_chars(pi.content()) => {}
            Event::Eof => break,
            _ => {
                return Err(FlexError::Parse(format!(
                    "{endpoint} response has unexpected content after the control envelope"
                )));
            }
        }
    }

    quick_xml::de::from_str(xml)
        .map_err(|err| FlexError::Parse(format!("{endpoint} response: {err}")))
}

fn valid_pi_target(target: &[u8]) -> bool {
    let Ok(target) = str::from_utf8(target) else {
        return false;
    };
    // The serializer exposes quick-xml's XML name check without writing any output.
    !target.is_empty()
        && !target.eq_ignore_ascii_case("xml")
        && quick_xml::se::Serializer::with_root(&mut String::new(), Some(target)).is_ok()
}

fn valid_xml_chars(text: &[u8]) -> bool {
    str::from_utf8(text).is_ok_and(|text| {
        text.chars().all(|ch| {
            matches!(
                ch,
                '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    const SEND_SUCCESS: &str = r#"<FlexStatementResponse timestamp="2026-06-09"><Status>Success</Status><ReferenceCode>REF123</ReferenceCode><Url>https://gdcdyn.interactivebrokers.com/x/GetStatement</Url></FlexStatementResponse>"#;
    const GET_IN_PROGRESS: &str = "<FlexStatementResponse><Status>Warn</Status><ErrorCode>1019</ErrorCode></FlexStatementResponse>";

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
        assert_eq!(
            classify_get_response(GET_IN_PROGRESS.into()).unwrap(),
            GetOutcome::InProgress
        );
    }

    #[test]
    fn control_responses_allow_trailing_whitespace_comments_and_processing_instructions() {
        let tail = " \t\r\n<!-- sent by gateway -->\n<?état complete?>\n";
        assert_eq!(
            parse_send_response(&format!("{SEND_SUCCESS}{tail}")).unwrap(),
            "REF123"
        );
        assert_eq!(
            classify_get_response(format!("{GET_IN_PROGRESS}{tail}")).unwrap(),
            GetOutcome::InProgress
        );
    }

    #[test]
    fn control_responses_reject_malformed_trailing_content() {
        for tail in [
            "<!-- metadata --><Unexpected/>",
            "<Unexpected></Unexpected>",
            "trailing text",
            "<!-- unterminated",
            "<!-- invalid -- comment -->",
            "<!-- invalid \0 character -->",
            "<!DOCTYPE FlexStatementResponse>",
            "<?xml version=\"1.0\"?>",
            "<?XML?>",
            "<?1trace?>",
            "<??>",
            "<?trace \u{ffff}?>",
        ] {
            assert_matches!(
                parse_send_response(&format!("{SEND_SUCCESS}{tail}")),
                Err(FlexError::Parse(_))
            );
            assert_matches!(
                classify_get_response(format!("{GET_IN_PROGRESS}{tail}")),
                Err(FlexError::Parse(_))
            );
        }
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

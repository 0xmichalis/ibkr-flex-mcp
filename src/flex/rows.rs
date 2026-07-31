//! Shared scanning of Flex statement section rows.
//!
//! Every Flex section (`OpenPositions`, `Trades`, ...) is a flat list of elements whose
//! attributes are present only when the query is configured to include them. This module
//! turns "every `<Tag ...>` in the document" into rows; each section decides the mapping.

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

use super::FlexError;

/// The attributes of one Flex row element.
pub struct Attrs(HashMap<String, String>);

impl Attrs {
    /// The attribute's value, if the query emitted it.
    pub fn text(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    /// The attribute parsed as a number; `None` if absent or not numeric.
    pub fn num(&self, key: &str) -> Option<f64> {
        self.0.get(key).and_then(|v| v.parse::<f64>().ok())
    }
}

/// Parse every `<tag ...>` element in a Flex statement into a row via `row_from`.
pub fn parse_rows<T>(
    xml: &str,
    tag: &str,
    row_from: impl Fn(&Attrs) -> T,
) -> Result<Vec<T>, FlexError> {
    let mut reader = Reader::from_str(xml);
    let mut rows = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == tag.as_bytes() => {
                rows.push(row_from(&attrs_of(&e, tag)?));
            }
            Err(e) => return Err(FlexError::Parse(format!("{tag} XML: {e}"))),
            _ => {}
        }
    }

    Ok(rows)
}

fn attrs_of(e: &BytesStart<'_>, tag: &str) -> Result<Attrs, FlexError> {
    let mut attrs = HashMap::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|err| FlexError::Parse(format!("{tag} attribute: {err}")))?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
        // Flex statements are XML 1.0; we do not read the declaration, so assume it.
        // 1.1 would only add a few more characters to newline normalization.
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|err| FlexError::Parse(format!("{tag} attribute value: {err}")))?
            .into_owned();
        attrs.insert(key, value);
    }
    Ok(Attrs(attrs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_matching_elements_only() {
        let xml = r#"<Root><Trade symbol="AAPL"/><OpenPosition symbol="MSFT"/><Trade symbol="SAP"></Trade></Root>"#;
        let symbols = parse_rows(xml, "Trade", |a| a.text("symbol").unwrap_or_default()).unwrap();
        assert_eq!(symbols, vec!["AAPL", "SAP"]);
    }

    #[test]
    fn unescapes_text_and_parses_numbers() {
        let xml =
            r#"<Root><Trade description="AT&amp;T INC" quantity="-15.5" price="n/a"/></Root>"#;
        let rows = parse_rows(xml, "Trade", |a| {
            (a.text("description"), a.num("quantity"), a.num("price"))
        })
        .unwrap();
        assert_eq!(
            rows,
            vec![(Some("AT&T INC".to_string()), Some(-15.5), None)]
        );
    }

    #[test]
    fn malformed_xml_is_a_parse_error() {
        let err = parse_rows("<Root><Trade symbol=", "Trade", |_| ()).unwrap_err();
        assert!(matches!(err, FlexError::Parse(_)), "got {err:?}");
    }
}

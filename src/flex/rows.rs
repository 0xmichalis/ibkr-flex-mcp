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
    reader.config_mut().trim_text(true);
    let mut rows = Vec::new();
    let mut depth = 0;
    let mut seen_root = false;

    // Each event consumes input; quick-xml checks matching end tags but permits EOF with open tags.
    loop {
        let event = reader
            .read_event()
            .map_err(|err| FlexError::Parse(format!("{tag} XML: {err}")))?;
        let opens_element = matches!(event, Event::Start(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                if depth == 0 && seen_root {
                    return Err(FlexError::Parse(format!("{tag} XML: multiple roots")));
                }
                seen_root = true;
                if opens_element {
                    depth += 1;
                }
                if e.name().as_ref() == tag.as_bytes() {
                    rows.push(row_from(&attrs_of(&e, tag)?));
                }
            }
            Event::End(_) => {
                debug_assert!(depth > 0, "reader must reject unmatched end tags");
                depth -= 1;
            }
            Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) if depth == 0 => {
                return Err(FlexError::Parse(format!("{tag} XML: text outside root")));
            }
            Event::Eof if seen_root && depth == 0 => return Ok(rows),
            Event::Eof => {
                return Err(FlexError::Parse(format!("{tag} XML: no complete root")));
            }
            _ => {}
        }
    }
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
    use std::assert_matches;

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
        for xml in [
            "<Root><Trade symbol=",
            "<Root><Trade symbol=\"AAPL\"/>",
            "<Root><Trade/></Other>",
            "<Root><Trade/></Root><Root/>",
            "<Root><Trade/></Root>trailing text",
            "\n<!-- no root -->\n",
        ] {
            assert_matches!(parse_rows(xml, "Trade", |_| ()), Err(FlexError::Parse(_)));
        }
    }

    #[test]
    fn document_may_include_prolog_and_trailing_whitespace() {
        let xml = "\u{feff}<?xml version=\"1.0\"?>\n<!-- report --><Root><Trade/></Root>\n";
        assert_eq!(parse_rows(xml, "Trade", |_| ()).unwrap(), vec![()]);
    }
}

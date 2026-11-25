//! Shared scanning of Flex statement section rows.
//!
//! Every Flex section (`OpenPositions`, `Trades`, ...) is a flat list of elements whose
//! attributes are present only when the query is configured to include them. This module
//! turns "every `<Tag ...>` in the document" into rows; each section decides the mapping.

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use quick_xml::utils::is_whitespace;

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
    // Leave leading whitespace visible so the XML declaration must be the first event.
    reader.config_mut().check_comments = true;
    let mut rows = Vec::new();
    let mut depth = 0;
    let mut seen_root = false;
    let mut seen_doctype = false;
    let mut at_document_start = true;

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
                } else {
                    e.attributes()
                        .try_for_each(|attr| attr.map(drop))
                        .map_err(|err| FlexError::Parse(format!("{tag} attribute: {err}")))?;
                }
            }
            Event::End(_) => {
                debug_assert!(depth > 0, "reader must reject unmatched end tags");
                depth -= 1;
            }
            Event::Decl(_) if !at_document_start => {
                return Err(FlexError::Parse(format!(
                    "{tag} XML: declaration must be at the start of the document"
                )));
            }
            Event::DocType(_) => {
                if seen_doctype || seen_root {
                    return Err(FlexError::Parse(format!(
                        "{tag} XML: only one DOCTYPE is allowed, before the root"
                    )));
                }
                seen_doctype = true;
            }
            Event::Text(text) if depth == 0 && text.iter().copied().all(is_whitespace) => {}
            Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) if depth == 0 => {
                return Err(FlexError::Parse(format!("{tag} XML: text outside root")));
            }
            Event::Eof if seen_root && depth == 0 => return Ok(rows),
            Event::Eof => {
                return Err(FlexError::Parse(format!("{tag} XML: no complete root")));
            }
            _ => {}
        }
        at_document_start = false;
    }
}

fn attrs_of(e: &BytesStart<'_>, tag: &str) -> Result<Attrs, FlexError> {
    let mut attrs = HashMap::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|err| FlexError::Parse(format!("{tag} attribute: {err}")))?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
        // Flex statements use XML 1.0 attribute value normalization.
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
    fn rejects_malformed_trailing_comments() {
        for xml in [
            "<Root><Trade/></Root><!-- bad--comment -->",
            "<Root><Trade/></Root><!-- unterminated",
        ] {
            let result = parse_rows(xml, "Trade", |_| ());
            assert!(
                matches!(result, Err(FlexError::Parse(_))),
                "{xml}: {result:?}"
            );
        }
    }

    #[test]
    fn rejects_attribute_errors_on_nonmatching_elements() {
        for xml in [
            r#"<Root broken><Trade symbol="AAPL"/></Root>"#,
            r#"<Root><Trade/><Other source=unquoted/></Root>"#,
            r#"<Root><Trade/><Other source="one" source="two"/></Root>"#,
        ] {
            let result = parse_rows(xml, "Trade", |_| ());
            assert!(
                matches!(result, Err(FlexError::Parse(_))),
                "{xml}: {result:?}"
            );
        }
    }

    #[test]
    fn rejects_misplaced_or_duplicate_declarations_and_doctypes() {
        for xml in [
            "<Root><Trade/></Root><?xml version='1.0'?>",
            "<Root><?xml version='1.0'?><Trade/></Root>",
            "\n<?xml version='1.0'?><Root/>",
            "<!-- report --><?xml version='1.0'?><Root/>",
            "<?xml version='1.0'?><?xml version='1.0'?><Root/>",
            "<!DOCTYPE Root><?xml version='1.0'?><Root/>",
            "<!DOCTYPE Root><!DOCTYPE Root><Root/>",
            "<Root><!DOCTYPE Root><Trade/></Root>",
            "<Root/><!DOCTYPE Root>",
        ] {
            let result = parse_rows(xml, "Trade", |_| ());
            assert!(
                matches!(result, Err(FlexError::Parse(_))),
                "{xml}: {result:?}"
            );
        }
    }

    #[test]
    fn document_may_include_a_prolog_and_trailing_miscellany() {
        let xml = concat!(
            "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!-- report --><?export ready?><!DOCTYPE Root>\n",
            "<Root source='IBKR'><Trade/></Root>\r\n<!-- end-of-report --><?trace done?>\t",
        );
        assert_eq!(parse_rows(xml, "Trade", |_| ()).unwrap(), vec![()]);

        let empty_xml = "\n<!-- report --><!DOCTYPE Root><Root />\n";
        assert!(parse_rows(empty_xml, "Trade", |_| ()).unwrap().is_empty());
    }
}

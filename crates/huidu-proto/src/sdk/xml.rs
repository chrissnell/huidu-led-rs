//! Thin `quick-xml` helpers so message bodies never build XML by hand.
//!
//! [`XmlWriter`] wraps `quick_xml::Writer` with the handful of shapes the SDK
//! uses — self-closing attribute elements, open/close pairs, text elements —
//! and escapes every attribute value and text node. [`elements`] walks a reply
//! and hands each start/empty element to a visitor, mirroring the lenient,
//! scan-for-known-tags parsing the reference firmware tolerates.

use crate::error::ProtoError;
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};
use quick_xml::name::QName;
use quick_xml::{Reader, Writer};
use std::io::Write;

/// Builds an SDK XML document element by element. All escaping is handled by
/// `quick-xml`; callers pass raw attribute/text values.
pub struct XmlWriter {
    inner: Writer<Vec<u8>>,
}

impl Default for XmlWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl XmlWriter {
    /// A fresh writer with an empty buffer.
    pub fn new() -> Self {
        Self {
            inner: Writer::new(Vec::new()),
        }
    }

    /// Write the `<?xml version="1.0" encoding="utf-8"?>` declaration.
    pub fn decl(&mut self) -> Result<&mut Self, ProtoError> {
        self.inner
            .write_event(Event::Decl(BytesDecl::new("1.0", Some("utf-8"), None)))?;
        Ok(self)
    }

    /// Write a self-closing element: `<tag k="v" .../>`.
    pub fn empty(&mut self, tag: &str, attrs: &[(&str, &str)]) -> Result<&mut Self, ProtoError> {
        let mut e = BytesStart::new(tag);
        for (k, v) in attrs {
            e.push_attribute((*k, *v));
        }
        self.inner.write_event(Event::Empty(e))?;
        Ok(self)
    }

    /// Write an opening tag: `<tag k="v" ...>`.
    pub fn open(&mut self, tag: &str, attrs: &[(&str, &str)]) -> Result<&mut Self, ProtoError> {
        let mut e = BytesStart::new(tag);
        for (k, v) in attrs {
            e.push_attribute((*k, *v));
        }
        self.inner.write_event(Event::Start(e))?;
        Ok(self)
    }

    /// Write a closing tag: `</tag>`.
    pub fn close(&mut self, tag: &str) -> Result<&mut Self, ProtoError> {
        self.inner.write_event(Event::End(BytesEnd::new(tag)))?;
        Ok(self)
    }

    /// Inject an already well-formed XML fragment verbatim (e.g. a screen tree
    /// serialized elsewhere). No escaping: the fragment must be valid XML.
    pub fn raw(&mut self, xml: &str) -> Result<&mut Self, ProtoError> {
        self.inner
            .get_mut()
            .write_all(xml.as_bytes())
            .map_err(|e| ProtoError::Xml(e.to_string()))?;
        Ok(self)
    }

    /// Consume the writer and return the finished document bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.inner.into_inner()
    }
}

/// Strip a leading UTF-8 BOM some firmware prepends, then borrow as `str`.
pub fn as_str(bytes: &[u8]) -> Result<&str, ProtoError> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    std::str::from_utf8(bytes).map_err(|e| ProtoError::Xml(e.to_string()))
}

/// Call `visit` for every start and empty element in `xml`, in document order.
/// End elements and text are skipped — visitors match on tag name and pull the
/// attributes they care about.
pub fn elements(
    xml: &str,
    mut visit: impl FnMut(&BytesStart) -> Result<(), ProtoError>,
) -> Result<(), ProtoError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) => visit(&e)?,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

/// The local name of an element as bytes, ignoring any namespace prefix.
pub fn local_name<'a>(e: &'a BytesStart) -> &'a [u8] {
    e.local_name().into_inner()
}

/// Fetch one attribute's unescaped value by local name.
pub fn attr(e: &BytesStart, name: &str) -> Result<Option<String>, ProtoError> {
    for a in e.attributes() {
        let a: Attribute = a?;
        if a.key == QName(name.as_bytes()) || a.key.local_name().into_inner() == name.as_bytes() {
            let raw = std::str::from_utf8(&a.value).map_err(|e| ProtoError::Xml(e.to_string()))?;
            let value = quick_xml::escape::unescape(raw)
                .map_err(|e| ProtoError::Xml(e.to_string()))?
                .into_owned();
            return Ok(Some(value));
        }
    }
    Ok(None)
}

/// Return the first `<tag>…</tag>` element (with its children) re-serialized as
/// a standalone fragment, or `None` if no such element is present. Used to lift
/// an embedded screen tree back out of a decoded program envelope.
pub fn extract_element(xml: &str, tag: &str) -> Result<Option<String>, ProtoError> {
    let mut reader = Reader::from_str(xml);
    let mut writer = Writer::new(Vec::new());
    let target = tag.as_bytes();
    let mut depth = 0usize;
    loop {
        let ev = reader.read_event()?;
        match &ev {
            Event::Start(e) => {
                if depth == 0 {
                    if e.local_name().into_inner() == target {
                        depth = 1;
                        writer.write_event(ev.borrow())?;
                    }
                } else {
                    depth += 1;
                    writer.write_event(ev.borrow())?;
                }
            }
            Event::Empty(e) => {
                if depth == 0 {
                    if e.local_name().into_inner() == target {
                        writer.write_event(ev.borrow())?;
                        return Ok(Some(finish(writer)?));
                    }
                } else {
                    writer.write_event(ev.borrow())?;
                }
            }
            Event::End(_) if depth > 0 => {
                writer.write_event(ev.borrow())?;
                depth -= 1;
                if depth == 0 {
                    return Ok(Some(finish(writer)?));
                }
            }
            Event::Eof => break,
            _ if depth > 0 => writer.write_event(ev.borrow())?,
            _ => {}
        }
    }
    Ok(None)
}

fn finish(writer: Writer<Vec<u8>>) -> Result<String, ProtoError> {
    String::from_utf8(writer.into_inner()).map_err(|e| ProtoError::Xml(e.to_string()))
}

/// Parse an SDK boolean the lenient way the firmware writes them (`true`/`false`,
/// any case).
pub fn parse_bool(s: &str) -> bool {
    s.eq_ignore_ascii_case("true")
}

/// Render a bool as the lowercase string the firmware expects.
pub fn bool_str(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

//! Read-only ACE XML gauge documents. Unknown content remains in the tree;
//! decoding a document does not mean its scripts or visual elements are supported.
use crate::cabinet::{resource_key, Cabinet};
use quick_xml::{
    events::{BytesStart, Event},
    Reader, XmlVersion,
};
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, String>;
const MAX_XML: usize = 1024 * 1024;
const MAX_NODES: usize = 8192;
const MAX_DEPTH: usize = 64;

fn valid_characters(text: &str) -> Result<()> {
    if text.chars().all(|c| matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')) {
        Ok(())
    } else { Err("Invalid XML 1.0 character".into()) }
}

#[derive(Debug)]
pub struct Element {
    pub name: String,
    pub attributes: BTreeMap<String, String>,
    pub text: String,
    pub children: Vec<Element>,
}
impl Element {
    pub fn child(&self, name: &str) -> Result<Option<&Self>> {
        let mut matching = self.children.iter().filter(|e| e.name == name);
        let first = matching.next();
        if matching.next().is_some() {
            return Err(format!("Duplicate XML gauge property {name}"));
        }
        Ok(first)
    }
}

#[derive(Debug)]
pub struct GaugeDocument {
    root: Element,
}

fn start(element: BytesStart<'_>, reader: &Reader<&[u8]>) -> Result<Element> {
    let name = std::str::from_utf8(element.name().as_ref())
        .map_err(|e| e.to_string())?
        .to_owned();
    let mut attributes = BTreeMap::new();
    for item in element.attributes() {
        let item = item.map_err(|e| e.to_string())?;
        let key = std::str::from_utf8(item.key.as_ref())
            .map_err(|e| e.to_string())?
            .to_owned();
        let value = item
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|e| e.to_string())?
            .into_owned();
        valid_characters(&value)?;
        if attributes.insert(key, value).is_some() {
            return Err("Duplicate XML attribute".into());
        }
    }
    Ok(Element {
        name,
        attributes,
        text: String::new(),
        children: Vec::new(),
    })
}

fn finish(stack: &mut Vec<Element>, root: &mut Option<Element>) -> Result<()> {
    let node = stack.pop().ok_or("Unmatched XML end tag")?;
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.replace(node).is_some() {
        return Err("Multiple XML roots".into());
    }
    Ok(())
}

impl GaugeDocument {
    pub fn from_cabinet(cabinet: &Cabinet, resource: &str, windows_1252: bool) -> Result<Self> {
        Self::parse(cabinet.resource(resource)?, windows_1252)
    }

    pub fn parse(data: &[u8], windows_1252: bool) -> Result<Self> {
        if data.len() > MAX_XML {
            return Err("XML gauge exceeds 1 MiB".into());
        }
        let text = fsx_cfg::decode(data, windows_1252)?;
        valid_characters(&text)?;
        let mut reader = Reader::from_str(text.trim_start_matches('\u{feff}'));
        let mut stack = Vec::<Element>::new();
        let mut root = None;
        let mut count = 0;
        let mut declaration = false;
        loop {
            match reader.read_event().map_err(|e| format!("XML gauge: {e}"))? {
                event @ (Event::Start(_) | Event::Empty(_)) => {
                    let empty = matches!(&event, Event::Empty(_));
                    let (Event::Start(e) | Event::Empty(e)) = event else {
                        unreachable!()
                    };
                    count += 1;
                    if count > MAX_NODES || stack.len() >= MAX_DEPTH {
                        return Err("XML gauge node/depth limit".into());
                    }
                    stack.push(start(e, &reader)?);
                    if empty {
                        finish(&mut stack, &mut root)?;
                    }
                }
                Event::End(_) => finish(&mut stack, &mut root)?,
                Event::Text(t) => {
                    let value = t
                        .xml_content(XmlVersion::Implicit1_0)
                        .map_err(|e| e.to_string())?;
                    if let Some(node) = stack.last_mut() {
                        node.text.push_str(&value);
                    } else if !value.trim().is_empty() {
                        return Err("Text outside XML root".into());
                    }
                }
                Event::CData(t) => {
                    let value = t
                        .xml_content(XmlVersion::Implicit1_0)
                        .map_err(|e| e.to_string())?;
                    stack
                        .last_mut()
                        .ok_or("CDATA outside XML root")?
                        .text
                        .push_str(&value);
                }
                Event::GeneralRef(r) => {
                    let reference = format!("&{};", r.decode().map_err(|e| e.to_string())?);
                    let value =
                        quick_xml::escape::unescape(&reference).map_err(|e| e.to_string())?;
                    valid_characters(&value)?;
                    stack
                        .last_mut()
                        .ok_or("Entity outside XML root")?
                        .text
                        .push_str(&value);
                }
                Event::Decl(d) => {
                    if declaration || count != 0 {
                        return Err("Misplaced XML declaration".into());
                    }
                    declaration = true;
                    if d.xml_version().map_err(|e| e.to_string())? != XmlVersion::Explicit1_0 {
                        return Err("Only XML 1.0 gauge documents are supported".into());
                    }
                    if let Some(encoding) = d.encoding() {
                        let encoding = encoding.map_err(|e| e.to_string())?;
                        if !encoding.eq_ignore_ascii_case(b"utf-8")
                            && !(windows_1252 && encoding.eq_ignore_ascii_case(b"windows-1252"))
                        {
                            return Err("Unsupported XML gauge encoding".into());
                        }
                    }
                }
                Event::Comment(_) => (),
                Event::DocType(_) | Event::PI(_) => {
                    return Err("XML DTD and processing instructions are unsupported".into())
                }
                Event::Eof => break,
            }
        }
        if !stack.is_empty() {
            return Err("Unclosed XML gauge elements".into());
        }
        let document = Self {
            root: root.ok_or("Missing XML gauge root")?,
        };
        if document.root.name != "SimBase.Document"
            || document.root.attributes.get("Type").map(String::as_str) != Some("AceXML")
            || document.root.child("SimGauge.Gauge")?.is_none()
        {
            return Err("Expected ACE SimBase.Document with one SimGauge.Gauge".into());
        }
        Ok(document)
    }

    pub fn gauge(&self) -> &Element {
        self.root
            .children
            .iter()
            .find(|e| e.name == "SimGauge.Gauge")
            .unwrap()
    }

    pub fn root(&self) -> &Element {
        &self.root
    }

    /// Authored bitmap names only, resolved relative to this XML member's directory.
    /// ArtDirectory is a build-machine hint and is never followed.
    pub fn image_resources(&self, member: &str) -> Result<Vec<String>> {
        let member = resource_key(member)?;
        let directory = member.rsplit_once('/').map(|v| v.0);
        let mut output = Vec::new();
        fn walk(node: &Element, directory: Option<&str>, output: &mut Vec<String>) -> Result<()> {
            if node.name == "Image" {
                let name = node.attributes.get("Name").ok_or("XML Image lacks Name")?;
                let name = resource_key(name)?;
                output.push(match directory {
                    Some(d) => format!("{d}/{name}"),
                    None => name,
                });
            }
            for child in &node.children {
                walk(child, directory, output)?;
            }
            Ok(())
        }
        walk(self.gauge(), directory, &mut output)?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wrap(body: &str) -> String {
        format!(
            r#"<SimBase.Document Type="AceXML"><SimGauge.Gauge id="synthetic" ArtDirectory="C:\ignored">{body}</SimGauge.Gauge></SimBase.Document>"#
        )
    }
    #[test]
    fn preserves_scripts_unknown_nodes_and_contained_images() {
        let xml = wrap(
            r#"<Size>90,30</Size><Image Name="face.bmp"/><Element><GaugeText><GaugeString>%(1 2 &lt;) &amp; &#65;<![CDATA[<raw>]]></GaugeString></GaugeText></Element><Future value="&quot;"/>"#,
        );
        let doc = GaugeDocument::parse(xml.as_bytes(), false).unwrap();
        assert_eq!(doc.gauge().child("Size").unwrap().unwrap().text, "90,30");
        let script = doc
            .gauge()
            .child("Element")
            .unwrap()
            .unwrap()
            .child("GaugeText")
            .unwrap()
            .unwrap()
            .child("GaugeString")
            .unwrap()
            .unwrap();
        assert_eq!(script.text, "%(1 2 <) & A<raw>");
        assert_eq!(
            doc.image_resources("Folder/gauge.xml").unwrap(),
            ["folder/face.bmp"]
        );
        assert!(doc.gauge().child("Future").unwrap().is_some());
    }
    #[test]
    fn rejects_malformed_entities_dtd_duplicates_depth_and_truncation() {
        for xml in [
            wrap("<a></b>"),
            wrap("<a>&external;</a>"),
            wrap("<a>&#1;</a>"),
            wrap("<a x='&#xFFFE;'/>"),
            wrap("<a x='1' x='2'/>"),
            format!(
                "<!DOCTYPE a [<!ENTITY x SYSTEM 'file:///secret'>]>{}",
                wrap("")
            ),
            format!("{}{}", wrap(""), wrap("")),
            wrap(&format!("{}{}", "<a>".repeat(65), "</a>".repeat(65))),
        ] {
            assert!(
                GaugeDocument::parse(xml.as_bytes(), false).is_err(),
                "{xml}"
            );
        }
        let xml = wrap("<Image Name='x.bmp'/>");
        for n in 0..xml.len() {
            assert!(
                GaugeDocument::parse(&xml.as_bytes()[..n], false).is_err(),
                "{n}"
            );
        }
        let duplicate = wrap("<Size>1,2</Size><Size>3,4</Size>");
        assert!(GaugeDocument::parse(duplicate.as_bytes(), false)
            .unwrap()
            .gauge()
            .child("Size")
            .is_err());
        let escape = wrap("<Image Name='../outside.bmp'/>");
        assert!(GaugeDocument::parse(escape.as_bytes(), false)
            .unwrap()
            .image_resources("x.xml")
            .is_err());
    }
}

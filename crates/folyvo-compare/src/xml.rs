//! Reading XML parts safely: no DTDs, no entities beyond the five XML defines, no recursion.

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::{Budget, Unreadable};

/// One XML event, with names reduced to their local part (`w:p` → `p`).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Ev {
    Start(String, Vec<(String, String)>),
    /// An element with no content (`<w:tab/>`): its start with no end to follow.
    Empty(String, Vec<(String, String)>),
    End(String),
    Text(String),
}

/// Deeper than this is not a real document; it only costs memory and time.
pub(crate) const MAX_DEPTH: usize = 256;

pub(crate) struct Xml<'a> {
    reader: Reader<&'a [u8]>,
    events: u32,
    depth: usize,
}

impl<'a> Xml<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Xml<'a> {
        Xml { reader: Reader::from_reader(bytes), events: 0, depth: 0 }
    }

    /// The next event, `None` at the end. A DTD, an unknown entity or broken XML is `Malformed`.
    pub(crate) fn next(&mut self, budget: &mut Budget) -> Result<Option<Ev>, Unreadable> {
        loop {
            self.events += 1;
            if self.events.is_multiple_of(1000) {
                budget.tick()?;
            }
            let event = self.reader.read_event().map_err(|_| Unreadable::Malformed)?;
            return Ok(Some(match event {
                Event::Start(e) => {
                    self.depth += 1;
                    if self.depth > MAX_DEPTH {
                        return Err(Unreadable::TooLarge);
                    }
                    Ev::Start(local(e.name().as_ref())?, attributes(&e)?)
                }
                Event::Empty(e) => Ev::Empty(local(e.name().as_ref())?, attributes(&e)?),
                Event::End(e) => {
                    self.depth = self.depth.saturating_sub(1);
                    Ev::End(local(e.name().as_ref())?)
                }
                Event::Text(t) => Ev::Text(t.xml10_content().into_owned()),
                Event::CData(t) => Ev::Text(t.into_inner().into_owned()),
                Event::GeneralRef(r) => Ev::Text(entity(&r)?.to_string()),
                Event::DocType(_) => return Err(Unreadable::Malformed),
                // A part cut off before its elements close is broken, not short.
                Event::Eof if self.depth > 0 => return Err(Unreadable::Malformed),
                Event::Eof => return Ok(None),
                Event::Comment(_) | Event::Decl(_) | Event::PI(_) => continue,
            }));
        }
    }
}

fn local(name: &str) -> Result<String, Unreadable> {
    Ok(name.rsplit(':').next().unwrap_or(name).to_string())
}

fn attributes(e: &quick_xml::events::BytesStart<'_>) -> Result<Vec<(String, String)>, Unreadable> {
    e.attributes()
        .map(|a| {
            let a = a.map_err(|_| Unreadable::Malformed)?;
            let value =
                a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|_| Unreadable::Malformed)?;
            Ok((local(a.key.as_ref())?, value.into_owned()))
        })
        .collect()
}

fn entity(r: &quick_xml::events::BytesRef<'_>) -> Result<char, Unreadable> {
    if let Some(c) = r.resolve_char_ref().map_err(|_| Unreadable::Malformed)? {
        return Ok(c);
    }
    match r.as_ref() {
        "amp" => Ok('&'),
        "lt" => Ok('<'),
        "gt" => Ok('>'),
        "quot" => Ok('"'),
        "apos" => Ok('\''),
        _ => Err(Unreadable::Malformed),
    }
}

/// The value of attribute `name` (local name) in `attrs`.
pub(crate) fn attr<'v>(attrs: &'v [(String, String)], name: &str) -> Option<&'v str> {
    attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

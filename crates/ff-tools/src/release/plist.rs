//! XML property lists: enough to write an app's Info.plist and read the
//! ones Apple's tools hand back (an asset catalog's partial Info.plist, a
//! provisioning profile's entitlements).

use crate::pyjson::{Object, Value};

/// A plist value. `Data` keeps its base64 text as written.
#[derive(Clone, Debug, PartialEq)]
pub enum Plist {
    Bool(bool),
    Int(i64),
    Real(f64),
    String(String),
    Data(String),
    Date(String),
    Array(Vec<Plist>),
    Dict(Vec<(String, Plist)>),
}

impl Plist {
    pub fn dict(entries: Vec<(&str, Plist)>) -> Self {
        Plist::Dict(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    pub fn string(s: impl Into<String>) -> Self {
        Plist::String(s.into())
    }

    pub fn strings(items: &[&str]) -> Self {
        Plist::Array(items.iter().map(|s| Plist::string(*s)).collect())
    }

    pub fn get(&self, key: &str) -> Option<&Plist> {
        match self {
            Plist::Dict(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Merge another dict's keys into this one (`dict.update`).
    pub fn update(&mut self, other: Plist) {
        if let (Plist::Dict(mine), Plist::Dict(theirs)) = (self, other) {
            for (k, v) in theirs {
                match mine.iter_mut().find(|(mk, _)| *mk == k) {
                    Some(slot) => slot.1 = v,
                    None => mine.push((k, v)),
                }
            }
        }
    }

    /// A JSON-shaped copy, for printing or inspection.
    #[allow(dead_code)]
    pub fn to_value(&self) -> Value {
        match self {
            Plist::Bool(b) => Value::Bool(*b),
            Plist::Int(i) => Value::Int(*i as i128),
            Plist::Real(f) => Value::Float(*f),
            Plist::String(s) | Plist::Data(s) | Plist::Date(s) => Value::Str(s.clone()),
            Plist::Array(a) => Value::Array(a.iter().map(Plist::to_value).collect()),
            Plist::Dict(d) => {
                let mut o = Object::new();
                for (k, v) in d {
                    o.insert(k.clone(), v.to_value());
                }
                Value::Object(o)
            }
        }
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn write_value(value: &Plist, depth: usize, sort: bool, out: &mut String) {
    let pad = "\t".repeat(depth);
    match value {
        Plist::Bool(true) => out.push_str(&format!("{pad}<true/>\n")),
        Plist::Bool(false) => out.push_str(&format!("{pad}<false/>\n")),
        Plist::Int(i) => out.push_str(&format!("{pad}<integer>{i}</integer>\n")),
        Plist::Real(f) => out.push_str(&format!(
            "{pad}<real>{}</real>\n",
            crate::pyjson::float_repr(*f)
        )),
        Plist::String(s) => out.push_str(&format!("{pad}<string>{}</string>\n", escape(s))),
        Plist::Data(s) => out.push_str(&format!("{pad}<data>\n{pad}{}\n{pad}</data>\n", s.trim())),
        Plist::Date(s) => out.push_str(&format!("{pad}<date>{s}</date>\n")),
        Plist::Array(items) if items.is_empty() => out.push_str(&format!("{pad}<array/>\n")),
        Plist::Array(items) => {
            out.push_str(&format!("{pad}<array>\n"));
            for item in items {
                write_value(item, depth + 1, sort, out);
            }
            out.push_str(&format!("{pad}</array>\n"));
        }
        Plist::Dict(entries) if entries.is_empty() => out.push_str(&format!("{pad}<dict/>\n")),
        Plist::Dict(entries) => {
            let mut entries: Vec<&(String, Plist)> = entries.iter().collect();
            if sort {
                entries.sort_by(|a, b| a.0.cmp(&b.0));
            }
            out.push_str(&format!("{pad}<dict>\n"));
            for (k, v) in entries {
                out.push_str(&format!("{pad}\t<key>{}</key>\n", escape(k)));
                write_value(v, depth + 1, sort, out);
            }
            out.push_str(&format!("{pad}</dict>\n"));
        }
    }
}

/// An XML plist document, keys sorted as `plistlib.dump` sorts them.
pub fn to_xml(value: &Plist) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n",
    );
    write_value(value, 0, true, &mut out);
    out.push_str("</plist>\n");
    out
}

struct Reader<'a> {
    s: &'a str,
    pos: usize,
}

enum Tag<'a> {
    Open(&'a str),
    Close(&'a str),
    Empty(&'a str),
}

impl<'a> Reader<'a> {
    /// The next element tag, skipping text, comments, `<?...?>` and `<!...>`.
    fn tag(&mut self) -> Result<Tag<'a>, String> {
        loop {
            let start = self.s[self.pos..]
                .find('<')
                .ok_or("unexpected end of plist")?
                + self.pos;
            let rest = &self.s[start..];
            if rest.starts_with("<!--") {
                let end = rest.find("-->").ok_or("unterminated comment")?;
                self.pos = start + end + 3;
                continue;
            }
            let end = rest.find('>').ok_or("unterminated tag")? + start;
            self.pos = end + 1;
            let inner = &self.s[start + 1..end];
            if inner.starts_with('?') || inner.starts_with('!') {
                continue;
            }
            let name_of = |t: &'a str| t.split_whitespace().next().unwrap_or("");
            return Ok(if let Some(name) = inner.strip_prefix('/') {
                Tag::Close(name_of(name))
            } else if let Some(name) = inner.strip_suffix('/') {
                Tag::Empty(name_of(name))
            } else {
                Tag::Open(name_of(inner))
            });
        }
    }

    /// The text up to `</name>`.
    fn text(&mut self, name: &str) -> Result<String, String> {
        let close = format!("</{name}>");
        let end = self.s[self.pos..]
            .find(&close)
            .ok_or_else(|| format!("unterminated <{name}>"))?
            + self.pos;
        let text = unescape(&self.s[self.pos..end]);
        self.pos = end + close.len();
        Ok(text)
    }

    fn value_from(&mut self, tag: Tag<'a>) -> Result<Plist, String> {
        match tag {
            Tag::Empty("true") => Ok(Plist::Bool(true)),
            Tag::Empty("false") => Ok(Plist::Bool(false)),
            Tag::Empty("array") => Ok(Plist::Array(Vec::new())),
            Tag::Empty("dict") => Ok(Plist::Dict(Vec::new())),
            Tag::Empty("string") => Ok(Plist::String(String::new())),
            Tag::Open("string") => Ok(Plist::String(self.text("string")?)),
            Tag::Open("integer") => self
                .text("integer")?
                .trim()
                .parse()
                .map(Plist::Int)
                .map_err(|e| format!("bad integer: {e}")),
            Tag::Open("real") => self
                .text("real")?
                .trim()
                .parse()
                .map(Plist::Real)
                .map_err(|e| format!("bad real: {e}")),
            Tag::Open("data") => Ok(Plist::Data(self.text("data")?.split_whitespace().collect())),
            Tag::Open("date") => Ok(Plist::Date(self.text("date")?)),
            Tag::Open("array") => {
                let mut items = Vec::new();
                loop {
                    match self.tag()? {
                        Tag::Close("array") => return Ok(Plist::Array(items)),
                        other => items.push(self.value_from(other)?),
                    }
                }
            }
            Tag::Open("dict") => {
                let mut entries = Vec::new();
                loop {
                    match self.tag()? {
                        Tag::Close("dict") => return Ok(Plist::Dict(entries)),
                        Tag::Open("key") => {
                            let key = self.text("key")?;
                            let tag = self.tag()?;
                            entries.push((key, self.value_from(tag)?));
                        }
                        _ => return Err("expected <key> in <dict>".into()),
                    }
                }
            }
            Tag::Open(other) | Tag::Empty(other) | Tag::Close(other) => {
                Err(format!("unexpected <{other}> in plist"))
            }
        }
    }
}

/// Parse an XML plist document.
pub fn from_xml(text: &str) -> Result<Plist, String> {
    let mut reader = Reader { s: text, pos: 0 };
    loop {
        match reader.tag()? {
            Tag::Open("plist") => {
                let tag = reader.tag()?;
                return reader.value_from(tag);
            }
            Tag::Empty("plist") => return Ok(Plist::Dict(Vec::new())),
            _ => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let value = Plist::dict(vec![
            ("b", Plist::Bool(true)),
            ("a", Plist::string("x & <y>")),
            ("n", Plist::Int(14)),
            (
                "list",
                Plist::Array(vec![Plist::dict(vec![("ProfileName", Plist::string("E"))])]),
            ),
            ("empty", Plist::Dict(vec![])),
        ]);
        let xml = to_xml(&value);
        assert!(xml.contains("\t<key>a</key>\n\t<string>x &amp; &lt;y&gt;</string>\n"));
        let back = from_xml(&xml).unwrap();
        assert_eq!(back.get("a"), Some(&Plist::string("x & <y>")));
        assert_eq!(back.get("n"), Some(&Plist::Int(14)));
        assert_eq!(back.get("empty"), Some(&Plist::Dict(vec![])));
    }

    #[test]
    fn reads_entitlements_with_comments_and_data() {
        let xml = "<?xml version=\"1.0\"?>\n<!DOCTYPE plist>\n<plist version=\"1.0\"><!-- c --><dict>\
                   <key>Entitlements</key><dict><key>get-task-allow</key><false/>\
                   <key>keychain</key><array><string>T.*</string></array></dict>\
                   <key>DeveloperCertificates</key><array><data>\n  AAAA\n  BBBB\n</data></array></dict></plist>";
        let p = from_xml(xml).unwrap();
        let ent = p.get("Entitlements").unwrap();
        assert_eq!(ent.get("get-task-allow"), Some(&Plist::Bool(false)));
        assert_eq!(
            p.get("DeveloperCertificates"),
            Some(&Plist::Array(vec![Plist::Data("AAAABBBB".into())]))
        );
    }
}

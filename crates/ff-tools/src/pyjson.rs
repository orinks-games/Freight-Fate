//! JSON that reads and writes the way Python's `json` module does.
//!
//! The committed data files were written by `json.dumps`, and the checks
//! that keep them honest (`index-world --check`, the community-station fold)
//! compare bytes. So this keeps what `serde_json::Value` drops: the order
//! keys appeared in, and whether a number was written as an int or a float.
//! [`dumps`] reproduces `json.dumps` with its `indent`, `sort_keys` and
//! `ensure_ascii` options, including Python's float repr.

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

/// A parsed JSON value, ordered as written.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    Object(Object),
}

/// A JSON object: keys in insertion order, as a Python dict keeps them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Object(pub Vec<(String, Value)>);

impl Object {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.0.iter().any(|(k, _)| k == key)
    }

    /// `dict[key] = value`: replaces in place, else appends.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        if let Some(slot) = self.get_mut(&key) {
            *slot = value;
        } else {
            self.0.push((key, value));
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let index = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(index).1)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    // Used by the world commands only.
    #[cfg_attr(not(feature = "world"), allow(dead_code))]
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter().map(|(k, v)| (k, v))
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.0.iter().map(|(_, v)| v)
    }
}

impl Value {
    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    #[cfg_attr(not(feature = "world"), allow(dead_code))]
    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A number (int or float) as f64; `bool` is not a number here.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => i64::try_from(*i).ok(),
            _ => None,
        }
    }

    /// `obj.get(key)` on an object, `None` on anything else.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object().and_then(|o| o.get(key))
    }

    /// Python `str(value)` for the scalar types the tools stringify.
    pub fn py_str(&self) -> String {
        match self {
            Value::Null => "None".into(),
            Value::Bool(true) => "True".into(),
            Value::Bool(false) => "False".into(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => float_repr(*f),
            Value::Str(s) => s.clone(),
            other => dumps(other, &Options::compact()),
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::Int(i as i128)
    }
}

impl From<u64> for Value {
    fn from(i: u64) -> Self {
        Value::Int(i as i128)
    }
}

impl From<usize> for Value {
    fn from(i: usize) -> Self {
        Value::Int(i as i128)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::Float(f)
    }
}

impl From<Object> for Value {
    fn from(o: Object) -> Self {
        Value::Object(o)
    }
}

impl From<Vec<Value>> for Value {
    fn from(a: Vec<Value>) -> Self {
        Value::Array(a)
    }
}

/// Build an [`Object`] from `(key, value)` pairs in order.
#[macro_export]
macro_rules! pyobj {
    ($($key:expr => $value:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut object = $crate::pyjson::Object::new();
        $(object.insert($key, $crate::pyjson::Value::from($value));)*
        object
    }};
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Int(v as i128))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Int(v as i128))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Float(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::Str(v.to_string()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::Str(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        Deserialize::deserialize(d)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(v) = seq.next_element()? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut out = Object::new();
        while let Some((k, v)) = map.next_entry::<String, Value>()? {
            // A repeated key keeps its first position and takes the last
            // value, as a Python dict does.
            out.insert(k, v);
        }
        Ok(Value::Object(out))
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(ValueVisitor)
    }
}

/// `json.loads(text)`.
pub fn loads(text: &str) -> Result<Value, serde_json::Error> {
    let mut de = serde_json::Deserializer::from_str(text);
    let value = Value::deserialize(&mut de)?;
    de.end()?;
    Ok(value)
}

/// Read and parse a JSON file, naming the file in the error.
pub fn load_path(path: &std::path::Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    loads(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The `json.dumps` keyword arguments the tools use.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// `indent=N`; `None` writes one line.
    pub indent: Option<usize>,
    pub sort_keys: bool,
    pub ensure_ascii: bool,
}

impl Options {
    /// `json.dumps(x)`.
    pub fn compact() -> Self {
        Self {
            indent: None,
            sort_keys: false,
            ensure_ascii: true,
        }
    }

    /// `json.dumps(x, indent=n)`.
    pub fn indent(n: usize) -> Self {
        Self {
            indent: Some(n),
            ..Self::compact()
        }
    }

    pub fn sorted(mut self) -> Self {
        self.sort_keys = true;
        self
    }

    pub fn unicode(mut self) -> Self {
        self.ensure_ascii = false;
        self
    }
}

/// `json.dumps(value, **options)`.
pub fn dumps(value: &Value, options: &Options) -> String {
    let mut out = String::new();
    write_value(value, options, 0, &mut out);
    out
}

fn newline(options: &Options, level: usize, out: &mut String) {
    if let Some(n) = options.indent {
        out.push('\n');
        for _ in 0..n * level {
            out.push(' ');
        }
    }
}

fn write_value(value: &Value, options: &Options, level: usize, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Float(f) => out.push_str(&json_float(*f)),
        Value::Str(s) => write_string(s, options.ensure_ascii, out),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(item_separator(options));
                }
                newline(options, level + 1, out);
                write_value(item, options, level + 1, out);
            }
            newline(options, level, out);
            out.push(']');
        }
        Value::Object(object) => {
            if object.is_empty() {
                out.push_str("{}");
                return;
            }
            let mut entries: Vec<&(String, Value)> = object.0.iter().collect();
            if options.sort_keys {
                entries.sort_by(|a, b| a.0.cmp(&b.0));
            }
            out.push('{');
            for (i, (key, item)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(item_separator(options));
                }
                newline(options, level + 1, out);
                write_string(key, options.ensure_ascii, out);
                out.push_str(": ");
                write_value(item, options, level + 1, out);
            }
            newline(options, level, out);
            out.push('}');
        }
    }
}

fn item_separator(options: &Options) -> &'static str {
    if options.indent.is_some() {
        ","
    } else {
        ", "
    }
}

/// A JSON string literal as `json.dumps` writes it.
pub fn write_string(s: &str, ensure_ascii: bool, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if ensure_ascii && !(' '..='~').contains(&c) => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_float(f: f64) -> String {
    if f.is_nan() {
        "NaN".into()
    } else if f.is_infinite() {
        if f > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        float_repr(f)
    }
}

/// Python's `repr(float)`: the shortest digits that round trip, positional
/// for decimal exponents in `[-4, 16)`, else `d.ddde+XX`.
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.into();
    }
    // Rust's `{:e}` is already the shortest round-trip form.
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let decpt = exp + 1;
    let mut out = String::from(sign);
    if !(-3..=16).contains(&decpt) {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let e = decpt - 1;
        out.push('e');
        out.push(if e < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", e.abs()));
    } else if decpt <= 0 {
        out.push_str("0.");
        for _ in 0..(-decpt) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        let decpt = decpt as usize;
        if digits.len() <= decpt {
            out.push_str(&digits);
            for _ in 0..(decpt - digits.len()) {
                out.push('0');
            }
            out.push_str(".0");
        } else {
            out.push_str(&digits[..decpt]);
            out.push('.');
            out.push_str(&digits[decpt..]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_repr_matches_python() {
        for (x, want) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (0.1, "0.1"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (123456789012345680.0, "1.2345678901234568e+17"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (2.5, "2.5"),
            (-33.75, "-33.75"),
            (1e100, "1e+100"),
        ] {
            assert_eq!(float_repr(x), want, "{x}");
        }
    }

    #[test]
    fn dumps_matches_python_layout() {
        let v = loads(r#"{"b": [1, 2.0, {}], "a": "caf\u00e9 \ud83d\ude9a", "c": [], "d": null}"#)
            .unwrap();
        assert_eq!(
            dumps(&v, &Options::indent(2)),
            "{\n  \"b\": [\n    1,\n    2.0,\n    {}\n  ],\n  \"a\": \"caf\\u00e9 \\ud83d\\ude9a\",\n  \"c\": [],\n  \"d\": null\n}"
        );
        assert_eq!(
            dumps(&v, &Options::compact().sorted().unicode()),
            "{\"a\": \"café 🚚\", \"b\": [1, 2.0, {}], \"c\": [], \"d\": null}"
        );
    }

    #[test]
    fn repeated_keys_keep_first_position_last_value() {
        let v = loads(r#"{"a": 1, "b": 2, "a": 3}"#).unwrap();
        assert_eq!(dumps(&v, &Options::compact()), r#"{"a": 3, "b": 2}"#);
    }

    #[test]
    fn control_and_delete_characters() {
        let v = Value::Str("\u{1}\u{7f}é".into());
        assert_eq!(dumps(&v, &Options::compact()), "\"\\u0001\\u007f\\u00e9\"");
        assert_eq!(
            dumps(&v, &Options::compact().unicode()),
            "\"\\u0001\u{7f}é\""
        );
    }
}

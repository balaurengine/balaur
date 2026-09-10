//! The Godot text format, read.
//!
//! `project.godot`, `.tscn` and `.tres` are one grammar: sections introduced
//! by a `[header]` that may carry attributes, then `key = value` lines until
//! the next one. This module answers what a file says and nothing about what
//! it means; the phases that map a scene or a resource are its callers.
//!
//! Binary `.scn` and `.res` are not read. A Godot project can always be
//! resaved as text, and a second decoder would be a second grammar.

use std::collections::BTreeMap;

use anyhow::{Result, bail};

/// One value as the format spells it.
///
/// `Call` is every constructor in one arm — `Vector2(1, 2)`, `Color(...)`,
/// `ExtResource("1_a")`, `PackedFloat32Array(...)`. They differ in what they
/// mean, not in how they parse, and the meaning belongs to the caller.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// `&"name"` or `^"path"`: a StringName or NodePath literal.
    Name(String),
    Array(Vec<Value>),
    /// Kept as pairs rather than a map: a Godot dictionary takes any value as
    /// a key, and its order is what the file will round-trip to.
    Dict(Vec<(Value, Value)>),
    Call {
        name: String,
        args: Vec<Value>,
    },
    /// `Object(InputEventKey, "keycode": 32, …)`: a class name, then named
    /// fields. Its own arm because it is the one constructor whose arguments
    /// are not a value list, and the input map is written entirely in it.
    Object {
        class: String,
        fields: Vec<(String, Value)>,
    },
}

impl Value {
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) | Self::Name(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(n) => Some(*n as f64),
            Self::Float(n) => Some(*n),
            _ => None,
        }
    }

    pub(crate) fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            Self::Float(n) => Some(*n as i64),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The arguments of a constructor of that name, or `None` for anything
    /// else — including a constructor of another name.
    pub(crate) fn call(&self, name: &str) -> Option<&[Value]> {
        match self {
            Self::Call { name: n, args } if n == name => Some(args),
            _ => None,
        }
    }

    /// One named field of an `Object`.
    pub(crate) fn field(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Object { fields, .. } => fields
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// A constructor's arguments as numbers, for the vector and colour kinds
    /// whose arguments are always numeric.
    pub(crate) fn numbers(&self) -> Option<Vec<f64>> {
        match self {
            Self::Call { args, .. } | Self::Array(args) => args.iter().map(Self::as_f64).collect(),
            _ => None,
        }
    }
}

/// One `[header]` and the `key = value` lines under it.
#[derive(Clone, Debug)]
pub(crate) struct Section {
    /// The first word of the header: `node`, `ext_resource`, `gd_scene`, or
    /// in `project.godot` the whole of `[application]`.
    pub kind: String,
    /// The header's own `key=value` pairs.
    pub attributes: BTreeMap<String, Value>,
    /// The body, in file order, because a Godot node's properties are applied
    /// in the order they are written.
    pub fields: Vec<(String, Value)>,
}

impl Section {
    pub(crate) fn attr(&self, key: &str) -> Option<&Value> {
        self.attributes.get(key)
    }

    pub(crate) fn attr_str(&self, key: &str) -> Option<&str> {
        self.attr(key).and_then(Value::as_str)
    }

    pub(crate) fn field(&self, key: &str) -> Option<&Value> {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }
}

/// A parsed file. Keys before the first header — `project.godot` opens with
/// `config_version` — are a section whose `kind` is empty.
#[derive(Clone, Debug, Default)]
pub(crate) struct Document {
    pub sections: Vec<Section>,
}

impl Document {
    /// Every section of a kind, in file order.
    pub(crate) fn each<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Section> {
        self.sections.iter().filter(move |s| s.kind == kind)
    }

    pub(crate) fn first(&self, kind: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.kind == kind)
    }
}

/// Read a `project.godot`, `.tscn` or `.tres`.
pub(crate) fn parse(text: &str) -> Result<Document> {
    let mut scanner = Scanner::new(text);
    let mut document = Document::default();
    scanner.trivia();
    if !scanner.done() && scanner.peek() != Some('[') {
        document.sections.push(Section {
            kind: String::new(),
            attributes: BTreeMap::new(),
            fields: scanner.fields()?,
        });
    }
    while !scanner.done() {
        let section = scanner.section()?;
        document.sections.push(section);
    }
    Ok(document)
}

struct Scanner<'a> {
    text: &'a [u8],
    source: &'a str,
    at: usize,
}

impl<'a> Scanner<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            text: source.as_bytes(),
            source,
            at: 0,
        }
    }

    fn done(&self) -> bool {
        self.at >= self.text.len()
    }

    fn peek(&self) -> Option<char> {
        self.source[self.at..].chars().next()
    }

    fn line(&self) -> usize {
        1 + self.source[..self.at]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += c.len_utf8();
        Some(c)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.at += c.len_utf8();
            return true;
        }
        false
    }

    /// Whitespace and `;` comments. A comment runs to the end of its line and
    /// only ever appears outside a value, which is why this is not in `bump`.
    fn trivia(&mut self) {
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                self.at += 1;
            }
            if self.peek() != Some(';') {
                return;
            }
            while !self.done() && self.peek() != Some('\n') {
                self.bump();
            }
        }
    }

    /// Whitespace within a line, so a `key = value` keeps its newline as the
    /// thing that ends the value.
    fn spaces(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\r')) {
            self.at += 1;
        }
    }

    fn section(&mut self) -> Result<Section> {
        let line = self.line();
        self.bump();
        let kind = self.word();
        if kind.is_empty() {
            bail!("line {line}: a section needs a name after its `[`");
        }
        let mut attributes = BTreeMap::new();
        loop {
            self.spaces();
            if self.eat(']') {
                break;
            }
            if self.done() {
                bail!("line {line}: this [{kind}] never closes");
            }
            let key = self.word();
            if key.is_empty() {
                bail!("line {}: expected an attribute or `]`", self.line());
            }
            self.spaces();
            if !self.eat('=') {
                bail!("line {}: attribute `{key}` has no value", self.line());
            }
            self.spaces();
            attributes.insert(key, self.value()?);
        }
        let fields = self.fields()?;
        Ok(Section {
            kind,
            attributes,
            fields,
        })
    }

    /// `key = value` lines up to the next `[section]` or the end of the file.
    fn fields(&mut self) -> Result<Vec<(String, Value)>> {
        let mut fields = Vec::new();
        loop {
            self.trivia();
            if self.done() || self.peek() == Some('[') {
                return Ok(fields);
            }
            let key = self.word();
            if key.is_empty() {
                bail!(
                    "line {}: expected a `key = value` or a [section]",
                    self.line()
                );
            }
            self.spaces();
            if !self.eat('=') {
                bail!("line {}: `{key}` has no value", self.line());
            }
            self.spaces();
            fields.push((key, self.value()?));
        }
    }

    /// A bare identifier. Keys carry `/` and `.` in `project.godot`
    /// (`config/name`, `rendering/2d/snap`), so both are word characters here.
    fn word(&mut self) -> String {
        let start = self.at;
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || matches!(c, '_' | '/' | '.' | '-') {
                self.at += c.len_utf8();
            } else {
                break;
            }
        }
        self.source[start..self.at].to_string()
    }

    fn value(&mut self) -> Result<Value> {
        self.spaces();
        let line = self.line();
        match self.peek() {
            None => bail!("line {line}: a value was expected and the file ended"),
            Some('"') => Ok(Value::Str(self.string()?)),
            Some('&' | '^') => {
                self.bump();
                Ok(Value::Name(self.string()?))
            }
            Some('[') => {
                self.bump();
                Ok(Value::Array(self.items(']')?))
            }
            Some('{') => {
                self.bump();
                self.pairs()
            }
            Some(c) if c.is_ascii_digit() || c == '-' || c == '+' => self.number(),
            Some(c) if c.is_alphabetic() || c == '_' => {
                let word = self.word();
                match word.as_str() {
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    "null" | "nan" => Ok(Value::Null),
                    "inf" => Ok(Value::Float(f64::INFINITY)),
                    _ => {
                        self.spaces();
                        if !self.eat('(') {
                            bail!("line {line}: `{word}` is not a value this reads");
                        }
                        if word == "Object" {
                            return self.object();
                        }
                        Ok(Value::Call {
                            name: word,
                            args: self.items(')')?,
                        })
                    }
                }
            }
            Some(c) => bail!("line {line}: `{c}` does not begin a value"),
        }
    }

    /// Values until `close`, comma-separated, with a trailing comma allowed
    /// because Godot writes one in a multi-line array.
    fn items(&mut self, close: char) -> Result<Vec<Value>> {
        let mut items = Vec::new();
        loop {
            self.trivia();
            if self.eat(close) {
                return Ok(items);
            }
            if self.done() {
                bail!("line {}: a `{close}` is missing", self.line());
            }
            items.push(self.value()?);
            self.trivia();
            if !self.eat(',') && self.peek() != Some(close) {
                bail!("line {}: expected `,` or `{close}`", self.line());
            }
        }
    }

    /// The inside of an `Object(`, its `(` already eaten: a class name, then
    /// `"key": value` pairs to the `)`.
    fn object(&mut self) -> Result<Value> {
        self.trivia();
        let class = self.word();
        if class.is_empty() {
            bail!("line {}: `Object(` wants a class name", self.line());
        }
        let mut fields = Vec::new();
        loop {
            self.trivia();
            if self.eat(')') {
                return Ok(Value::Object { class, fields });
            }
            if !self.eat(',') {
                bail!("line {}: expected `,` or `)` in an Object", self.line());
            }
            self.trivia();
            if self.eat(')') {
                return Ok(Value::Object { class, fields });
            }
            let key = self.string()?;
            self.trivia();
            if !self.eat(':') {
                bail!("line {}: `{key}` needs a `:` after it", self.line());
            }
            fields.push((key, self.value()?));
        }
    }

    fn pairs(&mut self) -> Result<Value> {
        let mut pairs = Vec::new();
        loop {
            self.trivia();
            if self.eat('}') {
                return Ok(Value::Dict(pairs));
            }
            if self.done() {
                bail!("line {}: a `}}` is missing", self.line());
            }
            let key = self.value()?;
            self.trivia();
            if !self.eat(':') {
                bail!(
                    "line {}: a dictionary key needs a `:` after it",
                    self.line()
                );
            }
            let value = self.value()?;
            pairs.push((key, value));
            self.trivia();
            if !self.eat(',') && self.peek() != Some('}') {
                bail!("line {}: expected `,` or `}}`", self.line());
            }
        }
    }

    /// A quoted string. Godot's escapes are C's, plus `\uXXXX`; an unknown
    /// one keeps the character it escaped, which is what Godot's own reader
    /// does. Newlines inside the quotes are part of the string, which is how
    /// a BBCode label puts a `[b]` at the start of a line.
    fn string(&mut self) -> Result<String> {
        let line = self.line();
        if !self.eat('"') {
            bail!("line {line}: expected a quoted string");
        }
        let mut out = String::new();
        loop {
            let Some(c) = self.bump() else {
                bail!("line {line}: this string never closes");
            };
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(esc) = self.bump() else {
                        bail!("line {line}: this string never closes");
                    };
                    match esc {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'a' => out.push('\u{7}'),
                        'u' => out.push(self.unicode(line)?),
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
    }

    /// The four hex digits after `\u`. A lone surrogate has no character to
    /// stand for, so it becomes the replacement rather than failing the file.
    fn unicode(&mut self, line: usize) -> Result<char> {
        let start = self.at;
        for _ in 0..4 {
            if !self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                bail!("line {line}: `\\u` wants four hex digits");
            }
            self.at += 1;
        }
        let code = u32::from_str_radix(&self.source[start..self.at], 16)?;
        Ok(char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER))
    }

    /// An integer or a float, including the `1e-05` and `-inf` Godot writes.
    fn number(&mut self) -> Result<Value> {
        let line = self.line();
        let start = self.at;
        self.eat('-');
        self.eat('+');
        if self.source[self.at..].starts_with("inf") {
            self.at += 3;
            let text = &self.source[start..self.at];
            return Ok(Value::Float(if text.starts_with('-') {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }));
        }
        let mut float = false;
        while let Some(c) = self.peek() {
            match c {
                '0'..='9' => self.at += 1,
                '.' => {
                    float = true;
                    self.at += 1;
                }
                'e' | 'E' => {
                    float = true;
                    self.at += 1;
                    self.eat('-');
                    self.eat('+');
                }
                _ => break,
            }
        }
        let text = &self.source[start..self.at];
        if float {
            return Ok(Value::Float(text.parse()?));
        }
        match text.parse::<i64>() {
            Ok(n) => Ok(Value::Int(n)),
            // Past i64 rather than malformed: Godot writes hashes as integers.
            Err(_) => match text.parse::<f64>() {
                Ok(n) => Ok(Value::Float(n)),
                Err(_) => bail!("line {line}: `{text}` is not a number"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Value, parse};

    /// One file with every shape the format has: a header with attributes, a
    /// leading key before any section, constructors, a dictionary, a nested
    /// array, and a string that runs over lines and opens one with a `[`.
    const SAMPLE: &str = r#"; a comment
config_version=5

[gd_scene load_steps=3 format=3 uid="uid://cabc"]

[ext_resource type="Texture2D" uid="uid://cdef" path="res://art/hull.png" id="1_a"]

[node name="Ship" type="Sprite2D"]
position = Vector2(-12.5, 1e-05)
modulate = Color(1, 0.5, 0, 1)
texture = ExtResource("1_a")
metadata/tags = ["hull", "player"]
speeds = {"slow": 1, "fast": 2.5}
target = NodePath("../Sea")
group = &"boats"
blurb = "one
[b]two[/b]"
lucky = true
missing = null
"#;

    #[test]
    fn every_shape_the_format_has_reads_back() {
        let document = parse(SAMPLE).expect("the sample parses");

        let leading = document.first("").expect("the keys before any section");
        assert_eq!(leading.field("config_version"), Some(&Value::Int(5)));

        let scene = document.first("gd_scene").expect("a [gd_scene]");
        assert_eq!(scene.attr_str("uid"), Some("uid://cabc"));
        assert_eq!(scene.attr("format"), Some(&Value::Int(3)));

        let external = document.first("ext_resource").expect("an [ext_resource]");
        assert_eq!(external.attr_str("path"), Some("res://art/hull.png"));

        let node = document.first("node").expect("a [node]");
        assert_eq!(node.attr_str("type"), Some("Sprite2D"));
        assert_eq!(
            node.field("position").and_then(Value::numbers),
            Some(vec![-12.5, 1e-05])
        );
        assert_eq!(
            node.field("modulate").and_then(Value::numbers),
            Some(vec![1.0, 0.5, 0.0, 1.0])
        );
        assert_eq!(
            node.field("texture")
                .and_then(|v| v.call("ExtResource"))
                .and_then(|args| args.first())
                .and_then(Value::as_str),
            Some("1_a")
        );
        assert_eq!(
            node.field("metadata/tags")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
            Some(vec!["hull", "player"])
        );
        let Some(Value::Dict(speeds)) = node.field("speeds") else {
            panic!("speeds should be a dictionary");
        };
        assert_eq!(speeds.len(), 2);
        assert_eq!(speeds[1].1, Value::Float(2.5));
        assert_eq!(node.field("group").and_then(Value::as_str), Some("boats"));
        assert_eq!(node.field("lucky"), Some(&Value::Bool(true)));
        assert_eq!(node.field("missing"), Some(&Value::Null));
    }

    /// The one that a line-based reader gets wrong: a BBCode label puts a
    /// `[b]` at the start of a line inside a quoted string, and that is text,
    /// not the next section.
    #[test]
    fn a_bracket_inside_a_multi_line_string_is_not_a_section() {
        let document = parse(SAMPLE).unwrap();
        let node = document.first("node").unwrap();
        assert_eq!(
            node.field("blurb").and_then(Value::as_str),
            Some("one\n[b]two[/b]")
        );
        assert_eq!(
            document.sections.len(),
            4,
            "the `[b]` inside the string opened a section: {:?}",
            document
                .sections
                .iter()
                .map(|s| &s.kind)
                .collect::<Vec<_>>()
        );
    }

    /// The input map is written in `Object(Class, "key": value, …)`, which is
    /// the one constructor whose arguments are named rather than positional.
    #[test]
    fn an_object_reads_its_class_and_its_named_fields() {
        let document = parse(
            r#"[input]
jump={
"deadzone": 0.2,
"events": [Object(InputEventKey,"pressed":false,"physical_keycode":32,"script":null)
]
}
"#,
        )
        .expect("the action parses");
        let Some(Value::Dict(pairs)) = document.first("input").unwrap().field("jump") else {
            panic!("an action is a dictionary");
        };
        let events = pairs[1].1.as_array().expect("the events list");
        let Value::Object { class, .. } = &events[0] else {
            panic!("an event is an Object");
        };
        assert_eq!(class, "InputEventKey");
        assert_eq!(
            events[0].field("physical_keycode").and_then(Value::as_i64),
            Some(32)
        );
        assert_eq!(events[0].field("script"), Some(&Value::Null));
    }

    #[test]
    fn a_file_that_is_not_this_format_says_which_line() {
        let why = parse("[node name=\"A\"]\nthis is not a key = value\n")
            .expect_err("a bare sentence is not a field")
            .to_string();
        assert!(why.contains("line 2"), "{why}");
    }
}

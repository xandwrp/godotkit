//! The text resource grammar shared by `.tscn` and `.tres`: `[tag key=value …]`
//! section headers followed by `key = value` property lines. Values follow
//! Godot's variant text format and may span lines.

use super::{
    Connection, ExtResource, FileKind, Properties, SceneFile, SceneNode, SubResource,
    Value,
};
use crate::respath::{NodePath, ResPath, Uid};

pub(super) fn parse(source: &str) -> crate::Result<SceneFile> {
    let mut cursor = Cursor { source, offset: 0 };
    let mut file: Option<SceneFile> = None;
    // Where `key = value` lines currently go.
    let mut target = Target::None;

    loop {
        cursor.skip_blank_and_comments();
        if cursor.at_end() {
            break;
        }
        let line = cursor.line();
        if cursor.peek() == Some('[') {
            let header = cursor.header()?;
            target = Target::None;
            let Some(file) = file.as_mut() else {
                file = Some(start_file(&header, line)?);
                continue;
            };
            match header.tag.as_str() {
                "gd_scene" | "gd_resource" => return Err(error(line, "duplicate file header")),
                "ext_resource" => file.ext_resources.push(ExtResource {
                    id: header.id(line)?,
                    type_name: header.string("type").unwrap_or_default(),
                    path: header.res_path("path", line)?,
                    uid: header.string("uid").map(Uid),
                    line,
                }),
                "sub_resource" => {
                    file.sub_resources.push(SubResource {
                        id: header.id(line)?,
                        type_name: header.string("type").unwrap_or_default(),
                        properties: Properties::new(),
                        line,
                    });
                    target = Target::SubResource;
                }
                "node" => {
                    file.nodes.push(node(&header, line, file.nodes.is_empty())?);
                    target = Target::Node;
                }
                "connection" => file.connections.push(Connection {
                    signal: header.required_string("signal", line)?,
                    from: NodePath(header.required_string("from", line)?),
                    to: NodePath(header.required_string("to", line)?),
                    method: header.required_string("method", line)?,
                    flags: header.int("flags").map(|flags| flags as u32),
                    binds: match header.get("binds") {
                        Some(Value::Array(binds)) => binds.clone(),
                        _ => Vec::new(),
                    },
                    unbinds: header.int("unbinds").unwrap_or(0) as u32,
                    line,
                }),
                "editable" => {
                    file.editable_instances.push(NodePath(header.required_string("path", line)?))
                }
                "resource" => {
                    file.resource.get_or_insert_with(Properties::new);
                    target = Target::Resource;
                }
                other => return Err(error(line, &format!("unknown section [{other}]"))),
            }
            continue;
        }
        let key = cursor.key()?;
        cursor.skip_inline_space();
        if !cursor.eat('=') {
            return Err(error(line, &format!("expected `=` after property `{key}`")));
        }
        let value = cursor.value()?;
        let Some(file) = file.as_mut() else {
            return Err(error(line, "property before the file header"));
        };
        match target {
            Target::None => return Err(error(line, &format!("property `{key}` outside any section"))),
            Target::Node => {
                let node = file.nodes.last_mut().expect("target set after push");
                match key.as_str() {
                    "script" => node.script = value.as_resource_ref(),
                    "unique_name_in_owner" => node.unique_name_in_owner = value == Value::Bool(true),
                    _ => {}
                }
                node.properties.insert(key, value);
            }
            Target::SubResource => {
                file.sub_resources.last_mut().expect("target set after push").properties.insert(key, value);
            }
            Target::Resource => {
                file.resource.as_mut().expect("target set on [resource]").insert(key, value);
            }
        }
    }

    let file = file.ok_or_else(|| error(1, "missing [gd_scene] or [gd_resource] header"))?;
    if file.kind == FileKind::Scene && file.nodes.is_empty() {
        return Err(error(1, "scene contains no nodes"));
    }
    Ok(file)
}

#[derive(Clone, Copy)]
enum Target {
    None,
    Node,
    SubResource,
    Resource,
}

fn error(line: usize, message: &str) -> crate::Error {
    crate::Error::Parse { path: None, line, message: message.to_owned() }
}

fn start_file(header: &Header, line: usize) -> crate::Result<SceneFile> {
    let kind = match header.tag.as_str() {
        "gd_scene" => FileKind::Scene,
        "gd_resource" => FileKind::Resource,
        other => return Err(error(line, &format!("expected [gd_scene] or [gd_resource], found [{other}]"))),
    };
    Ok(SceneFile {
        kind,
        format: header.int("format").unwrap_or(1) as u32,
        uid: header.string("uid").map(Uid),
        load_steps: header.int("load_steps").map(|steps| steps as u32),
        ext_resources: Vec::new(),
        sub_resources: Vec::new(),
        nodes: Vec::new(),
        connections: Vec::new(),
        editable_instances: Vec::new(),
        resource: None,
    })
}

fn node(header: &Header, line: usize, first: bool) -> crate::Result<SceneNode> {
    let name = header.required_string("name", line)?;
    let parent = header.string("parent").map(NodePath);
    match (first, &parent) {
        (true, Some(_)) => return Err(error(line, "the first node must be the root (no parent)")),
        (false, None) => return Err(error(line, &format!("node `{name}` has no parent; only the first node may be the root"))),
        _ => {}
    }
    let groups = match header.get("groups") {
        None => Vec::new(),
        Some(Value::Array(groups)) => groups
            .iter()
            .map(|group| group.as_str().map(str::to_owned))
            .collect::<Option<_>>()
            .ok_or_else(|| error(line, "groups must be an array of strings"))?,
        Some(_) => return Err(error(line, "groups must be an array of strings")),
    };
    let instance = match header.get("instance") {
        None => None,
        Some(value) => Some(value.as_resource_ref().ok_or_else(|| error(line, "instance must be ExtResource(…)"))?),
    };
    let instance_placeholder = match header.string("instance_placeholder") {
        None => None,
        Some(path) => Some(ResPath::parse(&path).map_err(|_| error(line, &format!("invalid instance_placeholder path {path:?}")))?),
    };
    Ok(SceneNode {
        name,
        parent,
        type_name: header.string("type"),
        instance,
        instance_placeholder,
        script: None,
        groups,
        unique_name_in_owner: false,
        properties: Properties::new(),
        line,
    })
}

struct Header {
    tag: String,
    attributes: Vec<(String, Value)>,
}

impl Header {
    fn get(&self, key: &str) -> Option<&Value> {
        self.attributes.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }
    fn string(&self, key: &str) -> Option<String> {
        self.get(key).and_then(Value::as_str).map(str::to_owned)
    }
    fn int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(Value::as_int)
    }
    fn required_string(&self, key: &str, line: usize) -> crate::Result<String> {
        self.string(key).ok_or_else(|| error(line, &format!("[{}] requires a string `{key}`", self.tag)))
    }
    /// String ids (format 3) or integer ids (format 2).
    fn id(&self, line: usize) -> crate::Result<String> {
        match self.get("id") {
            Some(Value::Str(id)) => Ok(id.clone()),
            Some(Value::Int(id)) => Ok(id.to_string()),
            _ => Err(error(line, &format!("[{}] requires an `id`", self.tag))),
        }
    }
    fn res_path(&self, key: &str, line: usize) -> crate::Result<ResPath> {
        let path = self.required_string(key, line)?;
        ResPath::parse(&path).map_err(|_| error(line, &format!("`{key}` is not a valid res:// path: {path:?}")))
    }
}

struct Cursor<'a> {
    source: &'a str,
    offset: usize,
}

impl Cursor<'_> {
    fn rest(&self) -> &str {
        &self.source[self.offset..]
    }
    fn at_end(&self) -> bool {
        self.offset >= self.source.len()
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.offset += c.len_utf8();
        Some(c)
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.offset += c.len_utf8();
            true
        } else {
            false
        }
    }
    fn line(&self) -> usize {
        self.source[..self.offset].matches('\n').count() + 1
    }
    fn err(&self, message: &str) -> crate::Error {
        error(self.line(), message)
    }
    fn skip_inline_space(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.offset += 1;
        }
    }
    /// Whitespace including newlines. Inside values, newlines are insignificant.
    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\r' | '\n' | '\u{feff}')) {
            self.bump();
        }
    }
    fn skip_blank_and_comments(&mut self) {
        loop {
            self.skip_space();
            if self.peek() == Some(';') {
                while !matches!(self.peek(), None | Some('\n')) {
                    self.bump();
                }
            } else {
                return;
            }
        }
    }

    fn header(&mut self) -> crate::Result<Header> {
        let line = self.line();
        self.eat('[');
        let tag = self.identifier();
        if tag.is_empty() {
            return Err(error(line, "section header without a tag"));
        }
        let mut attributes = Vec::new();
        loop {
            self.skip_inline_space();
            match self.peek() {
                Some(']') => {
                    self.bump();
                    break;
                }
                None | Some('\n' | '\r') => return Err(error(line, &format!("unterminated [{tag}] header"))),
                _ => {}
            }
            let key = self.identifier();
            if key.is_empty() {
                return Err(error(line, &format!("malformed attribute in [{tag}] header")));
            }
            self.skip_inline_space();
            if !self.eat('=') {
                return Err(error(line, &format!("expected `=` after `{key}` in [{tag}] header")));
            }
            self.skip_inline_space();
            let value = self.value()?;
            attributes.push((key, value));
        }
        self.skip_inline_space();
        if !matches!(self.peek(), None | Some('\n' | '\r' | ';')) {
            return Err(error(line, &format!("unexpected text after [{tag}] header")));
        }
        Ok(Header { tag, attributes })
    }

    fn identifier(&mut self) -> String {
        let start = self.offset;
        while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_') {
            self.bump();
        }
        self.source[start..self.offset].to_owned()
    }

    /// A property name: everything up to ` =`, or a quoted string.
    fn key(&mut self) -> crate::Result<String> {
        if self.peek() == Some('"') {
            return self.string();
        }
        let start = self.offset;
        while !matches!(self.peek(), None | Some('=' | '\n' | '\r')) {
            self.bump();
        }
        let key = self.source[start..self.offset].trim_end();
        if key.is_empty() {
            return Err(self.err("expected a property name"));
        }
        Ok(key.to_owned())
    }

    fn value(&mut self) -> crate::Result<Value> {
        self.skip_space();
        match self.peek() {
            None => Err(self.err("expected a value, found end of file")),
            Some('"') => self.string().map(Value::Str),
            Some('&') => {
                self.bump();
                self.string().map(Value::StringName)
            }
            Some('^') => {
                self.bump();
                let path = self.string()?;
                Ok(Value::Call { name: "NodePath".into(), args: vec![Value::Str(path)] })
            }
            Some('[') => {
                self.bump();
                self.sequence(']').map(Value::Array)
            }
            Some('{') => {
                self.bump();
                self.dictionary().map(Value::Dict)
            }
            Some(c) if c.is_ascii_digit() || matches!(c, '-' | '+' | '.') => self.number(),
            Some(c) if c.is_alphabetic() || c == '_' => self.word(),
            Some(c) => Err(self.err(&format!("unexpected `{c}` in value"))),
        }
    }

    fn string(&mut self) -> crate::Result<String> {
        let line = self.line();
        self.eat('"');
        let mut value = String::new();
        loop {
            match self.bump() {
                None => return Err(error(line, "unterminated string")),
                Some('"') => return Ok(value),
                Some('\\') => match self.bump() {
                    Some('n') => value.push('\n'),
                    Some('t') => value.push('\t'),
                    Some('r') => value.push('\r'),
                    Some('b') => value.push('\u{8}'),
                    Some('f') => value.push('\u{c}'),
                    Some(digits @ ('u' | 'U')) => {
                        let count = if digits == 'u' { 4 } else { 6 };
                        let start = self.offset;
                        for _ in 0..count {
                            self.bump();
                        }
                        let hex = &self.source[start..self.offset];
                        let c = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
                        value.push(c.ok_or_else(|| self.err("invalid unicode escape"))?);
                    }
                    Some(other) => value.push(other),
                    None => return Err(error(line, "unterminated string")),
                },
                Some(c) => value.push(c),
            }
        }
    }

    fn sequence(&mut self, close: char) -> crate::Result<Vec<Value>> {
        let mut items = Vec::new();
        loop {
            self.skip_space();
            if self.eat(close) {
                return Ok(items);
            }
            items.push(self.value()?);
            self.skip_space();
            // `Object(Type, "prop": value, …)` pairs flatten into the argument list.
            if close == ')' && self.eat(':') {
                items.push(self.value()?);
                self.skip_space();
            }
            if self.eat(close) {
                return Ok(items);
            }
            if !self.eat(',') {
                return Err(self.err(&format!("expected `,` or `{close}`")));
            }
        }
    }

    fn dictionary(&mut self) -> crate::Result<Vec<(Value, Value)>> {
        let mut entries = Vec::new();
        loop {
            self.skip_space();
            if self.eat('}') {
                return Ok(entries);
            }
            let key = self.value()?;
            self.skip_space();
            if !self.eat(':') {
                return Err(self.err("expected `:` in dictionary"));
            }
            let value = self.value()?;
            entries.push((key, value));
            self.skip_space();
            if self.eat('}') {
                return Ok(entries);
            }
            if !self.eat(',') {
                return Err(self.err("expected `,` or `}` in dictionary"));
            }
        }
    }

    fn number(&mut self) -> crate::Result<Value> {
        let start = self.offset;
        if matches!(self.peek(), Some('-' | '+')) {
            self.bump();
        }
        if self.peek().is_some_and(char::is_alphabetic) {
            // -inf, +inf
            let word = self.identifier();
            let negative = self.source[start..].starts_with('-');
            return match word.as_str() {
                "inf" if negative => Ok(Value::Float(f64::NEG_INFINITY)),
                "inf" => Ok(Value::Float(f64::INFINITY)),
                _ => Err(self.err(&format!("invalid number `{}`", &self.source[start..self.offset]))),
            };
        }
        let mut float = false;
        while let Some(c) = self.peek() {
            match c {
                '0'..='9' => {}
                '.' | 'e' | 'E' => float = true,
                '-' | '+' if matches!(self.source[..self.offset].chars().last(), Some('e' | 'E')) => {}
                _ => break,
            }
            self.bump();
        }
        let text = &self.source[start..self.offset];
        let invalid = || self.err(&format!("invalid number `{text}`"));
        if float {
            text.parse().map(Value::Float).map_err(|_| invalid())
        } else {
            text.parse().map(Value::Int).map_err(|_| invalid())
        }
    }

    /// `true`, `null`, `inf`, `nan`, or a constructor such as `Vector2(1, 2)`
    /// or `Array[int]([1])`. Type arguments are kept in the name as written.
    fn word(&mut self) -> crate::Result<Value> {
        let mut name = self.identifier();
        match name.as_str() {
            "true" => return Ok(Value::Bool(true)),
            "false" => return Ok(Value::Bool(false)),
            "null" => return Ok(Value::Null),
            "inf" => return Ok(Value::Float(f64::INFINITY)),
            "inf_neg" => return Ok(Value::Float(f64::NEG_INFINITY)),
            "nan" => return Ok(Value::Float(f64::NAN)),
            _ => {}
        }
        if self.peek() == Some('[') {
            let start = self.offset;
            let mut depth = 0;
            while let Some(c) = self.bump() {
                match c {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    '\n' | '\r' => return Err(self.err("unterminated type arguments")),
                    _ => {}
                }
            }
            name.push_str(&self.source[start..self.offset]);
        }
        self.skip_inline_space();
        if !self.eat('(') {
            return Err(self.err(&format!("expected `(` after `{name}`")));
        }
        let args = self.sequence(')')?;
        Ok(Value::Call { name, args })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_values_span_lines() {
        let file = parse("[gd_resource type=\"Resource\" format=3]\n\n[resource]\nitems = [{\n\"a\": Vector2(1, -2.5e-05),\n}, &\"sn\", ^\"A/B\", Array[int]([1, 2]), inf, -inf]\n").unwrap();
        let Value::Array(items) = &file.resource.as_ref().unwrap()["items"] else { panic!() };
        assert_eq!(items.len(), 6);
        assert_eq!(
            items[0],
            Value::Dict(vec![(
                Value::Str("a".into()),
                Value::Call { name: "Vector2".into(), args: vec![Value::Int(1), Value::Float(-2.5e-05)] }
            )])
        );
        assert_eq!(items[3], Value::Call { name: "Array[int]".into(), args: vec![Value::Array(vec![Value::Int(1), Value::Int(2)])] });
        assert_eq!(items[5], Value::Float(f64::NEG_INFINITY));
    }
}

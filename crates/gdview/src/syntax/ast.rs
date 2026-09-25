//! Typed views over [`super::Node`]. Each `cast` returns `Some` only for the
//! matching kind. These are the only syntax accessors `declarations`, `xref`,
//! and `net` use.

use super::{Element, Node, SyntaxKind};

macro_rules! view {
    ($name:ident, $kind:ident) => {
        #[derive(Clone, Copy, Debug)]
        pub struct $name<'a>(pub Node<'a>);
        impl<'a> $name<'a> {
            pub fn cast(node: Node<'a>) -> Option<Self> {
                (node.kind() == SyntaxKind::$kind).then_some(Self(node))
            }
            pub fn node(&self) -> Node<'a> {
                self.0
            }
        }
    };
}

view!(SourceFile, SourceFile);
view!(ClassNameDecl, ClassNameDecl);
view!(SignalDecl, SignalDecl);
view!(ConstDecl, ConstDecl);
view!(VarDecl, VarDecl);
view!(FuncDecl, FuncDecl);
view!(EnumDecl, EnumDecl);
view!(ClassDecl, InnerClassDecl);
view!(Annotation, Annotation);
view!(CallExpr, CallExpr);
view!(Preload, PreloadExpr);

/// Script-level annotations; they never belong to the member that follows them.
const SCRIPT_ANNOTATIONS: &[&str] = &["tool", "icon", "static_unload"];

impl<'a> SourceFile<'a> {
    pub fn class_name(&self) -> Option<ClassNameDecl<'a>> {
        self.0.children().find_map(ClassNameDecl::cast)
    }
    /// `extends X` as its own statement, or inline in `class_name A extends X`.
    pub fn extends(&self) -> Option<ExtendsDecl<'a>> {
        self.0.children().find_map(ExtendsDecl::cast)
    }
    /// Top-level members in source order.
    pub fn members(&self) -> impl Iterator<Item = Member<'a>> + 'a {
        self.0.children().filter_map(Member::cast)
    }
    /// Script-level annotations such as `@tool` and `@icon`, by name.
    pub fn script_annotations(&self) -> impl Iterator<Item = Annotation<'a>> + 'a {
        self.0
            .children()
            .filter_map(Annotation::cast)
            .filter(|annotation| SCRIPT_ANNOTATIONS.contains(&annotation.name()))
    }
}

impl<'a> ClassDecl<'a> {
    pub fn name(&self) -> Option<&'a str> {
        name_of(self.0)
    }
    pub fn extends(&self) -> Option<ExtendsDecl<'a>> {
        ExtendsDecl::cast(self.0)
    }
    pub fn members(&self) -> impl Iterator<Item = Member<'a>> + 'a {
        self.0
            .child(SyntaxKind::ClassBody)
            .into_iter()
            .flat_map(|body| body.children().filter_map(Member::cast))
    }
}

/// One declaration in a class body.
#[derive(Clone, Copy, Debug)]
pub enum Member<'a> {
    Signal(SignalDecl<'a>),
    Const(ConstDecl<'a>),
    Var(VarDecl<'a>),
    Func(FuncDecl<'a>),
    Enum(EnumDecl<'a>),
    Class(ClassDecl<'a>),
}

impl<'a> Member<'a> {
    pub fn cast(node: Node<'a>) -> Option<Self> {
        Some(match node.kind() {
            SyntaxKind::SignalDecl => Member::Signal(SignalDecl(node)),
            SyntaxKind::ConstDecl => Member::Const(ConstDecl(node)),
            SyntaxKind::VarDecl => Member::Var(VarDecl(node)),
            SyntaxKind::FuncDecl => Member::Func(FuncDecl(node)),
            SyntaxKind::EnumDecl => Member::Enum(EnumDecl(node)),
            SyntaxKind::InnerClassDecl => Member::Class(ClassDecl(node)),
            _ => return None,
        })
    }
    pub fn node(&self) -> Node<'a> {
        match self {
            Member::Signal(m) => m.0,
            Member::Const(m) => m.0,
            Member::Var(m) => m.0,
            Member::Func(m) => m.0,
            Member::Enum(m) => m.0,
            Member::Class(m) => m.0,
        }
    }
    /// `None` for an anonymous enum or a declaration missing its name.
    pub fn name(&self) -> Option<&'a str> {
        name_of(self.node())
    }
    /// Annotations immediately preceding this member (`@export`, `@onready`,
    /// `@rpc(...)`), in source order. Script-level annotations are excluded.
    pub fn annotations(&self) -> impl Iterator<Item = Annotation<'a>> + 'a {
        let node = self.node();
        let mut preceding: Vec<Annotation<'a>> = match node.parent() {
            Some(parent) => parent
                .children()
                .take_while(|sibling| *sibling != node)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map_while(Annotation::cast)
                .filter(|annotation| !SCRIPT_ANNOTATIONS.contains(&annotation.name()))
                .collect(),
            None => Vec::new(),
        };
        preceding.reverse();
        preceding.into_iter()
    }
}

impl<'a> ClassNameDecl<'a> {
    pub fn name(&self) -> Option<&'a str> {
        name_of(self.0)
    }
}

/// The `extends` part of a script, a `class_name … extends …` line, or an inner class.
#[derive(Clone, Copy, Debug)]
pub struct ExtendsDecl<'a>(pub Node<'a>);

impl<'a> ExtendsDecl<'a> {
    pub fn cast(node: Node<'a>) -> Option<Self> {
        match node.kind() {
            SyntaxKind::ExtendsClause => Some(Self(node)),
            SyntaxKind::ClassNameDecl | SyntaxKind::InnerClassDecl
                if node.has_token(SyntaxKind::ExtendsKw) =>
            {
                Some(Self(node))
            }
            _ => None,
        }
    }
    pub fn node(&self) -> Node<'a> {
        self.0
    }
    /// `Node`, `"res://base.gd"`, or `Outer.Inner` as written, without spaces.
    pub fn base_text(&self) -> &'a str {
        let mut start = None;
        let mut end = 0;
        let mut seen_extends = false;
        for element in self.0.elements() {
            let range = match element {
                Element::Token(token) if token.kind() == SyntaxKind::ExtendsKw => {
                    seen_extends = true;
                    continue;
                }
                Element::Token(token) if !seen_extends || !token.is_significant() => continue,
                Element::Token(token) if token.kind() == SyntaxKind::Colon => break,
                Element::Token(token) => token.token.range.range(),
                Element::Node(node) if seen_extends && node.kind() == SyntaxKind::Name => {
                    node.trimmed_range()
                }
                Element::Node(_) => continue,
            };
            start.get_or_insert(range.start);
            end = range.end;
        }
        match start {
            Some(start) => &self.0.parsed.source[start..end],
            None => "",
        }
    }
    /// The path of `extends "res://…"`, unquoted.
    pub fn base_path(&self) -> Option<String> {
        string_value(self.base_text())
    }
}

impl<'a> VarDecl<'a> {
    pub fn is_static(&self) -> bool {
        self.0.has_token(SyntaxKind::StaticKw)
    }
    pub fn type_text(&self) -> Option<&'a str> {
        self.0
            .child(SyntaxKind::TypeRef)
            .map(|node| node.trimmed_text())
    }
    pub fn initializer(&self) -> Option<Node<'a>> {
        node_after_assignment(self.0)
    }
}

impl<'a> ConstDecl<'a> {
    pub fn type_text(&self) -> Option<&'a str> {
        self.0
            .child(SyntaxKind::TypeRef)
            .map(|node| node.trimmed_text())
    }
    pub fn initializer(&self) -> Option<Node<'a>> {
        node_after_assignment(self.0)
    }
}

impl<'a> SignalDecl<'a> {
    pub fn parameters(&self) -> impl Iterator<Item = Parameter<'a>> + 'a {
        parameters_of(self.0)
    }
}

impl<'a> FuncDecl<'a> {
    pub fn is_static(&self) -> bool {
        self.0.has_token(SyntaxKind::StaticKw)
    }
    pub fn parameters(&self) -> impl Iterator<Item = Parameter<'a>> + 'a {
        parameters_of(self.0)
    }
    pub fn return_type_text(&self) -> Option<&'a str> {
        self.0
            .child(SyntaxKind::TypeRef)
            .map(|node| node.trimmed_text())
    }
    pub fn body(&self) -> Option<Node<'a>> {
        self.0.child(SyntaxKind::Block)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Parameter<'a> {
    pub name: &'a str,
    pub type_text: Option<&'a str>,
    pub default: Option<Node<'a>>,
    /// `...rest` (GDScript 4.5+).
    pub is_variadic: bool,
}

impl<'a> Annotation<'a> {
    /// Name without the `@`.
    pub fn name(&self) -> &'a str {
        self.0
            .own_tokens()
            .find(|token| token.kind() == SyntaxKind::Ident)
            .map_or("", |token| token.text())
    }
    /// Argument source texts, e.g. `["any_peer", "call_local"]`. String
    /// arguments keep their quotes.
    pub fn arguments(&self) -> Vec<&'a str> {
        argument_nodes(self.0)
            .map(|node| node.trimmed_text())
            .collect()
    }
}

impl<'a> CallExpr<'a> {
    /// Callee source text, e.g. `self.rpc`, `multiplayer.get_unique_id`, `rpc_id`.
    pub fn callee_text(&self) -> &'a str {
        self.0
            .children()
            .next()
            .map_or("", |node| node.trimmed_text())
    }
    pub fn arguments(&self) -> Vec<Node<'a>> {
        argument_nodes(self.0).collect()
    }
}

impl<'a> Preload<'a> {
    pub fn argument(&self) -> Option<Node<'a>> {
        argument_nodes(self.0).next()
    }
}

/// `$A/B`, `$"A/B"`, `%Unique`, `$%Unique/Child`.
#[derive(Clone, Copy, Debug)]
pub struct GetNode<'a>(pub Node<'a>);

impl<'a> GetNode<'a> {
    pub fn cast(node: Node<'a>) -> Option<Self> {
        matches!(
            node.kind(),
            SyntaxKind::GetNodeExpr | SyntaxKind::UniqueNodeExpr
        )
        .then_some(Self(node))
    }
    pub fn node(&self) -> Node<'a> {
        self.0
    }
    /// The node path the expression names, as `get_node` would receive it:
    /// `A/B`, `%Unique/Child`, `/root/Main`. Quoted segments are unquoted.
    pub fn path(&self) -> String {
        let mut path = String::new();
        let mut tokens = self.0.own_tokens().peekable();
        if tokens
            .peek()
            .is_some_and(|token| token.kind() == SyntaxKind::Dollar)
        {
            tokens.next();
        }
        for token in tokens {
            match token.kind() {
                SyntaxKind::Slash => path.push('/'),
                SyntaxKind::Percent => path.push('%'),
                SyntaxKind::String => {
                    path.push_str(&string_value(token.text()).unwrap_or_default())
                }
                _ => path.push_str(token.text()),
            }
        }
        path
    }
}

/// The value of a node that is a plain string literal (`"…"`, `'…'`,
/// `"""…"""`, `r"…"`), unescaped. `None` for anything else, including
/// `&"…"` StringNames, `^"…"` NodePaths, and concatenations.
pub fn string_literal(node: Node<'_>) -> Option<String> {
    if node.kind() != SyntaxKind::Literal {
        return None;
    }
    let token = node.own_tokens().next()?;
    (token.kind() == SyntaxKind::String).then(|| string_value(token.text()))?
}

/// Unquotes and unescapes GDScript string source text. `None` if it is not a string.
fn string_value(text: &str) -> Option<String> {
    let (raw, text) = match text.strip_prefix('r') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let quote = text.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let width = if text.len() >= 6 && text.starts_with(&quote.to_string().repeat(3)) {
        3
    } else {
        1
    };
    let inner = text.get(width..text.len().checked_sub(width)?)?;
    if raw {
        return Some(inner.to_owned());
    }
    let mut value = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            value.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => value.push('\n'),
            Some('t') => value.push('\t'),
            Some('r') => value.push('\r'),
            Some('a') => value.push('\u{7}'),
            Some('b') => value.push('\u{8}'),
            Some('f') => value.push('\u{c}'),
            Some('v') => value.push('\u{b}'),
            Some(digits @ ('u' | 'U')) => {
                let count = if digits == 'u' { 4 } else { 6 };
                let hex: String = chars.by_ref().take(count).collect();
                value.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
            }
            Some('\n') => {}
            Some(other) => value.push(other),
            None => value.push('\\'),
        }
    }
    Some(value)
}

fn name_of(node: Node<'_>) -> Option<&str> {
    node.child(SyntaxKind::Name)
        .map(|name| name.trimmed_text())
        .filter(|name| !name.is_empty())
}

fn node_after_assignment(node: Node<'_>) -> Option<Node<'_>> {
    let mut assigned = false;
    for element in node.elements() {
        match element {
            Element::Token(token)
                if matches!(token.kind(), SyntaxKind::Eq | SyntaxKind::ColonEq) =>
            {
                assigned = true
            }
            Element::Node(child) if assigned => return Some(child),
            _ => {}
        }
    }
    None
}

fn parameters_of<'a>(node: Node<'a>) -> impl Iterator<Item = Parameter<'a>> + 'a {
    node.child(SyntaxKind::ParamList)
        .into_iter()
        .flat_map(|list| list.children())
        .filter(|param| matches!(param.kind(), SyntaxKind::Param | SyntaxKind::VarargParam))
        .map(|param| Parameter {
            name: name_of(param).unwrap_or(""),
            type_text: param
                .child(SyntaxKind::TypeRef)
                .map(|node| node.trimmed_text()),
            default: node_after_assignment(param),
            is_variadic: param.kind() == SyntaxKind::VarargParam,
        })
}

fn argument_nodes<'a>(node: Node<'a>) -> impl Iterator<Item = Node<'a>> + 'a {
    node.child(SyntaxKind::ArgList)
        .into_iter()
        .flat_map(|list| list.children())
}

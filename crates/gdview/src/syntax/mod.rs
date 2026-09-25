//! Lossless GDScript syntax tree.
//!
//! The lexer, layout pass, and grammar are ported from gdview@49df929 (the
//! `legacy` dependency) unchanged; this file fixes the surface the rest of the
//! workspace is allowed to depend on. Nothing outside `syntax` may match on
//! token-level kinds; use the [`ast`] views.
//!
//! Invariants every consumer relies on:
//! - `parse(src).root().text() == src` byte for byte, always, even on garbage.
//! - `is_valid()` is false iff `diagnostics()` is non-empty.
//! - Ranges are byte offsets into the original source.
//! - A node's [`Node::range`] includes the trivia (whitespace, comments,
//!   newlines) that precedes its first token, so sibling ranges tile their
//!   parent. [`Node::trimmed_range`] is the span from its first to its last
//!   significant token, which is what views and line numbers use.
//!
//! # Tests (tests/syntax.rs)
//! - `round_trips_exact_source_including_crlf_bom_tabs_and_trailing_garbage`
//! - `recovers_from_errors_and_reports_byte_ranges`
//! - `diagnostics_are_ordered_by_offset`
//! - `deep_nesting_does_not_overflow_the_stack` (10k nested parens)
//! - `ast_views_expose_class_name_extends_signals_vars_consts_funcs_enums_inner_classes`
//! - `annotations_attach_to_the_following_declaration`
//! - `line_col_lookup_is_1_based_and_handles_multibyte`
//! - corpus (tests/syntax_corpus.rs, needs GODOT_SOURCE): every `modules/gdscript/tests/scripts/**/*.gd` parses with is_valid matching the engine's expectation.

pub mod ast;
mod kind;
mod layout;
mod lexer;
mod parser;

use std::ops::Range;

pub use kind::SyntaxKind;

/// Byte range into the parsed source.
pub type TextRange = Range<usize>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: TextRange,
    pub message: String,
}

/// Result of parsing. Owns the source; nodes borrow from it.
pub struct Parsed {
    source: String,
    tokens: Vec<Token>,
    nodes: Vec<NodeData>,
    diagnostics: Vec<Diagnostic>,
    /// Byte offset of the start of each line.
    line_starts: Vec<usize>,
}

impl Parsed {
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn root(&self) -> Node<'_> {
        Node { parsed: self, id: 0 }
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    pub fn is_valid(&self) -> bool {
        self.diagnostics().is_empty()
    }
    /// 1-based line and column for a byte offset. Columns count characters,
    /// not bytes. Offsets past the end clamp to the end of the source.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let mut offset = offset.min(self.source.len());
        while !self.source.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.line_starts.partition_point(|&start| start <= offset);
        let start = self.line_starts[line - 1];
        (line, self.source[start..offset].chars().count() + 1)
    }
}

pub fn parse(source: &str) -> Parsed {
    let raw = parser::parse(source);
    let mut diagnostics: Vec<Diagnostic> = raw
        .errors
        .into_iter()
        .map(|error| Diagnostic {
            range: error.range.range(),
            message: error.message,
        })
        .collect();
    diagnostics.sort_by_key(|diagnostic| diagnostic.range.start);
    let line_starts = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(index, _)| index + 1))
        .collect();
    Parsed {
        source: source.to_owned(),
        tokens: raw.tokens,
        nodes: raw.nodes,
        diagnostics,
        line_starts,
    }
}

/// A borrowed view of one node. `Copy` so iterators stay cheap.
#[derive(Clone, Copy)]
pub struct Node<'a> {
    parsed: &'a Parsed,
    id: u32,
}

impl std::fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("kind", &self.kind())
            .field("range", &self.range())
            .finish()
    }
}

impl PartialEq for Node<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.parsed, other.parsed) && self.id == other.id
    }
}

impl Eq for Node<'_> {}

impl<'a> Node<'a> {
    fn data(&self) -> &'a NodeData {
        &self.parsed.nodes[self.id as usize]
    }
    fn at(&self, id: NodeId) -> Node<'a> {
        Node { parsed: self.parsed, id: id.0 as u32 }
    }
    pub fn kind(&self) -> SyntaxKind {
        self.data().kind
    }
    /// Lossless range, including leading trivia.
    pub fn range(&self) -> TextRange {
        self.data().range.range()
    }
    /// Lossless text, including leading trivia.
    pub fn text(&self) -> &'a str {
        &self.parsed.source[self.range()]
    }
    /// From the first significant token to the last one. Empty (at the lossless
    /// start) for a node that holds only trivia.
    pub fn trimmed_range(&self) -> TextRange {
        let mut significant = self.tokens().filter(|token| token.is_significant());
        match significant.next() {
            Some(first) => {
                let end = significant.last().unwrap_or(first).token.range.end;
                first.token.range.start..end
            }
            None => self.range().start..self.range().start,
        }
    }
    pub fn trimmed_text(&self) -> &'a str {
        &self.parsed.source[self.trimmed_range()]
    }
    /// 1-based line of the first significant token.
    pub fn line(&self) -> usize {
        self.parsed.line_col(self.trimmed_range().start).0
    }
    pub fn parent(&self) -> Option<Node<'a>> {
        self.data().parent.map(|id| self.at(id))
    }
    /// Child nodes, in source order. Tokens are not nodes.
    pub fn children(self) -> impl Iterator<Item = Node<'a>> + 'a {
        let node = self;
        self.data().children.iter().filter_map(move |element| match element {
            ElementId::Node(id) => Some(node.at(*id)),
            ElementId::Token(_) => None,
        })
    }
    /// Pre-order traversal of this subtree, self included.
    pub fn descendants(self) -> impl Iterator<Item = Node<'a>> + 'a {
        let mut pending = vec![self];
        std::iter::from_fn(move || {
            let node = pending.pop()?;
            let mut children: Vec<_> = node.children().collect();
            children.reverse();
            pending.extend(children);
            Some(node)
        })
    }

    /// Children in order, nodes and tokens interleaved.
    pub(crate) fn elements(self) -> impl Iterator<Item = Element<'a>> + 'a {
        let node = self;
        self.data().children.iter().map(move |element| match element {
            ElementId::Node(id) => Element::Node(node.at(*id)),
            ElementId::Token(index) => Element::Token(TokenRef {
                parsed: node.parsed,
                token: &node.parsed.tokens[*index],
            }),
        })
    }
    /// Every token in the subtree, in source order.
    pub(crate) fn tokens(self) -> impl Iterator<Item = TokenRef<'a>> + 'a {
        let mut pending: Vec<std::vec::IntoIter<Element<'a>>> =
            vec![self.elements().collect::<Vec<_>>().into_iter()];
        std::iter::from_fn(move || {
            loop {
                match pending.last_mut()?.next() {
                    Some(Element::Token(token)) => return Some(token),
                    Some(Element::Node(node)) => {
                        pending.push(node.elements().collect::<Vec<_>>().into_iter())
                    }
                    None => {
                        pending.pop();
                    }
                }
            }
        })
    }
    /// Significant tokens that are direct children of this node.
    pub(crate) fn own_tokens(self) -> impl Iterator<Item = TokenRef<'a>> + 'a {
        self.elements().filter_map(|element| match element {
            Element::Token(token) if token.is_significant() => Some(token),
            _ => None,
        })
    }
    pub(crate) fn has_token(&self, kind: SyntaxKind) -> bool {
        self.own_tokens().any(|token| token.kind() == kind)
    }
    pub(crate) fn child(&self, kind: SyntaxKind) -> Option<Node<'a>> {
        self.children().find(|child| child.kind() == kind)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Element<'a> {
    Node(Node<'a>),
    Token(TokenRef<'a>),
}

#[derive(Clone, Copy)]
pub(crate) struct TokenRef<'a> {
    parsed: &'a Parsed,
    token: &'a Token,
}

impl<'a> TokenRef<'a> {
    pub(crate) fn kind(&self) -> SyntaxKind {
        self.token.kind
    }
    pub(crate) fn text(&self) -> &'a str {
        &self.parsed.source[self.token.range.range()]
    }
    fn is_significant(&self) -> bool {
        !self.token.kind.is_trivia() && !self.token.kind.is_synthetic_layout()
    }
}

// Storage shared with the ported parser.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Span {
    start: usize,
    end: usize,
}

impl Span {
    const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
    const fn empty(at: usize) -> Self {
        Self::new(at, at)
    }
    const fn start(self) -> usize {
        self.start
    }
    const fn is_empty(self) -> bool {
        self.start == self.end
    }
    const fn range(self) -> Range<usize> {
        self.start..self.end
    }
}

impl std::ops::Index<Span> for str {
    type Output = str;
    fn index(&self, span: Span) -> &str {
        &self[span.range()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Token {
    kind: SyntaxKind,
    range: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyntaxError {
    range: Span,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NodeId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementId {
    Node(NodeId),
    Token(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeData {
    kind: SyntaxKind,
    range: Span,
    parent: Option<NodeId>,
    children: Vec<ElementId>,
}

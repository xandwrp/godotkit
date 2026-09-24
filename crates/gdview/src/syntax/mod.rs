//! Lossless GDScript syntax tree.
//!
//! Port the lexer, grammar, tree, and typed AST views from gdview@49df929
//! (`legacy` dependency) into this module unchanged; this file only fixes the
//! surface the rest of the workspace is allowed to depend on. Nothing outside
//! `syntax` may match on token-level kinds; use the [`ast`] views.
//!
//! Invariants every consumer relies on:
//! - `parse(src).root().text() == src` byte for byte, always, even on garbage.
//! - `is_valid()` is false iff `diagnostics()` is non-empty.
//! - Ranges are byte offsets into the original source.
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

use std::ops::Range;

/// Byte range into the parsed source.
pub type TextRange = Range<usize>;

/// Token and node kinds. The full set is ported verbatim; only the ones other
/// modules name are listed here so the scaffold compiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SyntaxKind {
    SourceFile,
    ClassBody,
    ClassNameDecl,
    ExtendsDecl,
    SignalDecl,
    ConstDecl,
    VarDecl,
    FuncDecl,
    EnumDecl,
    ClassDecl,
    Annotation,
    CallExpr,
    ArgList,
    AssignExpr,
    Name,
    StringLit,
    Comment,
    Whitespace,
    Newline,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: TextRange,
    pub message: String,
}

/// Result of parsing. Owns the source; nodes borrow from it.
pub struct Parsed {
    source: String,
    // tree storage ported from gdview (green/red nodes)
}

impl Parsed {
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn root(&self) -> Node<'_> {
        todo!()
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        todo!()
    }
    pub fn is_valid(&self) -> bool {
        self.diagnostics().is_empty()
    }
    /// 1-based line and column for a byte offset.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        todo!()
    }
}

pub fn parse(source: &str) -> Parsed {
    todo!()
}

/// A borrowed view of one node. `Copy` so iterators stay cheap.
#[derive(Clone, Copy)]
#[allow(dead_code)] // storage fields are read once the tree is ported
pub struct Node<'a> {
    parsed: &'a Parsed,
    // index into tree storage
    id: u32,
}

impl<'a> Node<'a> {
    pub fn kind(&self) -> SyntaxKind {
        todo!()
    }
    pub fn range(&self) -> TextRange {
        todo!()
    }
    pub fn text(&self) -> &'a str {
        todo!()
    }
    pub fn parent(&self) -> Option<Node<'a>> {
        todo!()
    }
    pub fn children(&self) -> impl Iterator<Item = Node<'a>> + 'a {
        std::iter::empty()
    }
    /// Pre-order traversal of this subtree, self included.
    pub fn descendants(&self) -> impl Iterator<Item = Node<'a>> + 'a {
        std::iter::empty()
    }
    pub fn is_token(&self) -> bool {
        todo!()
    }
}

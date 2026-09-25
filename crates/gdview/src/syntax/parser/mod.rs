use super::lexer::tokenize;
use super::{ElementId, NodeData, NodeId, Span, SyntaxError, SyntaxKind, Token, layout};

mod grammar;

pub(super) struct RawTree {
    pub tokens: Vec<Token>,
    pub nodes: Vec<NodeData>,
    pub errors: Vec<SyntaxError>,
}

pub(super) fn parse(source: &str) -> RawTree {
    let (raw, mut errors) = tokenize(source);
    let (tokens, layout_errors) = layout::run(&raw, source);
    errors.extend(layout_errors);
    let mut parser = Parser::new(source, &tokens);
    parser.source_file();
    errors.extend(parser.errors);
    let nodes = build_tree(parser.events, &tokens);
    RawTree {
        tokens,
        nodes,
        errors,
    }
}

#[derive(Debug, Clone, Copy)]
enum Event {
    Open {
        kind: SyntaxKind,
        parent: Option<usize>,
    },
    Close,
    Advance,
    Empty,
}

#[must_use]
struct Marker(usize);

#[derive(Clone, Copy)]
struct MarkClosed(usize);

struct Parser<'a> {
    src: &'a str,
    tokens: &'a [Token],
    nontrivia: Vec<usize>,
    pos: usize,
    depth: usize,
    events: Vec<Event>,
    errors: Vec<SyntaxError>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, tokens: &'a [Token]) -> Self {
        Self {
            src,
            tokens,
            nontrivia: tokens
                .iter()
                .enumerate()
                .filter_map(|(i, token)| (!token.kind.is_trivia()).then_some(i))
                .collect(),
            pos: 0,
            depth: 0,
            events: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn nested<T: Default>(&mut self, parse: impl FnOnce(&mut Self) -> T) -> T {
        if self.depth >= 128 {
            self.advance_with_error("Syntax nesting limit exceeded.");
            return T::default();
        }
        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;
        result
    }

    fn nth(&self, n: usize) -> SyntaxKind {
        self.nontrivia
            .get(self.pos + n)
            .map_or(SyntaxKind::Eof, |&i| self.tokens[i].kind)
    }

    fn at(&self, kind: SyntaxKind) -> bool {
        self.nth(0) == kind
    }
    fn at_any(&self, kinds: &[SyntaxKind]) -> bool {
        kinds.contains(&self.nth(0))
    }
    fn eof(&self) -> bool {
        self.pos == self.nontrivia.len()
    }

    fn cur_range(&self) -> Span {
        self.nontrivia
            .get(self.pos)
            .map_or(Span::empty(self.src.len()), |&i| self.tokens[i].range)
    }

    fn text_at(&self, offset: usize) -> &str {
        self.nontrivia
            .get(self.pos + offset)
            .map_or("", |&i| &self.src[self.tokens[i].range])
    }

    fn cur_text(&self) -> &str {
        &self.src[self.cur_range()]
    }

    fn advance(&mut self) {
        if !self.eof() {
            self.events.push(Event::Advance);
            self.pos += 1;
        }
    }

    fn open(&mut self) -> Marker {
        let marker = Marker(self.events.len());
        self.events.push(Event::Empty);
        marker
    }

    fn close(&mut self, marker: Marker, kind: SyntaxKind) -> MarkClosed {
        self.events[marker.0] = Event::Open { kind, parent: None };
        self.events.push(Event::Close);
        MarkClosed(marker.0)
    }

    fn open_before(&mut self, closed: MarkClosed) -> Marker {
        let marker = self.open();
        if let Event::Open { parent, .. } = &mut self.events[closed.0] {
            *parent = Some(marker.0);
        }
        marker
    }

    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if !self.at(kind) {
            return false;
        }
        self.advance();
        true
    }

    fn expect(&mut self, kind: SyntaxKind) {
        if !self.eat(kind) {
            self.error(format!("Expected {}.", kind.display_name()));
        }
    }

    fn expect_after(&mut self, kind: SyntaxKind, context: &str) {
        if !self.eat(kind) {
            self.error(format!("Expected {} after {context}.", kind.display_name()));
        }
    }

    fn expect_closing(&mut self, kind: SyntaxKind, context: &str) {
        if !self.eat(kind) {
            self.error(format!(
                "Expected closing {} after {context}.",
                kind.display_name()
            ));
        }
    }

    fn error(&mut self, message: String) {
        let range = self.cur_range();
        if self
            .errors
            .last()
            .is_none_or(|error| error.range != range || error.message != message)
        {
            self.errors.push(SyntaxError { range, message });
        }
    }

    fn advance_with_error(&mut self, message: &str) -> MarkClosed {
        let marker = self.open();
        self.error(message.into());
        self.advance();
        self.close(marker, SyntaxKind::ErrorNode)
    }

    fn match_begins_statement(&self) -> bool {
        use SyntaxKind::*;
        match self.nth(1) {
            Dot | Eq | ColonEq | PlusEq | MinusEq | StarEq | SlashEq | StarStarEq | PercentEq
            | AmpEq | PipeEq | CaretEq | ShlEq | ShrEq => false,
            LParen | LBrack => {
                let mut depth = 0usize;
                for offset in 1..=self.nontrivia.len() - self.pos {
                    match self.nth(offset) {
                        Eof => break,
                        LParen | LBrack | LBrace => depth += 1,
                        RParen | RBrack | RBrace => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                return self.nth(offset + 1) == Colon;
                            }
                        }
                        _ => {}
                    }
                }
                true
            }
            _ => true,
        }
    }
}

fn build_tree(mut events: Vec<Event>, tokens: &[Token]) -> Vec<NodeData> {
    let mut nodes: Vec<NodeData> = Vec::new();
    let mut stack: Vec<NodeId> = Vec::new();
    let mut cursor = 0;
    let mut byte_end = 0;
    for index in 0..events.len() {
        match std::mem::replace(&mut events[index], Event::Empty) {
            Event::Open { kind, parent } => {
                let mut kinds = vec![kind];
                let mut next = parent;
                while let Some(index) = next {
                    match std::mem::replace(&mut events[index], Event::Empty) {
                        Event::Open { kind, parent } => {
                            kinds.push(kind);
                            next = parent;
                        }
                        _ => unreachable!("forward parent must be an open event"),
                    }
                }
                for kind in kinds.into_iter().rev() {
                    let id = NodeId(nodes.len());
                    nodes.push(NodeData {
                        kind,
                        range: Span::empty(byte_end),
                        parent: stack.last().copied(),
                        children: Vec::new(),
                    });
                    if let Some(parent) = stack.last() {
                        nodes[parent.0].children.push(ElementId::Node(id));
                    }
                    stack.push(id);
                }
            }
            Event::Advance => {
                let node = &mut nodes[stack.last().unwrap().0];
                while cursor < tokens.len() {
                    let token = tokens[cursor];
                    node.children.push(ElementId::Token(cursor));
                    cursor += 1;
                    if !token.range.is_empty() {
                        byte_end = token.range.end;
                    }
                    if !token.kind.is_trivia() {
                        break;
                    }
                }
            }
            Event::Close => {
                let id = stack.pop().unwrap();
                if stack.is_empty() {
                    for (index, token) in tokens.iter().enumerate().skip(cursor) {
                        nodes[id.0].children.push(ElementId::Token(index));
                        if !token.range.is_empty() {
                            byte_end = token.range.end;
                        }
                    }
                }
                nodes[id.0].range.end = byte_end;
            }
            Event::Empty => {}
        }
    }
    nodes
}

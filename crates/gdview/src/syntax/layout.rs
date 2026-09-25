use super::{Span, SyntaxError, SyntaxKind, Token};

const TAB_SIZE: u32 = 4;

#[derive(Debug, Clone)]
struct LambdaCtx {
    saved_indent_stack: Vec<u32>,
    base: u32,

    open_bracket_depth: u32,
}

#[derive(Debug, Clone, Copy)]
enum SignaturePhase {
    AfterFunc,
    AfterName,
    Parameters,
    AfterParameters,
    ReturnType,
}

#[derive(Debug, Clone)]
struct LambdaSignature {
    base: u32,
    bracket_depth: u32,
    phase: SignaturePhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndentChar {
    Tab,
    Space,
}

#[must_use]
pub fn run(tokens: &[Token], src: &str) -> (Vec<Token>, Vec<SyntaxError>) {
    let mut p = PrePass {
        src,
        out: Vec::with_capacity(tokens.len() + 16),
        diags: Vec::new(),
        indent_stack: vec![0],
        bracket_depth: 0,
        indent_char: None,
        lambda_stack: Vec::new(),
        lambda_signatures: Vec::new(),
    };
    p.run_lines(tokens);
    (p.out, p.diags)
}

struct PrePass<'s> {
    src: &'s str,
    out: Vec<Token>,
    diags: Vec<SyntaxError>,
    indent_stack: Vec<u32>,
    bracket_depth: u32,
    indent_char: Option<IndentChar>,

    lambda_stack: Vec<LambdaCtx>,
    lambda_signatures: Vec<LambdaSignature>,
}

impl PrePass<'_> {
    fn run_lines(&mut self, tokens: &[Token]) {
        let mut start = 0usize;
        let mut i = 0usize;
        let mut continued = false;
        while i < tokens.len() {
            if tokens[i].kind == SyntaxKind::LineContinuation {
                continued = true;
            } else if !tokens[i].kind.is_trivia() {
                continued = false;
            }
            if tokens[i].kind == SyntaxKind::NewlinePhys && !continued {
                self.line(&tokens[start..=i]);
                start = i + 1;
            }
            i += 1;
        }
        if start < tokens.len() {
            self.line(&tokens[start..]);
        }
        self.finish(src_end(self.src));
    }

    fn line(&mut self, line: &[Token]) {
        let Some(first) = line.iter().find(|t| !t.kind.is_trivia()) else {
            self.copy_verbatim(line);
            return;
        };
        let col = self.column(line);
        let at = first.range.start();

        if matches!(
            first.kind,
            SyntaxKind::RParen | SyntaxKind::RBrace | SyntaxKind::RBrack
        ) && self
            .lambda_stack
            .last()
            .is_some_and(|ctx| ctx.open_bracket_depth >= self.bracket_depth)
        {
            self.close_lambdas_on_bracket(self.bracket_depth.saturating_sub(1), at);
        }

        self.close_lambdas(col, at);

        let suppressed = self.indentation_suppressed();

        let mut lambda_base = None;

        if !suppressed {
            self.diagnose_indent(line);
            self.emit_indent_dedent(col, at);
        }

        let mut has_terminator = false;
        for (index, tok) in line.iter().enumerate() {
            if tok.kind == SyntaxKind::NewlinePhys && index + 1 == line.len() {
                has_terminator = true;
                let opens_lambda = lambda_base.is_some();

                if !self.indentation_suppressed() || opens_lambda {
                    self.push_marker(SyntaxKind::Newline, tok.range.start());
                }
                self.out.push(*tok);
            } else {
                if matches!(
                    tok.kind,
                    SyntaxKind::RParen | SyntaxKind::RBrace | SyntaxKind::RBrack
                ) && !self.lambda_stack.is_empty()
                {
                    let new_depth = self.bracket_depth.saturating_sub(1);
                    self.close_lambdas_on_bracket(new_depth, tok.range.start());
                } else if tok.kind == SyntaxKind::Comma
                    && self
                        .lambda_stack
                        .last()
                        .is_some_and(|ctx| ctx.open_bracket_depth == self.bracket_depth)
                {
                    self.close_lambdas_on_bracket(
                        self.bracket_depth.saturating_sub(1),
                        tok.range.start(),
                    );
                }
                if !tok.kind.is_trivia() {
                    lambda_base = self.track_lambda_signature(tok.kind, col);
                }
                self.out.push(*tok);
                self.track_bracket(tok.kind);
            }
        }

        if !has_terminator && !self.indentation_suppressed() {
            self.push_marker(SyntaxKind::Newline, src_end(self.src));
        }

        if let Some(base) = lambda_base {
            let saved = std::mem::replace(&mut self.indent_stack, vec![base]);
            self.lambda_stack.push(LambdaCtx {
                saved_indent_stack: saved,
                base,
                open_bracket_depth: self.bracket_depth,
            });
        }
    }

    fn indentation_suppressed(&self) -> bool {
        match self.lambda_stack.last() {
            Some(ctx) => self.bracket_depth > ctx.open_bracket_depth,
            None => self.bracket_depth > 0,
        }
    }

    fn close_lambdas(&mut self, col: u32, at: usize) {
        // Body-local brackets suppress indentation, but enclosing delimiters still
        // close lambdas through close_lambdas_on_bracket.
        while self
            .lambda_stack
            .last()
            .is_some_and(|ctx| col <= ctx.base && self.bracket_depth <= ctx.open_bracket_depth)
        {
            let base = self.lambda_stack.last().expect("checked").base;
            while *self.indent_stack.last().expect("lambda base present") > base {
                self.indent_stack.pop();
                self.push_marker(SyntaxKind::Dedent, at);
            }
            let ctx = self.lambda_stack.pop().expect("checked");
            self.indent_stack = ctx.saved_indent_stack;
        }
    }

    fn close_lambdas_on_bracket(&mut self, new_depth: u32, at: usize) {
        while self
            .lambda_stack
            .last()
            .is_some_and(|ctx| ctx.open_bracket_depth > new_depth)
        {
            let base = self.lambda_stack.last().expect("checked").base;
            while *self.indent_stack.last().expect("lambda base present") > base {
                self.indent_stack.pop();
                self.push_marker(SyntaxKind::Dedent, at);
            }
            let ctx = self.lambda_stack.pop().expect("checked");
            self.indent_stack = ctx.saved_indent_stack;
        }
    }

    fn copy_verbatim(&mut self, line: &[Token]) {
        for tok in line {
            self.out.push(*tok);
            if tok.kind != SyntaxKind::NewlinePhys {
                self.track_bracket(tok.kind);
            }
        }
    }

    fn emit_indent_dedent(&mut self, col: u32, at: usize) {
        let top = *self.indent_stack.last().expect("indent stack has a base 0");
        if col > top {
            self.indent_stack.push(col);
            self.push_marker(SyntaxKind::Indent, at);
        } else if col < top {
            while *self.indent_stack.last().expect("base 0 guards the loop") > col {
                self.indent_stack.pop();
                self.push_marker(SyntaxKind::Dedent, at);
            }
            if *self.indent_stack.last().expect("non-empty") != col {
                self.diags.push(SyntaxError {
                    range: Span::empty(at),
                    message: "Unindent does not match any outer indentation level.".to_owned(),
                });
                self.indent_stack.push(col);
            }
        }
    }

    fn column(&self, line: &[Token]) -> u32 {
        let Some(ws) = line.first().filter(|t| t.kind == SyntaxKind::Whitespace) else {
            return 0;
        };
        self.src[ws.range]
            .bytes()
            .fold(0u32, |col, b| col + if b == b'\t' { TAB_SIZE } else { 1 })
    }

    fn diagnose_indent(&mut self, line: &[Token]) {
        let Some(ws) = line.first().filter(|t| t.kind == SyntaxKind::Whitespace) else {
            return;
        };
        let text = &self.src[ws.range];
        let mut saw_tab = false;
        let mut saw_space = false;
        for b in text.bytes() {
            saw_tab |= b == b'\t';
            saw_space |= b == b' ';
        }
        if saw_tab && saw_space {
            self.diags.push(SyntaxError {
                range: ws.range,
                message: "Mixed use of tabs and spaces for indentation.".to_owned(),
            });
        } else if let Some(first) = text.bytes().next() {
            let this = if first == b'\t' {
                IndentChar::Tab
            } else {
                IndentChar::Space
            };
            match self.indent_char {
                None => self.indent_char = Some(this),
                Some(file) if file != this => {
                    let (used, before) = match this {
                        IndentChar::Tab => ("tab", "space"),
                        IndentChar::Space => ("space", "tab"),
                    };
                    self.diags.push(SyntaxError {
                        range: ws.range,
                        message: format!(
                            "Used {used} character for indentation instead of {before} as used before in the file."
                        ),
                    });
                }
                Some(_) => {}
            }
        }
    }

    fn finish(&mut self, at: usize) {
        self.close_lambdas_on_bracket(0, at);
        while *self.indent_stack.last().expect("base 0") > 0 {
            self.indent_stack.pop();
            self.push_marker(SyntaxKind::Dedent, at);
        }
    }

    fn track_lambda_signature(&mut self, kind: SyntaxKind, col: u32) -> Option<u32> {
        use SignaturePhase as P;
        use SyntaxKind as S;

        if let Some(signature) = self.lambda_signatures.last_mut() {
            let depth = signature.bracket_depth;
            if self.bracket_depth > depth {
                if matches!(signature.phase, P::Parameters)
                    && kind == S::RParen
                    && self.bracket_depth == depth + 1
                {
                    signature.phase = P::AfterParameters;
                }
            } else {
                match (signature.phase, kind) {
                    (P::AfterFunc, S::Ident) if self.bracket_depth == depth => {
                        signature.phase = P::AfterName;
                    }
                    (P::AfterFunc | P::AfterName, S::LParen) if self.bracket_depth == depth => {
                        signature.phase = P::Parameters;
                    }
                    (P::AfterParameters, S::Arrow) if self.bracket_depth == depth => {
                        signature.phase = P::ReturnType;
                    }
                    (P::AfterParameters | P::ReturnType, S::Colon)
                        if self.bracket_depth == depth =>
                    {
                        return self.lambda_signatures.pop().map(|signature| signature.base);
                    }
                    (P::ReturnType, S::Ident | S::VoidKw | S::Dot | S::LBrack)
                        if self.bracket_depth == depth => {}
                    _ => {
                        self.lambda_signatures.pop();
                    }
                }
            }
        }
        if kind == S::FuncKw && self.bracket_depth > 0 {
            // Keep the func line's indentation even when parameters or the return
            // type end on a differently indented line. A stack permits lambda defaults.
            self.lambda_signatures.push(LambdaSignature {
                base: col,
                bracket_depth: self.bracket_depth,
                phase: P::AfterFunc,
            });
        }
        None
    }

    fn track_bracket(&mut self, kind: SyntaxKind) {
        match kind {
            SyntaxKind::LParen | SyntaxKind::LBrack | SyntaxKind::LBrace => {
                self.bracket_depth += 1;
            }
            SyntaxKind::RParen | SyntaxKind::RBrack | SyntaxKind::RBrace => {
                self.bracket_depth = self.bracket_depth.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn push_marker(&mut self, kind: SyntaxKind, at: usize) {
        self.out.push(Token {
            kind,
            range: Span::empty(at),
        });
    }
}

fn src_end(src: &str) -> usize {
    src.len()
}

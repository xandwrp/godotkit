use super::{Span, SyntaxError, SyntaxKind as K, Token};

pub fn tokenize(source: &str) -> (Vec<Token>, Vec<SyntaxError>) {
    let mut lexer = Lexer {
        source,
        cursor: 0,
        errors: Vec::new(),
    };
    let mut tokens = Vec::new();
    while lexer.cursor < source.len() {
        let start = lexer.cursor;
        let kind = lexer.next();
        tokens.push(Token {
            kind,
            range: Span::new(start, lexer.cursor),
        });
    }
    (tokens, lexer.errors)
}

struct Lexer<'a> {
    source: &'a str,
    cursor: usize,
    errors: Vec<SyntaxError>,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.source[self.cursor..].chars().next()
    }

    fn bump(&mut self) {
        if let Some(ch) = self.peek() {
            self.cursor += ch.len_utf8();
        }
    }

    fn take_while(&mut self, predicate: impl Fn(char) -> bool) {
        while self.peek().is_some_and(&predicate) {
            self.bump();
        }
    }

    fn error(&mut self, start: usize, message: &str) {
        self.errors.push(SyntaxError {
            range: Span::new(start, self.cursor),
            message: message.into(),
        });
    }

    fn newline(&mut self) {
        let cr = self.peek() == Some('\r');
        self.bump();
        if cr && self.peek() == Some('\n') {
            self.bump();
        }
    }

    fn next(&mut self) -> K {
        let start = self.cursor;
        let tail = &self.source[start..];
        let ch = self.peek().unwrap();
        match ch {
            ' ' | '\t' => {
                self.take_while(|c| matches!(c, ' ' | '\t'));
                K::Whitespace
            }
            '\r' | '\n' => {
                self.newline();
                K::NewlinePhys
            }
            '\u{feff}' if start == 0 => {
                self.bump();
                K::Bom
            }
            '#' => {
                self.take_while(|c| !matches!(c, '\r' | '\n'));
                if tail.starts_with("#region") {
                    K::RegionComment
                } else if tail.starts_with("#endregion") {
                    K::EndRegionComment
                } else if tail.starts_with("##") {
                    K::DocComment
                } else {
                    K::LineComment
                }
            }
            '\\' if tail
                .as_bytes()
                .get(1)
                .is_some_and(|c| matches!(c, b'\n' | b'\r')) =>
            {
                self.bump();
                self.newline();
                K::LineContinuation
            }
            '\'' | '"' => self.string(start, K::String, false),
            'r' | '&' | '^'
                if tail
                    .as_bytes()
                    .get(1)
                    .is_some_and(|c| matches!(c, b'\'' | b'"')) =>
            {
                self.bump();
                let kind = match ch {
                    '&' => K::StringName,
                    '^' => K::NodePath,
                    _ => K::String,
                };
                self.string(start, kind, ch == 'r')
            }
            '0'..='9' => self.number(),
            '.' if tail.as_bytes().get(1).is_some_and(u8::is_ascii_digit) => self.number(),
            '_' => self.identifier(),
            c if unicode_ident::is_xid_start(c) => self.identifier(),
            _ => {
                for width in (1..=3.min(tail.len())).rev() {
                    if let Some(text) = tail.get(..width)
                        && let Some(kind) = K::from_fixed_text(text)
                    {
                        self.cursor += width;
                        return kind;
                    }
                }
                self.bump();
                self.error(start, "Unexpected character.");
                K::Error
            }
        }
    }

    fn identifier(&mut self) -> K {
        let start = self.cursor;
        self.bump();
        self.take_while(unicode_ident::is_xid_continue);
        K::from_fixed_text(&self.source[start..self.cursor]).unwrap_or(K::Ident)
    }

    fn string(&mut self, start: usize, kind: K, raw: bool) -> K {
        let quote = self.peek().unwrap();
        let triple =
            self.source.as_bytes().get(self.cursor..self.cursor + 3) == Some(&[quote as u8; 3]);
        let width = if triple { 3 } else { 1 };
        self.cursor += width;
        while let Some(ch) = self.peek() {
            if self.source.as_bytes().get(self.cursor..self.cursor + width)
                == Some(&[quote as u8; 3][..width])
            {
                self.cursor += width;
                return kind;
            }
            self.bump();
            if ch == '\\'
                && let Some(escaped) = self.peek()
            {
                if !raw
                    && !matches!(
                        escaped,
                        'a' | 'b'
                            | 'f'
                            | 'n'
                            | 'r'
                            | 't'
                            | 'v'
                            | '\\'
                            | '\''
                            | '"'
                            | 'u'
                            | 'U'
                            | '\n'
                            | '\r'
                    )
                {
                    self.error(self.cursor - 1, "Invalid escape sequence.");
                }
                if !raw && matches!(escaped, 'u' | 'U') {
                    let escape_start = self.cursor - 1;
                    self.bump();
                    let digits = if escaped == 'u' { 4 } else { 6 };
                    let hex_start = self.cursor;
                    for _ in 0..digits {
                        if self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    if self.cursor - hex_start != digits {
                        self.error(escape_start, "Invalid Unicode escape.");
                    }
                    continue;
                }
                if matches!(escaped, '\r' | '\n') {
                    self.newline();
                } else {
                    self.bump();
                }
            }
        }
        self.error(start, "Unterminated string.");
        kind
    }

    fn number(&mut self) -> K {
        let start = self.cursor;
        let mut float = false;
        if self.source[start..].starts_with("0x")
            || self.source[start..].starts_with("0X")
            || self.source[start..].starts_with("0b")
            || self.source[start..].starts_with("0B")
        {
            let radix = if self.source.as_bytes()[start + 1].eq_ignore_ascii_case(&b'x') {
                16
            } else {
                2
            };
            self.cursor += 2;
            let digits = self.cursor;
            self.take_while(|c| c.is_ascii_alphanumeric() || c == '_');
            let value = &self.source[digits..self.cursor];
            if !value.chars().any(|c| c.is_digit(radix))
                || value.chars().any(|c| c != '_' && !c.is_digit(radix))
            {
                self.error(start, "Invalid integer literal.");
            }
            return K::Int;
        }
        self.take_while(|c| c.is_ascii_digit() || c == '_');
        if self.peek() == Some('.') && !self.source[self.cursor..].starts_with("..") {
            float = true;
            self.bump();
            self.take_while(|c| c.is_ascii_digit() || c == '_');
        }
        if self.peek().is_some_and(|c| matches!(c, 'e' | 'E')) {
            float = true;
            self.bump();
            if self.peek().is_some_and(|c| matches!(c, '+' | '-')) {
                self.bump();
            }
            let exponent = self.cursor;
            self.take_while(|c| c.is_ascii_digit() || c == '_');
            if !self.source[exponent..self.cursor]
                .bytes()
                .any(|c| c.is_ascii_digit())
            {
                self.error(start, "Expected exponent digits.");
            }
        }
        if self.peek().is_some_and(unicode_ident::is_xid_start) {
            self.take_while(unicode_ident::is_xid_continue);
            self.error(start, "Invalid numeric literal.");
        }
        if float { K::Float } else { K::Int }
    }
}

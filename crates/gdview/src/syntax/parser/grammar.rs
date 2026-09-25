use super::super::SyntaxKind::{self, *};
use super::{Event, MarkClosed, Parser};

#[derive(Clone, Copy)]
enum Assoc {
    Left,
    Right,
}

const PREC_ASSIGN: u8 = 1;
const PREC_CAST: u8 = 2;
const PREC_TERNARY: u8 = 3;
const PREC_OR: u8 = 4;
const PREC_AND: u8 = 5;
const PREC_NOT: u8 = 6;
const PREC_IN: u8 = 7;
const PREC_CMP: u8 = 8;
const PREC_BIT_OR: u8 = 9;
const PREC_BIT_XOR: u8 = 10;
const PREC_BIT_AND: u8 = 11;
const PREC_SHIFT: u8 = 12;
const PREC_ADD: u8 = 13;
const PREC_FACTOR: u8 = 14;
const PREC_SIGN: u8 = 15;
const PREC_BIT_NOT: u8 = 16;
const PREC_POWER: u8 = 17;
const PREC_TYPE_TEST: u8 = 18;
const PREC_AWAIT: u8 = 19;

const fn bp(prec: u8, assoc: Assoc) -> (u8, u8) {
    match assoc {
        Assoc::Left => (2 * prec, 2 * prec + 1),
        Assoc::Right => (2 * prec + 1, 2 * prec),
    }
}

const RECOVERY: &[SyntaxKind] = &[
    FuncKw,
    VarKw,
    ConstKw,
    ClassKw,
    ClassNameKw,
    ExtendsKw,
    EnumKw,
    SignalKw,
    StaticKw,
    At,
    IfKw,
    ForKw,
    WhileKw,
    MatchKw,
    ReturnKw,
    BreakKw,
    ContinueKw,
    PassKw,
    AssertKw,
    BreakpointKw,
];

impl Parser<'_> {
    pub(super) fn source_file(&mut self) {
        let m = self.open();
        self.members(&[]);
        self.close(m, SourceFile);
    }

    fn members(&mut self, until: &[SyntaxKind]) {
        while !self.eof() && !self.at_any(until) {
            if self.at_any(&[Newline, Semicolon]) {
                self.advance();
                continue;
            }
            self.item();
        }
    }

    fn item(&mut self) {
        let annotation = self.at(At);
        self.member();
        if !annotation {
            self.end_statement();
        }
    }

    fn end_statement(&mut self) {
        let boundary = self.pos > 0
            && matches!(
                self.tokens[self.nontrivia[self.pos - 1]].kind,
                Newline | Dedent
            );
        if !boundary
            && !self.at_any(&[
                Newline, Dedent, Semicolon, Eof, RParen, RBrack, RBrace, Comma,
            ])
        {
            self.error("Expected end of statement.".into());
        }
    }

    fn member(&mut self) {
        match self.nth(0) {
            At => self.annotation(),
            ClassNameKw => self.class_name_decl(),
            ExtendsKw => self.extends_clause(),
            FuncKw => self.func_decl(),
            StaticKw if self.nth(1) == FuncKw => self.func_decl(),
            VarKw | StaticKw => self.var_decl(),
            ConstKw => self.const_decl(),
            EnumKw => self.enum_decl(),
            SignalKw => self.signal_decl(),
            ClassKw => self.inner_class(),
            PassKw => self.simple_stmt(PassStmt),
            String => self.simple_stmt(Literal),
            _ => {
                let msg = format!("Unexpected {} in class body.", self.nth(0).display_name());
                self.advance_with_error(&msg);
            }
        }
    }

    fn annotation(&mut self) {
        let m = self.open();
        self.expect(At);
        if self.at(Ident) {
            self.advance();
        } else {
            self.error("Expected annotation identifier after \"@\".".to_owned());
        }
        if self.at(LParen) {
            self.arg_list();
        }
        self.close(m, Annotation);
    }

    fn class_name_decl(&mut self) {
        let m = self.open();
        self.expect(ClassNameKw);
        self.required_name();
        if self.eat(ExtendsKw) {
            self.extends_target();
        }
        self.close(m, ClassNameDecl);
    }

    fn extends_clause(&mut self) {
        let m = self.open();
        self.expect(ExtendsKw);
        self.extends_target();
        self.close(m, ExtendsClause);
    }

    fn extends_target(&mut self) {
        if !self.eat(String) {
            self.required_name();
        }
        while self.eat(Dot) {
            self.required_name();
        }
    }

    fn func_decl(&mut self) {
        let m = self.open();
        self.eat(StaticKw);
        self.expect(FuncKw);
        self.required_name();
        self.param_list("function parameters");
        if self.eat(Arrow) {
            self.type_ref();
        }
        if self.eat(Colon) {
            self.block();
        } else if !self.at_any(&[Newline, Semicolon, Dedent, Eof]) {
            self.error("Expected a function body or end of declaration.".into());
        }
        self.close(m, FuncDecl);
    }

    fn var_decl(&mut self) {
        let m = self.open();
        self.eat(StaticKw);
        self.expect(VarKw);
        self.required_name();
        if self.at(Colon) && (self.nth(1) == Newline || matches!(self.text_at(1), "get" | "set")) {
            self.property_body();
        } else {
            if self.eat(Colon) && !self.at(Eq) {
                self.type_ref();
            }
            if self.eat(ColonEq) || self.eat(Eq) {
                self.expr();
            }
            if self.at(Colon) {
                self.property_body();
            }
        }
        self.close(m, VarDecl);
    }

    fn const_decl(&mut self) {
        let m = self.open();
        self.expect(ConstKw);
        self.required_name();
        if self.eat(Colon) && !self.at_any(&[Eq, Newline, Dedent]) {
            self.type_ref();
        }
        if self.eat(ColonEq) || self.eat(Eq) {
            self.expr();
        } else {
            self.error("Expected a constant initializer.".into());
        }
        self.close(m, ConstDecl);
    }

    fn enum_decl(&mut self) {
        let m = self.open();
        self.expect(EnumKw);
        self.opt_name();
        self.expect(LBrace);
        while !self.at(RBrace) && !self.eof() {
            let v = self.open();
            self.required_name();
            if self.eat(Eq) {
                self.expr();
            }
            self.close(v, EnumVariant);
            if !self.eat(Comma) {
                break;
            }
        }
        self.expect(RBrace);
        self.close(m, EnumDecl);
    }

    fn signal_decl(&mut self) {
        let m = self.open();
        self.expect(SignalKw);
        if self.at_name() {
            self.opt_name();
        } else {
            self.error("Expected signal name after \"signal\".".to_owned());
        }
        if self.at(LParen) {
            self.param_list("signal parameters");
        }
        self.close(m, SignalDecl);
    }

    fn inner_class(&mut self) {
        self.nested(|p| {
            let m = p.open();
            p.expect(ClassKw);
            p.required_name();
            if p.eat(ExtendsKw) {
                p.extends_target();
            }
            p.expect_after(Colon, "class declaration");
            let b = p.open();
            if p.eat(Newline) {
                if p.eat(Indent) {
                    p.members(&[Dedent]);
                    p.expect(Dedent);
                } else {
                    p.error("Expected an indented class body.".into());
                }
            } else {
                p.members(&[Newline, Dedent]);
            }
            p.close(b, ClassBody);
            p.close(m, InnerClassDecl);
        })
    }

    fn opt_name(&mut self) {
        if self.at_name() {
            let m = self.open();
            self.advance();
            self.close(m, Name);
        }
    }

    fn required_name(&mut self) {
        if self.at_name() {
            self.opt_name();
        } else {
            self.error("Expected identifier.".into());
        }
    }

    fn at_name(&self) -> bool {
        matches!(
            self.nth(0),
            Ident | MatchKw | WhenKw | ConstPi | ConstTau | ConstInf | ConstNan
        )
    }

    fn at_member_name(&self) -> bool {
        self.at_name()
            || matches!(
                self.nth(0),
                IfKw | ElifKw
                    | ElseKw
                    | ForKw
                    | WhileKw
                    | BreakKw
                    | ContinueKw
                    | PassKw
                    | ReturnKw
                    | VarKw
                    | ConstKw
                    | EnumKw
                    | FuncKw
                    | StaticKw
                    | SignalKw
                    | ClassKw
                    | ClassNameKw
                    | ExtendsKw
                    | IsKw
                    | InKw
                    | AsKw
                    | SelfKw
                    | SuperKw
                    | VoidKw
                    | AwaitKw
                    | PreloadKw
                    | AssertKw
                    | BreakpointKw
                    | NotKw
                    | AndKw
                    | OrKw
                    | YieldKw
                    | NamespaceKw
                    | TraitKw
                    | ConstPi
                    | ConstTau
                    | ConstInf
                    | ConstNan
            )
    }

    fn param_list(&mut self, what: &str) {
        let m = self.open();
        self.expect(LParen);
        let mut rest_seen = false;
        let mut default_seen = false;
        while !self.at_any(&[RParen, Newline, Dedent, Eof]) {
            let p = self.open();
            if rest_seen {
                self.error("Parameters cannot follow a rest parameter.".into());
            }
            let rest = self.eat(Ellipsis);
            if !self.at_name() {
                self.advance_with_error("Expected a parameter name.");
            } else {
                self.required_name();
            }
            if self.eat(Colon) && !self.at(Eq) {
                self.type_ref();
            }
            let default = self.eat(ColonEq) || self.eat(Eq);
            if default {
                if rest {
                    self.error("A rest parameter cannot have a default value.".into());
                }
                self.expr();
            } else if default_seen && !rest {
                self.error("Mandatory parameters cannot follow optional parameters.".into());
            }
            rest_seen |= rest;
            default_seen |= default;
            self.close(p, if rest { VarargParam } else { Param });
            if !self.eat(Comma) {
                break;
            }
        }
        self.expect_closing(RParen, what);
        self.close(m, ParamList);
    }

    fn type_ref(&mut self) {
        self.nested(|p| {
            let m = p.open();
            if p.at_name() || p.at(VoidKw) {
                p.advance();
                while p.eat(Dot) {
                    p.required_name();
                }
                if p.eat(LBrack) {
                    p.type_ref();
                    while p.eat(Comma) {
                        p.type_ref();
                    }
                    p.expect(RBrack);
                }
            } else {
                p.error("Expected a type.".to_owned());
            }
            p.close(m, TypeRef);
        })
    }

    fn property_body(&mut self) {
        let m = self.open();
        self.expect(Colon);
        if self.eat(Newline) && self.at(Indent) {
            self.advance();
            while !self.at(Dedent) && !self.eof() {
                if self.at_any(&[Newline, Semicolon, Comma]) {
                    self.advance();
                    continue;
                }
                self.accessor();
            }
            self.eat(Dedent);
        } else {
            self.accessor();
            while self.eat(Comma) {
                self.accessor();
            }
        }
        self.close(m, PropertyBody);
    }

    fn accessor(&mut self) {
        let kind = match self.cur_text() {
            "get" => Getter,
            "set" => Setter,

            _ => {
                self.advance_with_error("Expected \"get\" or \"set\" in a property accessor.");
                return;
            }
        };
        let m = self.open();
        self.advance();

        if self.eat(LParen) {
            if kind == Setter {
                self.required_name();
            }
            self.expect(RParen);
        }
        if self.eat(Colon) {
            self.block();
        } else if self.eat(Eq) {
            self.required_name();
        } else {
            self.error("Expected an accessor body or delegate.".into());
        }
        self.close(m, kind);
    }

    fn block(&mut self) -> bool {
        self.nested(|p| {
            if p.eat(Newline) {
                if p.at(Indent) {
                    let m = p.open();
                    p.advance();
                    while !p.at(Dedent) && !p.eof() {
                        if p.at_any(&[Newline, Semicolon]) {
                            p.advance();
                            continue;
                        }

                        if p.at(Indent) {
                            p.over_indented_region();
                            continue;
                        }
                        p.stmt();
                    }
                    p.eat(Dedent);
                    p.close(m, Block);
                    return true;
                }
                p.error("Expected an indented block.".into());
                false
            } else {
                let m = p.open();
                if p.at_any(&[Newline, Dedent, RParen, RBrack, RBrace, Comma, Eof]) {
                    p.error("Expected a block body.".into());
                }
                while !p.at_any(&[Newline, Dedent, RParen, RBrack, RBrace, Comma]) && !p.eof() {
                    if p.eat(Semicolon) {
                        continue;
                    }
                    p.stmt();
                }
                p.close(m, Block);
                false
            }
        })
    }

    fn over_indented_region(&mut self) {
        self.error("Unexpected indentation.".to_owned());
        let m = self.open();
        let mut depth = 0u32;
        self.advance();
        loop {
            match self.nth(0) {
                Indent => {
                    depth += 1;
                    self.advance();
                }
                Dedent => {
                    self.advance();
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                Eof => break,
                Newline | Semicolon => self.advance(),

                _ => self.stmt(),
            }
        }
        self.close(m, Block);
    }

    fn stmt(&mut self) {
        let start = self.pos;
        let annotation = self.at(At);
        self.statement();
        if !annotation {
            self.end_statement();
        }
        if self.pos == start && !self.eof() {
            self.advance_with_error("Expected a statement.");
        }
    }

    fn statement(&mut self) {
        match self.nth(0) {
            At => self.annotation(),
            IfKw => self.if_stmt(),
            ForKw => self.for_stmt(),
            WhileKw => self.while_stmt(),

            MatchKw if self.match_begins_statement() => self.match_stmt(),
            ReturnKw => self.return_stmt(),
            BreakKw => self.simple_stmt(BreakStmt),
            ContinueKw => self.simple_stmt(ContinueStmt),
            PassKw => self.simple_stmt(PassStmt),
            BreakpointKw => self.simple_stmt(BreakpointStmt),
            AssertKw => self.assert_stmt(),
            StaticKw if self.nth(1) == FuncKw => self.func_decl(),
            VarKw | StaticKw => self.var_decl(),
            ConstKw => self.const_decl(),
            _ => self.expr_stmt(),
        }
    }

    fn simple_stmt(&mut self, kind: SyntaxKind) {
        let m = self.open();
        self.advance();
        self.close(m, kind);
    }

    fn return_stmt(&mut self) {
        let m = self.open();
        self.expect(ReturnKw);
        if !self.at_any(&[Newline, Dedent, Semicolon, RParen, RBrack, RBrace, Comma]) && !self.eof()
        {
            self.expr();
        }
        self.close(m, ReturnStmt);
    }

    fn assert_stmt(&mut self) {
        let m = self.open();
        self.expect(AssertKw);
        if self.at(LParen) {
            self.arg_list();
        }
        self.close(m, AssertStmt);
    }

    fn if_stmt(&mut self) {
        let m = self.open();
        self.expect(IfKw);
        self.expr();
        self.expect_after(Colon, "\"if\" condition");
        self.block();
        while self.at_clause(ElifKw) {
            let e = self.open();
            self.advance();
            self.expr();
            self.expect_after(Colon, "\"elif\" condition");
            self.block();
            self.close(e, ElifClause);
        }
        if self.at_clause(ElseKw) {
            let e = self.open();
            self.advance();
            self.expect_after(Colon, "\"else\"");
            self.block();
            self.close(e, ElseClause);
        }
        self.close(m, IfStmt);
    }

    fn at_clause(&mut self, kw: SyntaxKind) -> bool {
        if self.at(kw) {
            return true;
        }
        if self.at(Newline) && self.nth(1) == kw {
            self.advance();
            return true;
        }
        false
    }

    fn for_stmt(&mut self) {
        let m = self.open();
        self.expect(ForKw);
        self.required_name();
        if self.eat(Colon) {
            self.type_ref();
        }
        if !self.eat(InKw) {
            self.error("Expected \"in\" or \":\" after \"for\" variable name.".to_owned());
        }
        self.expr();
        self.expect_after(Colon, "\"for\" condition");
        self.block();
        self.close(m, ForStmt);
    }

    fn while_stmt(&mut self) {
        let m = self.open();
        self.expect(WhileKw);
        self.expr();
        self.expect_after(Colon, "\"while\" condition");
        self.block();
        self.close(m, WhileStmt);
    }

    fn match_stmt(&mut self) {
        let m = self.open();
        self.expect(MatchKw);
        self.expr();
        self.expect_after(Colon, "\"match\" expression");
        if self.eat(Newline) && self.at(Indent) {
            self.advance();
            while !self.at(Dedent) && !self.eof() {
                if self.at_any(&[Newline, Semicolon]) {
                    self.advance();
                    continue;
                }
                if self.at(PassKw) {
                    self.simple_stmt(PassStmt);
                } else if self.at(At) {
                    self.annotation();
                } else {
                    let start = self.pos;
                    self.match_arm();
                    if self.pos == start {
                        self.advance_with_error("Expected a match arm.");
                    }
                }
            }
            self.eat(Dedent);
        }
        self.close(m, MatchStmt);
    }

    fn match_arm(&mut self) {
        let m = self.open();
        self.pattern();
        while self.eat(Comma) {
            if self.at(Colon) || self.at(WhenKw) {
                break;
            }
            self.pattern();
        }
        if self.at(WhenKw) {
            let g = self.open();
            self.advance();
            self.expr();
            self.close(g, PatternGuard);
        }
        if !self.eat(Colon) {
            self.error("Expected \":\" or \"when\" after \"match\" patterns.".to_owned());
        }
        self.block();
        self.close(m, MatchArm);
    }

    fn pattern(&mut self) {
        self.nested(|p| match p.nth(0) {
            Ident if p.cur_text() == "_" => {
                let m = p.open();
                p.advance();
                p.close(m, PatternWildcard);
            }
            VarKw => {
                let m = p.open();
                p.advance();
                p.required_name();
                p.close(m, PatternBind);
            }
            DotDot => {
                let m = p.open();
                p.advance();
                p.close(m, PatternRest);
            }
            LBrack => {
                let m = p.open();
                p.advance();
                while !p.at(RBrack) && !p.eof() {
                    p.pattern();
                    if !p.eat(Comma) {
                        break;
                    }
                }
                p.expect(RBrack);
                p.close(m, PatternArray);
            }
            LBrace => {
                let m = p.open();
                p.advance();
                while !p.at(RBrace) && !p.eof() {
                    p.pattern();
                    if p.eat(Colon) {
                        p.pattern();
                    }
                    if !p.eat(Comma) {
                        break;
                    }
                }
                p.expect(RBrace);
                p.close(m, PatternDict);
            }
            _ => {
                let m = p.open();
                p.expr();
                p.close(m, PatternLiteral);
            }
        })
    }

    fn expr_stmt(&mut self) {
        let m = self.open();
        if let Some(lhs) = self.expr() {
            if is_assignment(self.nth(0)) {
                let assign = self.open_before(lhs);
                if !self.is_assignment_target(lhs) {
                    self.error("Invalid assignment target.".into());
                }
                self.advance();
                self.expr();
                self.close(assign, AssignExpr);
            }
        } else if !self.at_any(&[Newline, Dedent, Semicolon])
            && !self.eof()
            && !self.at_any(RECOVERY)
        {
            self.advance_with_error("Expected a statement.");
        }
        self.close(m, ExprStmt);
    }

    fn is_assignment_target(&self, target: MarkClosed) -> bool {
        let mut index = target.0;
        loop {
            match self.events[index] {
                Event::Open {
                    kind: NameRef | FieldExpr | IndexExpr,
                    ..
                } => return true,
                Event::Open {
                    kind: ParenExpr, ..
                } => {
                    let Some(offset) = self.events[index + 1..]
                        .iter()
                        .take_while(|event| !matches!(event, Event::Close))
                        .position(|event| matches!(event, Event::Open { .. }))
                    else {
                        return false;
                    };
                    index += 1 + offset;
                    // The first open event may be an operand; follow forward parents
                    // to inspect the whole grouped expression without changing its CST.
                    while let Event::Open {
                        parent: Some(parent),
                        ..
                    } = self.events[index]
                    {
                        index = parent;
                    }
                }
                _ => return false,
            }
        }
    }

    fn expr(&mut self) -> Option<MarkClosed> {
        self.expr_bp(0)
    }

    fn expr_bp(&mut self, min_bp: u8) -> Option<MarkClosed> {
        self.nested(|p| {
            let mut lhs = p.lhs()?;
            loop {
                if p.pos > 0 && p.tokens[p.nontrivia[p.pos - 1]].kind == Dedent {
                    break;
                }
                let op = p.nth(0);

                if op == IfKw {
                    let (l, r) = bp(PREC_TERNARY, Assoc::Right);
                    if l < min_bp {
                        break;
                    }
                    let m = p.open_before(lhs);
                    p.advance();
                    p.expr_bp(0);
                    p.expect(ElseKw);
                    p.expr_bp(r);
                    lhs = p.close(m, TernaryExpr);
                    continue;
                }

                if op == IsKw {
                    let (l, _) = bp(PREC_TYPE_TEST, Assoc::Left);
                    if l < min_bp {
                        break;
                    }
                    let m = p.open_before(lhs);
                    p.advance();
                    p.eat(NotKw);
                    p.type_ref();
                    lhs = p.close(m, IsExpr);
                    continue;
                }

                if op == AsKw {
                    let (l, _) = bp(PREC_CAST, Assoc::Left);
                    if l < min_bp {
                        break;
                    }
                    let m = p.open_before(lhs);
                    p.advance();
                    p.type_ref();
                    lhs = p.close(m, CastExpr);
                    continue;
                }

                if op == InKw || (op == NotKw && p.nth(1) == InKw) {
                    let (l, r) = bp(PREC_IN, Assoc::Left);
                    if l < min_bp {
                        break;
                    }
                    let m = p.open_before(lhs);
                    if op == NotKw {
                        p.advance();
                    }
                    p.expect(InKw);
                    p.expr_bp(r);
                    lhs = p.close(m, InExpr);
                    continue;
                }

                let Some((prec, assoc)) = infix_prec(op) else {
                    break;
                };
                let (l, r) = bp(prec, assoc);
                if l < min_bp {
                    break;
                }
                let m = p.open_before(lhs);
                p.advance();
                p.expr_bp(r);
                lhs = p.close(m, BinExpr);
            }
            Some(lhs)
        })
    }

    fn lhs(&mut self) -> Option<MarkClosed> {
        let op = self.nth(0);
        let prefix = match op {
            NotKw | Bang => Some(PREC_NOT),
            Minus | Plus => Some(PREC_SIGN),
            Tilde => Some(PREC_BIT_NOT),
            AwaitKw => Some(PREC_AWAIT),
            _ => None,
        };
        if let Some(prec) = prefix {
            let m = self.open();
            self.advance();
            self.expr_bp(2 * prec);
            let kind = if op == AwaitKw { AwaitExpr } else { UnaryExpr };
            return Some(self.close(m, kind));
        }

        if self.at(FuncKw) {
            let (lam, multiline) = self.lambda();
            return Some(if multiline { lam } else { self.postfix(lam) });
        }
        let primary = self.primary()?;
        Some(self.postfix(primary))
    }

    fn postfix(&mut self, lhs: MarkClosed) -> MarkClosed {
        let mut lhs = lhs;
        loop {
            match self.nth(0) {
                LParen => {
                    let m = self.open_before(lhs);
                    self.arg_list();
                    lhs = self.close(m, CallExpr);
                }
                LBrack => {
                    let m = self.open_before(lhs);
                    self.advance();
                    self.expr();
                    self.expect(RBrack);
                    lhs = self.close(m, IndexExpr);
                }
                Dot => {
                    let m = self.open_before(lhs);
                    self.advance();
                    if self.at_member_name() {
                        let n = self.open();
                        self.advance();
                        self.close(n, NameRef);
                    } else {
                        self.error("Expected a member name.".to_owned());
                    }
                    lhs = self.close(m, FieldExpr);
                }
                _ => break,
            }
        }
        lhs
    }

    fn primary(&mut self) -> Option<MarkClosed> {
        match self.nth(0) {
            Int | Float | String | StringName | NodePath | True | False | Null | ConstPi
            | ConstTau | ConstInf | ConstNan => {
                let m = self.open();
                self.advance();
                Some(self.close(m, Literal))
            }
            Ident | SelfKw | SuperKw | MatchKw | WhenKw => {
                let m = self.open();
                self.advance();
                Some(self.close(m, NameRef))
            }
            LParen => {
                let m = self.open();
                self.advance();
                self.expr();
                self.expect_closing(RParen, "grouping expression");
                Some(self.close(m, ParenExpr))
            }
            LBrack => Some(self.array_lit()),
            LBrace => Some(self.dict_lit()),
            Dollar => Some(self.get_node(GetNodeExpr, Dollar)),
            Percent => Some(self.get_node(UniqueNodeExpr, Percent)),

            FuncKw => Some(self.lambda().0),
            PreloadKw => Some(self.preload_expr()),
            _ => {
                if self.at_any(&[Newline, Dedent, RParen, RBrack, RBrace, Comma]) || self.eof() {
                    self.error("Expected expression.".to_owned());
                    None
                } else {
                    Some(self.advance_with_error("Expected expression."))
                }
            }
        }
    }

    fn array_lit(&mut self) -> MarkClosed {
        let m = self.open();
        self.expect(LBrack);
        while !self.at(RBrack) && !self.eof() {
            if self.expr().is_none() {
                break;
            }
            if !self.eat(Comma) {
                break;
            }
        }
        self.expect(RBrack);
        self.close(m, ArrayLit)
    }

    fn dict_lit(&mut self) -> MarkClosed {
        let m = self.open();
        self.expect(LBrace);
        while !self.at(RBrace) && !self.eof() {
            let e = self.open();

            self.expr_bp(bp(PREC_ASSIGN, Assoc::Right).0 + 1);
            if self.eat(Colon) || self.eat(Eq) {
                self.expr();
            } else {
                self.error("Expected a dictionary entry value.".into());
            }
            self.close(e, DictEntry);
            if !self.eat(Comma) {
                break;
            }
        }
        self.expect(RBrace);
        self.close(m, DictLit)
    }

    fn get_node(&mut self, node: SyntaxKind, sigil: SyntaxKind) -> MarkClosed {
        let m = self.open();
        self.expect(sigil);
        if sigil == Dollar {
            self.eat(Slash);
            self.eat(Percent);
        }
        self.eat_node_path_segment();
        while self.eat(Slash) {
            self.eat(Percent);
            self.eat_node_path_segment();
        }
        self.close(m, node)
    }

    fn eat_node_path_segment(&mut self) {
        // Godot 4.7.2 uses Token::is_node_name() for both paths and member access.
        if self.at(String) || self.at_member_name() {
            self.advance();
        } else {
            self.error("Expected node path segment as string or identifier.".into());
        }
    }

    fn lambda(&mut self) -> (MarkClosed, bool) {
        let m = self.open();
        self.expect(FuncKw);
        self.opt_name();
        self.param_list("lambda parameters");
        if self.eat(Arrow) {
            self.type_ref();
        }
        self.expect(Colon);
        let multiline = self.block();
        (self.close(m, LambdaExpr), multiline)
    }

    fn preload_expr(&mut self) -> MarkClosed {
        let m = self.open();
        self.expect(PreloadKw);
        if self.at(LParen) {
            self.arg_list();
        }
        self.close(m, PreloadExpr)
    }

    fn arg_list(&mut self) {
        let m = self.open();
        self.expect(LParen);
        while !self.at(RParen) && !self.eof() {
            if self.expr().is_none() {
                break;
            }
            if !self.eat(Comma) {
                break;
            }
        }
        self.expect_closing(RParen, "call arguments");
        self.close(m, ArgList);
    }
}

macro_rules! operators {
    (assign { $($assign:ident)|+ } binary { $($prec:ident : $assoc:ident => $($op:ident)|+;)* }) => {
        fn is_assignment(kind: SyntaxKind) -> bool {
            matches!(kind, $($assign)|+)
        }

        fn infix_prec(kind: SyntaxKind) -> Option<(u8, Assoc)> {
            match kind {
                $($($op)|+ => Some(($prec, Assoc::$assoc)),)*
                _ => None,
            }
        }
    };
}

operators! {
    assign { Eq | PlusEq | MinusEq | StarEq | SlashEq | StarStarEq | PercentEq | AmpEq | PipeEq | CaretEq | ShlEq | ShrEq }
    binary {
        PREC_OR: Left => OrKw | PipePipe;
        PREC_AND: Left => AndKw | AmpAmp;
        PREC_CMP: Left => EqEq | Neq | Lt | Gt | Le | Ge;
        PREC_BIT_OR: Left => Pipe;
        PREC_BIT_XOR: Left => Caret;
        PREC_BIT_AND: Left => Amp;
        PREC_SHIFT: Left => Shl | Shr;
        PREC_ADD: Left => Plus | Minus;
        PREC_FACTOR: Left => Star | Slash | Percent;
        PREC_POWER: Left => StarStar;
    }
}

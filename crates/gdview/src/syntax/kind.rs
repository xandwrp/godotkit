macro_rules! syntax_kinds {
    (trivia { $($trivia:ident),* $(,)? }
     layout { $($layout:ident),* $(,)? }
     tokens { $($token:ident),* $(,)? }
     fixed { $($fixed:ident => $text:literal),* $(,)? }
     nodes { $($node:ident),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum SyntaxKind {
            $($trivia,)* $($layout,)* $($token,)* $($fixed,)* $($node,)*
        }

        impl SyntaxKind {
            pub const fn is_trivia(self) -> bool {
                matches!(self, $(Self::$trivia)|*)
            }

            pub const fn is_synthetic_layout(self) -> bool {
                matches!(self, $(Self::$layout)|*)
            }

            pub const fn is_node(self) -> bool {
                matches!(self, $(Self::$node)|*)
            }

            pub const fn fixed_text(self) -> Option<&'static str> {
                match self { $(Self::$fixed => Some($text),)* _ => None }
            }

            pub fn from_fixed_text(text: &str) -> Option<Self> {
                match text { $($text => Some(Self::$fixed),)* _ => None }
            }
        }
    };
}

syntax_kinds! {
    trivia { Whitespace, LineComment, DocComment, RegionComment, EndRegionComment, LineContinuation, NewlinePhys, Bom }
    layout { Newline, Indent, Dedent }
    tokens { Int, Float, String, StringName, NodePath, Ident, Error, Eof }
    fixed {
        True => "true",
        False => "false",
        Null => "null",
        ConstPi => "PI",
        ConstTau => "TAU",
        ConstInf => "INF",
        ConstNan => "NAN",
        IfKw => "if",
        ElifKw => "elif",
        ElseKw => "else",
        ForKw => "for",
        WhileKw => "while",
        MatchKw => "match",
        WhenKw => "when",
        BreakKw => "break",
        ContinueKw => "continue",
        PassKw => "pass",
        ReturnKw => "return",
        VarKw => "var",
        ConstKw => "const",
        EnumKw => "enum",
        FuncKw => "func",
        StaticKw => "static",
        SignalKw => "signal",
        ClassKw => "class",
        ClassNameKw => "class_name",
        ExtendsKw => "extends",
        IsKw => "is",
        InKw => "in",
        AsKw => "as",
        SelfKw => "self",
        SuperKw => "super",
        VoidKw => "void",
        AwaitKw => "await",
        PreloadKw => "preload",
        AssertKw => "assert",
        BreakpointKw => "breakpoint",
        NotKw => "not",
        AndKw => "and",
        OrKw => "or",
        YieldKw => "yield",
        NamespaceKw => "namespace",
        TraitKw => "trait",
        LParen => "(",
        RParen => ")",
        LBrack => "[",
        RBrack => "]",
        LBrace => "{",
        RBrace => "}",
        Comma => ",",
        Colon => ":",
        Semicolon => ";",
        Dot => ".",
        DotDot => "..",
        Ellipsis => "...",
        At => "@",
        Dollar => "$",
        Percent => "%",
        Amp => "&",
        Arrow => "->",
        ColonEq => ":=",
        Plus => "+",
        Minus => "-",
        Star => "*",
        Slash => "/",
        StarStar => "**",
        Eq => "=",
        EqEq => "==",
        Neq => "!=",
        Lt => "<",
        Gt => ">",
        Le => "<=",
        Ge => ">=",
        AmpAmp => "&&",
        PipePipe => "||",
        Bang => "!",
        Tilde => "~",
        Pipe => "|",
        Caret => "^",
        Shl => "<<",
        Shr => ">>",
        PlusEq => "+=",
        MinusEq => "-=",
        StarEq => "*=",
        SlashEq => "/=",
        StarStarEq => "**=",
        PercentEq => "%=",
        AmpEq => "&=",
        PipeEq => "|=",
        CaretEq => "^=",
        ShlEq => "<<=",
        ShrEq => ">>=",
    }
    nodes {
        SourceFile,
        ExtendsClause,
        ClassNameDecl,
        Annotation,
        AnnotationArgList,
        InnerClassDecl,
        ClassBody,
        FuncDecl,
        ParamList,
        Param,
        VarargParam,
        VarDecl,
        ConstDecl,
        EnumDecl,
        EnumVariant,
        SignalDecl,
        PropertyBody,
        Getter,
        Setter,
        Name,
        TypeRef,
        TypedArray,
        TypedDict,
        Block,
        IfStmt,
        ElifClause,
        ElseClause,
        ForStmt,
        WhileStmt,
        MatchStmt,
        MatchArm,
        ReturnStmt,
        BreakStmt,
        ContinueStmt,
        PassStmt,
        AssertStmt,
        BreakpointStmt,
        ExprStmt,
        VarStmt,
        PatternLiteral,
        PatternBind,
        PatternWildcard,
        PatternArray,
        PatternDict,
        PatternRest,
        PatternGuard,
        BinExpr,
        UnaryExpr,
        TernaryExpr,
        CastExpr,
        IsExpr,
        InExpr,
        CallExpr,
        ArgList,
        IndexExpr,
        FieldExpr,
        AwaitExpr,
        LambdaExpr,
        ParenExpr,
        ArrayLit,
        DictLit,
        DictEntry,
        NameRef,
        Literal,
        GetNodeExpr,
        UniqueNodeExpr,
        PreloadExpr,
        ErrorNode,
        AssignExpr,
    }
}

impl SyntaxKind {
    pub fn display_name(self) -> String {
        if let Some(text) = self.fixed_text() {
            return format!("{text:?}");
        }
        match self {
            Self::Ident => "identifier".into(),
            Self::Int | Self::Float | Self::String | Self::StringName | Self::NodePath => {
                "literal".into()
            }
            Self::Newline => "end of statement".into(),
            Self::Eof => "end of file".into(),
            _ => format!("{self:?}"),
        }
    }
}

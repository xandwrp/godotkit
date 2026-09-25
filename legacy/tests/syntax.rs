use gdkit::syntax::{
    Node, SyntaxKind as K,
    ast::{AstNode, Binary, Function},
    parse, tokenize,
};

fn assert_lossless(source: &str) {
    let parsed = parse(source);
    assert_eq!(parsed.root().text(), source);
    let mut cursor = 0;
    for token in parsed
        .root()
        .tokens()
        .filter(|token| !token.range.is_empty())
    {
        assert_eq!(token.range.start, cursor, "{source:?}");
        cursor = token.range.end;
    }
    assert_eq!(cursor, source.len());
    let rebuilt: String = parsed
        .root()
        .tokens()
        .map(|token| &source[token.range])
        .collect();
    assert_eq!(rebuilt, source);
    for node in parsed.root().descendants() {
        let text: String = node.tokens().map(|token| &source[token.range]).collect();
        assert_eq!(node.text(), text, "{:?}", node.kind());
    }
}

#[test]
fn declarations_and_typed_views() {
    let source = "@tool\nclass_name Example extends Node\nsignal changed(value: int)\nenum Mode { FIRST, SECOND = 2 }\nvar items: Dictionary[String, Array] = {}\n@abstract func compute(value: int) -> int\nstatic func collect(first: int, ...rest: Array) -> Array:\n\treturn rest\nclass Inner: pass\n";
    let parsed = parse(source);
    assert!(parsed.is_valid(), "{:?}", parsed.errors());
    let functions: Vec<_> = parsed
        .root()
        .children()
        .filter_map(Function::cast)
        .collect();
    assert_eq!(functions.len(), 2);
    assert_eq!(functions[0].name(), Some("compute"));
    assert!(functions[0].body().is_none());
    assert_eq!(functions[1].name(), Some("collect"));
    assert!(functions[1].body().is_some());
    assert_eq!(functions[1].parameters().count(), 2);
    assert!(functions[1].parameters().last().unwrap().is_variadic());
    assert_lossless(source);
}

#[test]
fn property_forms_and_match_patterns() {
    let source = "var x:\n\tget = read, set = write\nvar y: int:\n\tget(): return 2\n\tset(value): y = value\nfunc test(value):\n\tmatch value:\n\t\t@warning_ignore(\"unused_variable\")\n\t\t[var first, ..] when first > 0:\n\t\t\treturn first\n\t\t{\"key\": var item, ..}: return item\n\t\t_: return\n";
    let parsed = parse(source);
    assert!(parsed.is_valid(), "{:?}", parsed.errors());
    assert!(
        parsed
            .root()
            .descendants()
            .any(|node| node.kind() == K::PatternWildcard)
    );
    assert_lossless(source);
}

fn shape(node: Node<'_>) -> String {
    if let Some(binary) = Binary::cast(node) {
        let children: Vec<_> = binary.operands().collect();
        format!(
            "({} {} {})",
            binary.operator().unwrap().kind.fixed_text().unwrap(),
            shape(children[0]),
            shape(children[1])
        )
    } else {
        node.text().trim().to_string()
    }
}

#[test]
fn precedence_and_left_associative_power() {
    let parsed = parse("var x = 2 ** 3 ** 4 + 5 * 6\n");
    assert!(parsed.is_valid());
    let expression = parsed
        .root()
        .descendants()
        .find(|node| node.kind() == K::BinExpr)
        .unwrap();
    assert_eq!(shape(expression), "(+ (** (** 2 3) 4) (* 5 6))");
    assert_lossless(parsed.source());
}

#[test]
fn multiline_lambdas_and_continuations() {
    for source in [
        "func test():\n\tcall(func(a):\n\t\treturn a, 2)\n",
        "func test():\n\touter(func():\n\t\tinner(func():\n\t\t\treturn [\n\t\t\t\t1,\n\t\t\t]\n\t\t)\n\t)\n",
        "func test():\n\tvar f = func():\n\t\treturn\n\tif true:\n\t\tf()\n",
        "func test():\n\tif true \\\n\t\t# continued comment\n\t\tand false:\n\t\treturn\n",
        "var x = {\n\t\"key\":\n\t\t1,\n}\n",
        "func test():\n\tif true: return\n\telif false: pass\n\telse: return\n",
    ] {
        let parsed = parse(source);
        assert!(parsed.is_valid(), "{source:?}: {:?}", parsed.errors());
        assert_lossless(source);
    }
}

#[test]
fn unicode_strings_and_line_endings() {
    for source in [
        "\u{feff}var caf\u{e9} = &\"name\"\r\nvar e\u{301} = ^'path'\r",
        "var s = r\"line one\nline two\"\n",
        "var s = \"\"\"quoted ' and \\\"\ntext\"\"\"\n",
        "var s = \"\\u2023\\U01f600\"\n",
        "#region example\n## docs\n# comment\n#endregion\n",
        "var n = 0xFF + 0b10 + .5 + 2. + 4e-2 + 1_000\n",
    ] {
        let parsed = parse(source);
        assert!(parsed.is_valid(), "{source:?}: {:?}", parsed.errors());
        assert_lossless(source);
    }
}

#[test]
fn malformed_input_reports_errors_and_preserves_source() {
    for source in [
        "var = 1\n",
        "func (): pass\n",
        "func test():\n",
        "func test():\npass\n",
        "var x = \"unterminated",
        "var x = \"\\q\"\n",
        "var x = 0x\n",
        "var x = 0b102\n",
        "var x = 1e+\n",
        "var x = [1, 2}\n",
        "var x = 1 var y = 2\n",
        "func test(): return 1 = 2\n",
        "func test(): 1 = 2\n",
        "func test(): call(a = 2)\n",
        "func test(...rest, value): pass\n",
        "func test(a = 1, b): pass\n",
        "func test():\n \tpass\n",
        "\tvar x = 1\n",
    ] {
        assert!(
            !parse(source).is_valid(),
            "unexpectedly accepted {source:?}"
        );
        assert_lossless(source);
    }
}

#[test]
fn recovery_reaches_later_declarations() {
    let source = "var broken = )\nfunc intact(value):\n\treturn value\n";
    let parsed = parse(source);
    assert!(!parsed.is_valid());
    assert!(
        parsed
            .root()
            .children()
            .filter_map(Function::cast)
            .any(|function| function.name() == Some("intact"))
    );
    assert_lossless(source);
}

#[test]
fn bounded_nesting_and_long_operator_chains() {
    let nested = format!("var x = {}0{}\n", "[".repeat(1000), "]".repeat(1000));
    let parsed = parse(&nested);
    assert!(
        parsed
            .errors()
            .iter()
            .any(|error| error.message.contains("nesting limit"))
    );
    let rebuilt: String = parsed
        .root()
        .tokens()
        .map(|token| &nested[token.range.range()])
        .collect();
    assert_eq!(rebuilt, nested);
    let long = format!("var x = 0{}\n", " + 1".repeat(10_000));
    let parsed = parse(&long);
    assert!(parsed.is_valid());
    assert_eq!(
        parsed
            .root()
            .descendants()
            .filter(|node| node.kind() == K::BinExpr)
            .count(),
        10_000
    );
    assert_eq!(
        parsed
            .root()
            .tokens()
            .filter(|token| token.kind == K::Plus)
            .count(),
        10_000
    );
}

#[test]
fn deterministic_malformed_input_stress() {
    let alphabet = [
        "func",
        "var",
        "if",
        "else",
        "match",
        "when",
        "class",
        "return",
        "pass",
        "x",
        "1",
        "\"",
        "'",
        "(",
        ")",
        "[",
        "]",
        "{",
        "}",
        ":",
        ",",
        "=",
        "+",
        "\n",
        "\t",
        "#",
        "\\",
        "\u{e9}",
        "\u{1f600}",
    ];
    let mut state = 0x72a4b539u64;
    for _ in 0..2500 {
        let mut source = String::new();
        for _ in 0..48 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            source.push_str(alphabet[(state >> 32) as usize % alphabet.len()]);
            source.push(' ');
        }
        assert_lossless(&source);
    }
}

#[test]
fn lexer_uses_longest_operator_and_retains_every_byte() {
    let source = "**= ** *= * >>= >> >= > ... .. . := :";
    let (tokens, errors) = tokenize(source);
    assert!(errors.is_empty());
    let kinds: Vec<_> = tokens
        .iter()
        .filter(|token| !token.kind.is_trivia())
        .map(|token| token.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            K::StarStarEq,
            K::StarStar,
            K::StarEq,
            K::Star,
            K::ShrEq,
            K::Shr,
            K::Ge,
            K::Gt,
            K::Ellipsis,
            K::DotDot,
            K::Dot,
            K::ColonEq,
            K::Colon
        ]
    );
    assert_eq!(
        tokens
            .iter()
            .map(|token| &source[token.range])
            .collect::<String>(),
        source
    );
}

#[test]
fn truncated_programs_remain_lossless() {
    let source = "@tool\nclass_name Example extends Node\nvar values: Dictionary[String, Array] = {}\nfunc test():\n\tcall(func(value):\n\t\tif value:\n\t\t\treturn [1, 2]\n\t\treturn []\n\t)\n";
    for end in source
        .char_indices()
        .map(|(offset, _)| offset)
        .chain([source.len()])
    {
        assert_lossless(&source[..end]);
    }
}

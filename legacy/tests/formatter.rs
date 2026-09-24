use gdkit::{
    formatter::{Options, format_source},
    syntax::parse,
};

fn script(body: &str, indent: &str) -> String {
    if body.starts_with("func ") {
        return body.to_owned();
    }
    let newline = if body.contains("\r\n") { "\r\n" } else { "\n" };
    let mut source = format!("func f():{newline}");
    source.push_str(indent);
    source.push_str("pass");
    source.push_str(newline);
    for line in body.split_inclusive('\n') {
        source.push_str(indent);
        source.push_str(line);
    }
    source
}

fn check(before: &str, after: &str) {
    let indent = if before.contains('\t') { "\t" } else { "    " };
    let before = script(before, indent);
    let after = script(after, indent);
    let parsed_after = parse(&after);
    let strings: Vec<_> = parsed_after
        .tokens()
        .iter()
        .filter(|token| {
            matches!(
                token.kind,
                gdkit::syntax::SyntaxKind::String
                    | gdkit::syntax::SyntaxKind::StringName
                    | gdkit::syntax::SyntaxKind::NodePath
            )
        })
        .map(|token| token.range)
        .collect();
    let mut offset = 0;
    let after: String = after
        .split_inclusive('\n')
        .map(|line| {
            let protected = strings
                .iter()
                .any(|range| range.start < offset && offset < range.end);
            offset += line.len();
            if protected {
                return line.to_owned();
            }
            let body = line.trim_start_matches(' ');
            let tabs = (line.len() - body.len()) / 4;
            let ending = if body.ends_with("\r\n") {
                "\r\n"
            } else if body.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            let body = body.trim_end_matches([' ', '\t', '\r', '\n']);
            format!(
                "{}{body}{ending}",
                "\t".repeat(if body.is_empty() { 0 } else { tabs })
            )
        })
        .collect();
    let before = before.as_str();
    let formatted = format_source(before, &Options::default()).unwrap();
    assert_eq!(formatted, after);
    assert_eq!(
        format_source(&formatted, &Options::default()).unwrap(),
        formatted
    );
    assert!(parse(&formatted).is_valid());
}

#[test]
fn compacts_guards_at_nested_indentation() {
    check(
        "func f():\n\tif ready:\n\t\treturn\n\twork()\n",
        "func f():\n\tif ready: return\n\twork()\n",
    );
    check(
        "func f():\n    if outer:\n        if inner:\n            return\n    work()\n",
        "func f():\n    if outer:\n        if inner: return\n    work()\n",
    );
}

#[test]
fn preserves_line_endings_and_missing_final_newline() {
    check("if ready:\r\n\treturn\r\n", "if ready: return\r\n");
    check("if ready:\n    return", "if ready: return");
}

#[test]
fn preserves_comments_and_other_statements() {
    for source in [
        "if ready: # reason\n    return\n",
        "if ready:\n    return # reason\n",
        "if ready:\n    # reason\n    return\n",
        "if ready:\n    return\n    # reason\n",
        "if ready:\n    return\n    work()\n",
        "if ready:\n\n    return\n",
        "if ready: return\n",
        "if ready:\n    return;\n",
    ] {
        check(source, source);
    }
    check("if ready:\n    return value\n", "if ready: return value\n");
    check("if ready:\n    break\n", "if ready: break\n");
}

#[test]
fn protects_strings_and_continuations() {
    for source in [
        "var text = \"\"\"\nif ready:\n    return\n\"\"\"\n",
        "var text = '''\nif ready:\n    return\n'''\n",
        "var text = r\"escaped \\\" quote\"\n",
        "var text = \"日本語 😀 # []\"\n",
        "if (\n    ready\n):\n    return\n",
        "if ready \\\n    and waiting:\n    return\n",
    ] {
        check(source, source);
    }
    check(
        "if text == \"#:\":\n    return\n",
        "if text == \"#:\": return\n",
    );
}

#[test]
fn handles_adjacent_guards_and_else() {
    check(
        "if a:\n    return\nif b:\n    return\n",
        "if a: return\nif b: return\n",
    );
    check(
        "if a:\n    return\nelse:\n    work()\n",
        "if a: return\nelse: work()\n",
    );
}

#[test]
fn respects_visual_width_with_tabs() {
    let source = "func f():\n\tif ready:\n\t\treturn\n";
    assert_eq!(
        format_source(
            source,
            &Options {
                line_width: 20,
                tab_width: 4
            }
        )
        .unwrap(),
        "func f():\n\tif ready: return\n"
    );
    assert_eq!(
        format_source(
            source,
            &Options {
                line_width: 19,
                tab_width: 4
            }
        )
        .unwrap(),
        source
    );
}

#[test]
fn reports_syntax_errors_without_panicking() {
    for source in ["var x = \"unterminated", "var x = [)", "var x = ("] {
        assert!(
            format_source(source, &Options::default()).is_err(),
            "{source}"
        );
    }
    for source in ["", "# trailing", "var π = 3.14\n"] {
        assert_eq!(format_source(source, &Options::default()).unwrap(), source);
    }
}

#[test]
fn preserves_newlines_inside_single_quoted_strings() {
    for source in [
        "var x = r\"first\nif ready:\n    return\nlast\"\n",
        "var x = \"first\nlast\"\n",
    ] {
        check(source, source);
    }
}

#[test]
fn dedented_comments_do_not_end_a_suite() {
    let source = "if ready:\n    return\n# explanation\n    work()\n";
    check(source, source);
    check(
        "if ready:\n    return\n# explanation\nwork()\n",
        "if ready: return\n# explanation\nwork()\n",
    );
}

#[test]
fn rejects_invalid_syntax_even_after_a_valid_guard() {
    for source in [
        "if ready:\n    return\n",
        "func f():\n    if :\n        return\n",
        "func f():\n    if ready:\n        return\n    var value =\n",
        "func f():\n    if ready:\n        return\n  pass\n",
        "var value = \"bad\\q\"\n",
    ] {
        let parsed = parse(source);
        let error = format_source(source, &Options::default()).unwrap_err();
        assert_eq!(&error, &parsed.errors()[0]);
    }
}

#[test]
fn preserves_branch_boundaries_and_statement_separators() {
    check(
        "if a:\n    return\nelif b:\n    return\nelse:\n    if c:\n        return\n",
        "if a: return\nelif b: return\nelse:\n    if c: return\n",
    );
    for source in [
        "if a:\n    return; work()\n",
        "if a:\n    return\n    ;\n",
        "if a: return; work()\n",
        "if a:\n    return\n    ## explanation\n",
        "if a:\n    return\n    #region explanation\n",
    ] {
        check(source, source);
    }
    check("if a:  \n    return  \n", "if a: return  \n");
}

#[test]
fn formats_guards_in_lambdas_and_match_arms() {
    check(
        "var callback = func():\n    if ready:\n        return\ncallback.call()\n",
        "var callback = func():\n    if ready: return\ncallback.call()\n",
    );
    check(
        "match value:\n    1:\n        if ready:\n            return\n    _:\n        pass\n",
        "match value:\n    1:\n        if ready: return\n    _: pass\n",
    );
}

fn exact(before: &str, after: &str) {
    let formatted = format_source(before, &Options::default()).unwrap();
    assert_eq!(formatted, after);
    assert!(parse(&formatted).is_valid());
    assert_eq!(
        format_source(&formatted, &Options::default()).unwrap(),
        formatted
    );
}

#[test]
fn cleans_character_script_spacing() {
    exact(
        "# autoload\nextends Node2D\n\n@onready var instance_container: Node2D = %CharacterInstances\nvar _live_instances: Array[Node2D] = []\nfunc spawn_character(definition_path: String):\n  var _ci = CharacterSpawner.build_character_from_file(definition_path)  \n  \n  _live_instances.append(_ci)\n  instance_container.add_child(_ci)\n  \nfunc clear_instances() -> void:\n  _live_instances.clear()\n\n\n",
        "# autoload\nextends Node2D\n\n@onready var instance_container: Node2D = %CharacterInstances\n\nvar _live_instances: Array[Node2D] = []\n\n\nfunc spawn_character(definition_path: String):\n\tvar _ci = CharacterSpawner.build_character_from_file(definition_path)\n\n\t_live_instances.append(_ci)\n\tinstance_container.add_child(_ci)\n\n\nfunc clear_instances() -> void: _live_instances.clear()\n",
    );
}

#[test]
fn orders_fields_with_their_annotations_and_comments() {
    exact(
        "extends Node\nvar value = 1\n# Scene node\n@onready var child = $Child\n@export_range(0, 10)\nvar speed = 2\nconst LIMIT = 10\nconst MINIMUM = 0\nvar _internal = 3\nfunc run():\n    pass\n",
        "extends Node\n\nconst LIMIT = 10\nconst MINIMUM = 0\n\n@export_range(0, 10)\nvar speed = 2\n\n# Scene node\n@onready var child = $Child\n\nvar value = 1\n\nvar _internal = 3\n\n\nfunc run(): pass\n",
    );
}

#[test]
fn preserves_semantic_groups_and_function_documentation() {
    exact(
        "var a = 1\nvar b = 2\n\n\nvar c = 3\n## Does work\n@rpc\nfunc run():\n    pass\n## Stops work\nfunc stop():\n    pass\n",
        "var a = 1\nvar b = 2\n\nvar c = 3\n\n\n## Does work\n@rpc\nfunc run(): pass\n\n\n## Stops work\nfunc stop(): pass\n",
    );
}

#[test]
fn cleans_nested_classes_and_empty_files() {
    exact(" \n\t\n", "");
    exact(
        "class Inner:\n  var a = 1\n  var _b = 2\n  func run():\n    pass\n  func stop():\n    pass\n",
        "class Inner:\n\tvar a = 1\n\n\tvar _b = 2\n\n\n\tfunc run(): pass\n\n\n\tfunc stop(): pass\n",
    );
}

#[test]
fn preserves_multiline_string_whitespace() {
    exact(
        "var text = \"\"\"first  \n    \n  last\"\"\"\n",
        "var text = \"\"\"first  \n    \n  last\"\"\"\n",
    );
}

#[test]
fn respects_export_groups_and_groups_export_variants() {
    exact(
        "var before = 1\n@export_group(\"Movement\")\n@export var speed = 2\n@export_range(0, 10) var acceleration = 3\nvar after = 4\n",
        "@export_group(\"Movement\")\n@export var speed = 2\n@export_range(0, 10) var acceleration = 3\n\nvar before = 1\nvar after = 4\n",
    );
}

#[test]
fn normalizes_independent_indent_widths_and_lambda_suites() {
    exact(
        "func first():\n  if ready:\n    work()\nfunc second():\n    work()\n",
        "func first():\n\tif ready: work()\n\n\nfunc second(): work()\n",
    );
    exact(
        "var callbacks = [\n    func():\n        work(),\n]\n",
        "var callbacks = [\n\tfunc():\n\t\twork(),\n]\n",
    );
}

#[test]
fn preserves_class_documentation_when_ordering_fields() {
    exact(
        "extends Node\n## Class documentation.\nvar value=1\nconst LIMIT=2\n",
        "extends Node\n## Class documentation.\nconst LIMIT = 2\n\nvar value = 1\n",
    );
    exact(
        "class Inner:\n  ## Inner documentation.\n  var value=1\n  const LIMIT=2\n",
        "class Inner:\n\t## Inner documentation.\n\tconst LIMIT = 2\n\n\tvar value = 1\n",
    );
    exact(
        "# License header.\nvar value=1\nconst LIMIT=2\n",
        "# License header.\nconst LIMIT = 2\n\nvar value = 1\n",
    );
}

#[test]
fn follows_field_category_and_visibility_order() {
    exact(
        "@onready var child=$Child\nvar _private=1\n@export var speed=2\nstatic var cache={}\n@onready var _hidden=$Hidden\n@export var _tuning=5\nstatic var _static_private=6\nvar public=3\nconst LIMIT=4\n",
        "const LIMIT = 4\n\n@export var speed = 2\n@export var _tuning = 5\n\n@onready var child = $Child\n@onready var _hidden = $Hidden\n\nstatic var cache = {}\nvar public = 3\n\nvar _private = 1\nstatic var _static_private = 6\n",
    );
}

#[test]
fn moves_all_script_signals_and_fields_before_other_declarations() {
    exact(
        "extends Node\nsignal changed\n\nsignal requested\nfunc run():\n    pass\nvar public = 1\nconst LIMIT = 2\n@onready var child = $Child\nvar _private = 3\n@export var speed = 4\n# Completion signal\nsignal completed(value: int)\nenum State { IDLE }\n",
        "extends Node\n\nsignal changed\n\nsignal requested\n# Completion signal\nsignal completed(value: int)\n\nconst LIMIT = 2\n\n@export var speed = 4\n\n@onready var child = $Child\n\nvar public = 1\n\nvar _private = 3\n\n\nfunc run(): pass\n\n\nenum State { IDLE }\n",
    );
}

#[test]
fn preserves_single_blank_lines_within_field_groups() {
    exact(
        "@export_group(\"Meshes\")\n@export var body: Node3D\n\n@export_group(\"Parameters\")\n@export var speed = 1.0\nvar second = 2\n\nvar first = 1\nvar _second = 4\n\nvar _first = 3\n",
        "@export_group(\"Meshes\")\n@export var body: Node3D\n\n@export_group(\"Parameters\")\n@export var speed = 1.0\n\nvar second = 2\n\nvar first = 1\n\nvar _second = 4\n\nvar _first = 3\n",
    );
}

#[test]
fn normalizes_inline_token_spacing() {
    exact(
        "func f(a:int,b=2)->void:\n  var positive=+1\n  var negative=-2\n  var child=%Child\n  var math=a%b**2\n  var dict={\"a\":1,\"b\":-2}\n  if(a==b&&not child):\n    print( a,b )# note\n",
        "func f(a: int, b = 2) -> void:\n\tvar positive = +1\n\tvar negative = -2\n\tvar child = %Child\n\tvar math = a % b ** 2\n\tvar dict = { \"a\": 1, \"b\": -2 }\n\tif (a == b && not child):\n\t\tprint(a, b) # note\n",
    );
}

#[test]
fn does_not_join_tokens_across_physical_lines() {
    exact(
        "func f():\n  var values = [\n    1,\n    -2,\n  ]\n  var result = (\n      values[0]\n      + values[1]\n  )\n",
        "func f():\n\tvar values = [\n\t\t1,\n\t\t-2,\n\t]\n\tvar result = (\n\t\t\tvalues[0]\n\t\t\t+ values[1]\n\t)\n",
    );
}

#[test]
fn preserves_disabled_formatting_regions() {
    exact(
        "var outside=1\n# gdkit: off\nvar raw   =   {\"x\":1}  \n# gdkit: on\nvar after=2\n",
        "var outside = 1\n# gdkit: off\nvar raw   =   {\"x\":1}  \n# gdkit: on\nvar after = 2\n",
    );
}

#[test]
fn normalizes_collection_trailing_commas() {
    exact(
        "var single=[1,2,]\nvar values=[\n  1,\n  2\n]\nvar mapping={\n  \"value\":1 # note\n}\nenum State {IDLE,RUN,}\n",
        "var single = [1, 2]\nvar values = [\n\t1,\n\t2,\n]\nvar mapping = {\n\t\"value\": 1, # note\n}\n\nenum State { IDLE, RUN }\n",
    );
}

#[test]
fn wraps_long_call_arguments_one_per_line() {
    exact(
        "func f():\n  mesh.rotation.y=lerp_angle(mesh.rotation.y,wheel_turn_degrees*(PI/180),wheel_turn_speed*delta)\n",
        "func f():\n\tmesh.rotation.y = lerp_angle(\n\t\tmesh.rotation.y,\n\t\twheel_turn_degrees * (PI / 180),\n\t\twheel_turn_speed * delta,\n\t)\n",
    );
    exact(
        "func f():\r\n  result=calculate_really_long_value(first_parameter,second_parameter,third_parameter) # retained\r\n",
        "func f():\r\n\tresult = calculate_really_long_value(\r\n\t\tfirst_parameter,\r\n\t\tsecond_parameter,\r\n\t\tthird_parameter,\r\n\t) # retained\r\n",
    );
}

#[test]
fn recursively_wraps_nested_calls_that_remain_too_wide() {
    let source = "func f():\n\tvar result = outer(inner(first_value, second_value), third_value)\n";
    let expected = "func f():\n\tvar result = outer(\n\t\tinner(\n\t\t\tfirst_value,\n\t\t\tsecond_value,\n\t\t),\n\t\tthird_value,\n\t)\n";
    let options = Options {
        line_width: 30,
        ..Options::default()
    };
    let formatted = format_source(source, &options).unwrap();
    assert_eq!(formatted, expected);
    assert_eq!(format_source(&formatted, &options).unwrap(), formatted);
    assert!(parse(&formatted).is_valid());
}

#[test]
fn leaves_calls_at_the_width_limit_inline() {
    let source = "func f():\n\tprint(first, second)\n";
    let options = Options {
        line_width: 25,
        ..Options::default()
    };
    assert_eq!(format_source(source, &options).unwrap(), source);
}

#[test]
fn compacts_single_simple_statement_suites() {
    exact(
        "func stub() -> void:\n  pass\nfunc identity(value: int) -> int:\n  return value\nfunc execute() -> void:\n  work()\n",
        "func stub() -> void: pass\n\n\nfunc identity(value: int) -> int: return value\n\n\nfunc execute() -> void: work()\n",
    );
    exact(
        "var value:\n  get:\n    return backing\n  set(new_value):\n    backing=new_value\n",
        "var value:\n\tget: return backing\n\tset(new_value): backing = new_value\n",
    );
    check(
        "if ready:\n    pass\nelif waiting:\n    return value\nelse:\n    work()\nfor item in items:\n    inspect(item)\nwhile running:\n    break\n",
        "if ready: pass\nelif waiting: return value\nelse: work()\nfor item in items: inspect(item)\nwhile running: break\n",
    );
}

#[test]
fn column_wrapping_is_the_terminal_formatting_phase() {
    let source = "func calculate() -> float:\n\treturn lerp_angle(first_really_long_parameter, second_really_long_parameter, third_really_long_parameter)\n";
    let inline = "func calculate() -> float: return lerp_angle(first_really_long_parameter, second_really_long_parameter, third_really_long_parameter)\n";
    let expected = "func calculate() -> float:\n\treturn lerp_angle(\n\t\tfirst_really_long_parameter,\n\t\tsecond_really_long_parameter,\n\t\tthird_really_long_parameter,\n\t)\n";
    let options = Options {
        line_width: 60,
        ..Options::default()
    };
    let formatted = format_source(source, &options).unwrap();
    assert_eq!(formatted, expected);
    assert_eq!(format_source(inline, &options).unwrap(), expected);
    assert_eq!(format_source(&formatted, &options).unwrap(), formatted);
    assert!(parse(&formatted).is_valid());
}

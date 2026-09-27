use gdview::syntax::{ast, parse};

#[test]
fn rpc_callees_keep_receiver_structure_and_literal_kinds() {
    let parsed =
        parse("func send():\n    get_node(\"Target.With.Dot\").fire.rpc_id(1, &\"payload\")\n");
    assert!(parsed.is_valid());
    let calls: Vec<_> = parsed
        .root()
        .descendants()
        .filter_map(ast::CallExpr::cast)
        .collect();
    let callee = ast::FieldExpr::cast(calls[0].callee().unwrap()).unwrap();
    assert_eq!(callee.name(), Some("rpc_id"));
    let method = ast::FieldExpr::cast(callee.receiver().unwrap()).unwrap();
    assert_eq!(method.name(), Some("fire"));
    let receiver = ast::CallExpr::cast(method.receiver().unwrap()).unwrap();
    assert_eq!(receiver.callee_text(), "get_node");
    assert_eq!(
        ast::string_literal(receiver.arguments()[0]).as_deref(),
        Some("Target.With.Dot")
    );
    let name = calls[0].arguments()[1];
    assert!(ast::string_literal(name).is_none());
    assert_eq!(
        ast::string_or_name_literal(name).as_deref(),
        Some("payload")
    );
    let parsed = parse("var path = ^\"Player/Sync\"\n");
    let ast::Member::Var(var) = ast::SourceFile::cast(parsed.root())
        .unwrap()
        .members()
        .next()
        .unwrap()
    else {
        panic!()
    };
    let value = var.initializer().unwrap();
    assert_eq!(
        ast::node_path_literal(value).as_deref(),
        Some("Player/Sync")
    );
    assert!(ast::string_or_name_literal(value).is_none());
}

#[test]
fn lexical_bindings_include_parameters_loops_patterns_and_lambda_arguments() {
    let parsed = parse(
        "func send(arg):\n    var callback = func(shadow): pass\n    for item in []:\n        pass\n    match arg:\n        var bound:\n            pass\n",
    );
    assert!(parsed.is_valid(), "{:?}", parsed.diagnostics());
    let names: Vec<_> = parsed
        .root()
        .descendants()
        .filter_map(ast::binding_name)
        .collect();
    assert_eq!(names, ["arg", "callback", "shadow", "item", "bound"]);
}

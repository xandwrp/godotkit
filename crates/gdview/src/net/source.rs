use super::*;
use crate::declarations::{self, InnerClassDeclaration, MemberDeclaration};
use crate::syntax::{self, Node, SyntaxKind as K, ast};
use std::collections::BTreeSet;

/// Recover observations even from damaged source. No engine, I/O, evaluation,
/// or global method-name matching. Calls remain unlinked until `analyze`.
pub fn scan_script(path: ResPath, source: &str) -> ScriptObservations {
    let indexed = declarations::index_script(path.clone(), source);
    let parsed = syntax::parse(source);
    let mut result = ScriptObservations {
        script: path.clone(),
        endpoints: vec![],
        calls: vec![],
        authority_uses: vec![],
        contexts: vec![],
        unknowns: vec![],
        bindings: vec![],
    };
    if let Some(error) = indexed.parse_error {
        unknown(
            &mut result.unknowns,
            &path,
            parsed
                .diagnostics()
                .first()
                .map_or(1, |d| parsed.line_col(d.range.start).0),
            format!("incomplete script parse: {error}"),
        );
    }
    let mut methods = BTreeSet::new();
    endpoints(
        &indexed.declaration.members,
        None,
        &mut result,
        &mut methods,
    );
    fn inner(
        classes: &[InnerClassDeclaration],
        result: &mut ScriptObservations,
        methods: &mut BTreeSet<(Option<String>, String)>,
    ) {
        for class in classes {
            endpoints(&class.members, Some(&class.qualified_name), result, methods);
            inner(&class.inner_classes, result, methods);
        }
    }
    inner(
        &indexed.declaration.inner_classes,
        &mut result,
        &mut methods,
    );
    let outer_name = indexed
        .declaration
        .class_name
        .as_ref()
        .map(|n| n.name.as_str());
    for node in parsed.root().descendants() {
        let class = class_scope(node, outer_name);
        if let Some(var) = ast::VarDecl::cast(node)
            && enclosing_function(node).is_none()
            && ast::Member::Var(var)
                .annotations()
                .any(|a| a.name() == "onready")
            && let (Some(name), Some(path)) = (
                ast::Member::Var(var).name(),
                var.initializer().and_then(node_path),
            )
        {
            result.bindings.push(NodeBinding {
                class: class.clone(),
                name: name.into(),
                path: NodePath(path),
            });
        }
        let Some(call) = ast::CallExpr::cast(node) else {
            continue;
        };
        let Some(callee) = call.callee() else {
            continue;
        };
        let field = ast::FieldExpr::cast(callee);
        let name = field
            .and_then(|f| f.name())
            .or_else(|| ast::NameRef::cast(callee).map(|n| n.text()));
        let Some(name) = name else { continue };
        let receiver = field.and_then(|f| f.receiver());
        let args = call.arguments();
        let location = SourceLocation {
            path: path.clone(),
            line: node.line(),
        };
        if matches!(name, "rpc" | "rpc_id") {
            let mut observed = RpcCall {
                location: location.clone(),
                class: class.clone(),
                expression: node.trimmed_text().into(),
                form: if name == "rpc" {
                    CallForm::Rpc
                } else {
                    CallForm::RpcId
                },
                receiver: receiver.map(|r| r.trimmed_text().into()),
                receiver_is_local: receiver
                    .and_then(ast::NameRef::cast)
                    .is_some_and(|n| blocks_global_receiver(node, n.text())),
                method: None,
                target_peer: (name == "rpc_id")
                    .then(|| args.first().map(|n| n.trimmed_text().to_owned()))
                    .flatten(),
                candidates: vec![],
            };
            if receiver.is_some_and(is_multiplayer)
                && name == "rpc"
                && !shadowed(node, "multiplayer")
            {
                observed.form = CallForm::MultiplayerRpc;
                observed.target_peer = args.first().map(|n| n.trimmed_text().into());
                observed.receiver = args.get(1).map(|n| n.trimmed_text().into());
                observed.receiver_is_local = args
                    .get(1)
                    .and_then(|n| ast::NameRef::cast(*n))
                    .is_some_and(|n| blocks_global_receiver(node, n.text()));
                observed.method = args.get(2).copied().and_then(ast::string_or_name_literal);
                if args.len() < 3 || args.len() > 4 {
                    unknown(
                        &mut result.unknowns,
                        &path,
                        node.line(),
                        "MultiplayerAPI.rpc expects 3 or 4 arguments",
                    );
                }
            } else if let Some(target) =
                receiver.and_then(|r| callable_target(r, node, &class, &methods))
            {
                observed.form = CallForm::CallableRpc;
                observed.receiver = target.0;
                observed.receiver_is_local = observed
                    .receiver
                    .as_deref()
                    .is_some_and(|name| blocks_global_receiver(node, name));
                observed.method = target.1;
            } else {
                let self_receiver = receiver.is_none_or(|r| r.trimmed_text() == "self");
                let overridden = self_receiver
                    && (methods.contains(&(class.clone(), name.into())) || shadowed(node, name));
                let known_node = receiver.is_none_or(|r| is_node(r, node)) && !overridden;
                if !known_node {
                    observed.form = CallForm::AmbiguousRpc;
                    unknown(
                        &mut result.unknowns,
                        &path,
                        node.line(),
                        "RPC-like call has an unproven receiver type (Node, Callable, or custom method)",
                    );
                } else {
                    observed.method = args
                        .get(usize::from(name == "rpc_id"))
                        .copied()
                        .and_then(ast::string_or_name_literal);
                }
            }
            if observed.method.is_none() {
                unknown(
                    &mut result.unknowns,
                    &path,
                    node.line(),
                    "RPC method is dynamic, missing, or receiver-dependent",
                );
            }
            if name == "rpc_id" && args.is_empty() {
                unknown(
                    &mut result.unknowns,
                    &path,
                    node.line(),
                    "rpc_id is missing its target peer",
                );
            }
            result.calls.push(observed);
        }
        let node_authority = matches!(
            name,
            "get_multiplayer_authority" | "is_multiplayer_authority" | "set_multiplayer_authority"
        );
        let api_authority = matches!(name, "is_server" | "get_remote_sender_id" | "get_unique_id");
        let overridden_authority = receiver.is_none_or(|r| r.trimmed_text() == "self")
            && (methods.contains(&(class.clone(), name.into())) || shadowed(node, name));
        if (node_authority && !overridden_authority && receiver.is_none_or(|r| is_node(r, node)))
            || (api_authority
                && receiver.is_some_and(is_multiplayer)
                && !shadowed(node, "multiplayer"))
        {
            result.authority_uses.push(AuthorityUse {
                location: location.clone(),
                class,
                call: node.trimmed_text().into(),
            });
        } else if node_authority
            || (api_authority && receiver.is_some_and(|r| r.trimmed_text() != "OS"))
        {
            unknown(
                &mut result.unknowns,
                &path,
                node.line(),
                format!("unproven authority receiver: {}", node.trimmed_text()),
            );
        }
        if name == "set_multiplayer" && receiver.is_some_and(is_tree) {
            let root = if args.len() == 1 {
                Some(NodePath(String::new()))
            } else {
                args.get(1).copied().and_then(path_literal).map(NodePath)
            };
            if root.is_none() || args.is_empty() || args.len() > 2 {
                unknown(
                    &mut result.unknowns,
                    &path,
                    node.line(),
                    "set_multiplayer has a dynamic root or unsupported arguments",
                );
            }
            result.contexts.push(MultiplayerContext {
                location: location.clone(),
                api: args.first().map(|n| n.trimmed_text().into()),
                root,
            });
        }
        if name == "set_multiplayer" && !receiver.is_some_and(is_tree) {
            unknown(
                &mut result.unknowns,
                &path,
                node.line(),
                "unproven SceneTree receiver for set_multiplayer; context not inferred",
            );
        }
        if name == "rpc_config" || name == "set_script" {
            unknown(
                &mut result.unknowns,
                &path,
                node.line(),
                "runtime RPC/script configuration is not evaluated",
            );
        }
    }
    result.endpoints.sort_by(|a, b| {
        (&a.location, &a.class, &a.method).cmp(&(&b.location, &b.class, &b.method))
    });
    result.unknowns.sort();
    result.unknowns.dedup();
    result
}

fn endpoints(
    members: &[MemberDeclaration],
    class: Option<&str>,
    result: &mut ScriptObservations,
    methods: &mut BTreeSet<(Option<String>, String)>,
) {
    for member in members {
        if member.kind == declarations::MemberKind::Func {
            methods.insert((class.map(str::to_owned), member.name.clone()));
        }
        if let Some(error) = &member.rpc_error {
            unknown(
                &mut result.unknowns,
                &result.script,
                member.line,
                format!("{}: {error}", member.name),
            );
        }
        if let Some(config) = &member.rpc {
            result.endpoints.push(RpcEndpoint {
                script: result.script.clone(),
                class: class.map(str::to_owned),
                method: member.name.clone(),
                config: config.clone(),
                location: SourceLocation {
                    path: result.script.clone(),
                    line: member.line,
                },
            });
        }
    }
}

fn class_scope(node: Node<'_>, outer: Option<&str>) -> Option<String> {
    let mut names = vec![];
    let mut next = node.parent();
    while let Some(node) = next {
        if let Some(name) = ast::ClassDecl::cast(node).and_then(|c| c.name()) {
            names.push(name);
        }
        next = node.parent();
    }
    if names.is_empty() {
        return None;
    }
    if let Some(outer) = outer {
        names.push(outer);
    }
    names.reverse();
    Some(names.join("."))
}

fn enclosing_function(mut node: Node<'_>) -> Option<ast::FuncDecl<'_>> {
    while let Some(parent) = node.parent() {
        if let Some(func) = ast::FuncDecl::cast(parent) {
            return Some(func);
        }
        node = parent;
    }
    None
}

/// Conservative: even a later local declaration prevents treating a name as an
/// unshadowed method reference. No attempted control-flow or assignment analysis.
fn local_shadowed(node: Node<'_>, name: &str) -> bool {
    if let Some(func) = enclosing_function(node) {
        if func.parameters().any(|p| p.name == name) {
            return true;
        }
        if func.body().is_some_and(|body| {
            body.descendants()
                .any(|n| ast::binding_name(n) == Some(name))
        }) {
            return true;
        }
    }
    // Lambdas can also occur in class property initializers, outside a FuncDecl.
    let mut next = node.parent();
    while let Some(n) = next {
        if n.kind() == K::LambdaExpr && n.descendants().any(|n| ast::binding_name(n) == Some(name))
        {
            return true;
        }
        next = n.parent();
    }
    false
}

fn blocks_global_receiver(node: Node<'_>, name: &str) -> bool {
    if local_shadowed(node, name) {
        return true;
    }
    let mut next = node.parent();
    while let Some(n) = next {
        if matches!(n.kind(), K::SourceFile | K::ClassBody) {
            return n.children().filter_map(ast::Member::cast).any(|m| {
                if m.name() != Some(name) {
                    return false;
                }
                match m {
                    ast::Member::Var(var) => {
                        !(m.annotations().any(|a| a.name() == "onready")
                            && var.initializer().and_then(node_path).is_some())
                    }
                    ast::Member::Const(_) => true,
                    _ => false,
                }
            });
        }
        next = n.parent();
    }
    false
}

fn shadowed(node: Node<'_>, name: &str) -> bool {
    if local_shadowed(node, name) {
        return true;
    }
    let mut next = node.parent();
    while let Some(n) = next {
        if matches!(n.kind(), K::SourceFile | K::ClassBody) {
            return n.children().filter_map(ast::Member::cast).any(|m| {
                matches!(m, ast::Member::Var(_) | ast::Member::Const(_)) && m.name() == Some(name)
            });
        }
        next = n.parent();
    }
    false
}

fn is_node(receiver: Node<'_>, at: Node<'_>) -> bool {
    if ast::GetNode::cast(receiver).is_some()
        || receiver.trimmed_text() == "self"
        || node_path(receiver).is_some()
    {
        return true;
    }
    let Some(name) = ast::NameRef::cast(receiver).map(|n| n.text()) else {
        return false;
    };
    enclosing_function(at).is_some_and(|f| {
        f.parameters()
            .any(|p| p.name == name && p.type_text == Some("Node"))
    })
}

fn is_multiplayer(receiver: Node<'_>) -> bool {
    if receiver.trimmed_text() == "multiplayer" {
        return true;
    }
    ast::FieldExpr::cast(receiver).is_some_and(|f| {
        f.name() == Some("multiplayer") && f.receiver().is_some_and(|r| r.trimmed_text() == "self")
    })
}

fn is_self_call(call: ast::CallExpr<'_>, name: &str) -> bool {
    call.callee().is_some_and(|callee| {
        ast::NameRef::cast(callee).is_some_and(|n| n.text() == name)
            || ast::FieldExpr::cast(callee).is_some_and(|f| {
                f.name() == Some(name) && f.receiver().is_some_and(|r| r.trimmed_text() == "self")
            })
    })
}

fn is_tree(receiver: Node<'_>) -> bool {
    ast::CallExpr::cast(receiver)
        .is_some_and(|c| is_self_call(c, "get_tree") && c.arguments().is_empty())
}

fn callable_target(
    receiver: Node<'_>,
    at: Node<'_>,
    class: &Option<String>,
    methods: &BTreeSet<(Option<String>, String)>,
) -> Option<(Option<String>, Option<String>)> {
    if let Some(call) = ast::CallExpr::cast(receiver)
        && call.callee_text() == "Callable"
    {
        let args = call.arguments();
        if args.len() == 2 {
            return Some((
                Some(args[0].trimmed_text().into()),
                ast::string_or_name_literal(args[1]),
            ));
        }
    }
    if let Some(name) = ast::NameRef::cast(receiver).map(|n| n.text())
        && methods.contains(&(class.clone(), name.into()))
        && !shadowed(at, name)
    {
        return Some((None, Some(name.into())));
    }
    if let Some(field) = ast::FieldExpr::cast(receiver) {
        let object = field.receiver()?;
        let name = field.name()?;
        if (object.trimmed_text() == "self" && methods.contains(&(class.clone(), name.into())))
            || (object.trimmed_text() != "self" && is_node(object, at))
        {
            return Some((Some(object.trimmed_text().into()), Some(name.into())));
        }
    }
    None
}

pub(super) fn path_literal(node: Node<'_>) -> Option<String> {
    ast::string_or_name_literal(node)
        .or_else(|| ast::node_path_literal(node))
        .or_else(|| {
            let call = ast::CallExpr::cast(node)?;
            let args = call.arguments();
            (call.callee_text() == "NodePath" && args.len() == 1)
                .then(|| ast::string_literal(args[0]))?
        })
}

fn node_path(node: Node<'_>) -> Option<String> {
    ast::GetNode::cast(node).map(|n| n.path()).or_else(|| {
        let call = ast::CallExpr::cast(node)?;
        let args = call.arguments();
        ((is_self_call(call, "get_node") || is_self_call(call, "get_node_or_null"))
            && args.len() == 1)
            .then(|| path_literal(args[0]))?
    })
}

/// Decode a retained receiver expression via the same AST, never string splits.
pub(super) fn receiver_path(expression: &str) -> Option<String> {
    let parsed = syntax::parse(&format!("var __receiver = {expression}\n"));
    if !parsed.is_valid() {
        return None;
    }
    let file = ast::SourceFile::cast(parsed.root())?;
    let ast::Member::Var(var) = file.members().next()? else {
        return None;
    };
    node_path(var.initializer()?)
}

pub(super) fn receiver_name(expression: &str) -> Option<String> {
    let parsed = syntax::parse(&format!("var __receiver = {expression}\n"));
    if !parsed.is_valid() {
        return None;
    }
    let file = ast::SourceFile::cast(parsed.root())?;
    let ast::Member::Var(var) = file.members().next()? else {
        return None;
    };
    let node = var.initializer()?;
    if let Some(name) = ast::NameRef::cast(node) {
        return Some(name.text().into());
    }
    let field = ast::FieldExpr::cast(node)?;
    (field.receiver()?.trimmed_text() == "self").then(|| field.name().map(str::to_owned))?
}

/// Once a receiver resolves to a scene/autoload node, disambiguate its Node RPC
/// argument positions. Still leave nonliteral method names unresolved.
pub(super) fn node_call_method(expression: &str) -> Option<(CallForm, Option<String>)> {
    let parsed = syntax::parse(&format!("var __call = {expression}\n"));
    if !parsed.is_valid() {
        return None;
    }
    let file = ast::SourceFile::cast(parsed.root())?;
    let ast::Member::Var(var) = file.members().next()? else {
        return None;
    };
    let call = ast::CallExpr::cast(var.initializer()?)?;
    let name = ast::FieldExpr::cast(call.callee()?)?.name()?;
    let index = usize::from(name == "rpc_id");
    Some((
        if index == 1 {
            CallForm::RpcId
        } else {
            CallForm::Rpc
        },
        call.arguments()
            .get(index)
            .copied()
            .and_then(ast::string_or_name_literal),
    ))
}

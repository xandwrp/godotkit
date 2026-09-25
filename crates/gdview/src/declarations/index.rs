//! Syntax tree → declarations. Only `syntax::ast` views are used here.

use super::{
    AnnotationDeclaration, IndexedScript, InnerClassDeclaration, MemberDeclaration, MemberKind,
    Named, NodePathUse, ParameterDeclaration, ResourceUse, ResourceUseKind, RpcConfig,
    ScriptDeclaration,
};
use crate::respath::ResPath;
use crate::syntax::ast::{self, ExtendsDecl, Member, Parameter, SourceFile};
use crate::syntax::{self, Node, Parsed};

pub(super) fn script(path: ResPath, source: &str) -> IndexedScript {
    let parsed = syntax::parse(source);
    let root = parsed.root();
    let file = SourceFile::cast(root).expect("parse always yields a SourceFile root");
    let class_name = file.class_name().and_then(|decl| {
        Some(Named {
            name: decl.name()?.to_owned(),
            line: decl.node().line(),
        })
    });
    let qualifier = class_name.as_ref().map(|named| named.name.clone());
    let (members, inner_classes) = class_members(file.members(), qualifier.as_deref());
    let declaration = ScriptDeclaration {
        path,
        class_name,
        extends: file.extends().map(extends),
        is_tool: file
            .script_annotations()
            .any(|annotation| annotation.name() == "tool"),
        members,
        inner_classes,
    };
    let parse_error = parsed.diagnostics().first().map(|diagnostic| {
        format!(
            "line {}: {}",
            parsed.line_col(diagnostic.range.start).0,
            diagnostic.message
        )
    });
    IndexedScript {
        resource_uses: resource_uses(&parsed, file),
        node_path_uses: node_path_uses(file),
        declaration,
        parse_error,
    }
}

fn extends(decl: ExtendsDecl<'_>) -> Named {
    Named {
        name: decl.base_text().to_owned(),
        line: decl.node().line(),
    }
}

fn class_members<'a>(
    items: impl Iterator<Item = Member<'a>>,
    qualifier: Option<&str>,
) -> (Vec<MemberDeclaration>, Vec<InnerClassDeclaration>) {
    let mut members = Vec::new();
    let mut classes = Vec::new();
    for item in items {
        let Some(name) = item.name() else {
            // Anonymous enums still declare their values, but not a member name.
            continue;
        };
        if let Member::Class(class) = item {
            let qualified_name = match qualifier {
                Some(outer) => format!("{outer}.{name}"),
                None => name.to_owned(),
            };
            let (inner_members, inner_classes) =
                class_members(class.members(), Some(&qualified_name));
            classes.push(InnerClassDeclaration {
                line: class.node().line(),
                extends: class.extends().map(extends),
                qualified_name,
                members: inner_members,
                inner_classes,
            });
            continue;
        }
        let annotations: Vec<AnnotationDeclaration> = item
            .annotations()
            .map(|annotation| AnnotationDeclaration {
                name: annotation.name().to_owned(),
                arguments: annotation
                    .arguments()
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            })
            .collect();
        let rpc = annotations.iter().find(|a| a.name == "rpc").and_then(|a| {
            RpcConfig::from_arguments(&a.arguments.iter().map(String::as_str).collect::<Vec<_>>())
                .ok()
        });
        let (kind, is_static, type_text, parameters) = match item {
            Member::Signal(signal) => (
                MemberKind::Signal,
                false,
                None,
                parameter_list(signal.parameters()),
            ),
            Member::Const(constant) => (MemberKind::Const, false, constant.type_text(), Vec::new()),
            Member::Var(var) => (
                MemberKind::Var,
                var.is_static(),
                var.type_text(),
                Vec::new(),
            ),
            Member::Func(func) => (
                MemberKind::Func,
                func.is_static(),
                func.return_type_text(),
                parameter_list(func.parameters()),
            ),
            Member::Enum(_) => (MemberKind::Enum, false, None, Vec::new()),
            Member::Class(_) => unreachable!("handled above"),
        };
        members.push(MemberDeclaration {
            kind,
            name: name.to_owned(),
            line: item.node().line(),
            is_static,
            is_private: name.starts_with('_'),
            type_text: type_text.map(str::to_owned),
            annotations,
            rpc,
            parameters,
        });
    }
    (members, classes)
}

fn parameter_list<'a>(
    parameters: impl Iterator<Item = Parameter<'a>>,
) -> Vec<ParameterDeclaration> {
    parameters
        .map(|parameter| ParameterDeclaration {
            name: parameter.name.to_owned(),
            type_text: parameter.type_text.map(str::to_owned),
            default: parameter.default.map(|node| node.trimmed_text().to_owned()),
            is_variadic: parameter.is_variadic,
        })
        .collect()
}

fn resource_uses(parsed: &Parsed, file: SourceFile<'_>) -> Vec<ResourceUse> {
    let mut uses = Vec::new();
    if let Some(decl) = file.extends()
        && let Some(path) = decl.base_path()
    {
        uses.push(ResourceUse {
            kind: ResourceUseKind::Extends,
            path,
            line: decl.node().line(),
        });
    }
    for node in parsed.root().descendants() {
        let (kind, argument) = if let Some(preload) = ast::Preload::cast(node) {
            (ResourceUseKind::Preload, preload.argument())
        } else if let Some(call) = ast::CallExpr::cast(node)
            && matches!(call.callee_text(), "load" | "ResourceLoader.load")
        {
            (ResourceUseKind::Load, call.arguments().first().copied())
        } else {
            continue;
        };
        if let Some(argument) = argument
            && let Some(path) = ast::string_literal(argument)
        {
            uses.push(ResourceUse {
                kind,
                path,
                line: argument.line(),
            });
        }
    }
    uses
}

fn node_path_uses(file: SourceFile<'_>) -> Vec<NodePathUse> {
    let onready: Vec<Node<'_>> = file
        .members()
        .filter_map(|member| match member {
            Member::Var(var) if member.annotations().any(|a| a.name() == "onready") => {
                var.initializer()
            }
            _ => None,
        })
        .collect();
    let in_onready = |node: Node<'_>| {
        let range = node.trimmed_range();
        onready.iter().any(|init| {
            let outer = init.trimmed_range();
            outer.start <= range.start && range.end <= outer.end
        })
    };
    let mut uses = Vec::new();
    for node in file.node().descendants() {
        let path = if let Some(get_node) = ast::GetNode::cast(node) {
            get_node.path()
        } else if let Some(call) = ast::CallExpr::cast(node)
            && matches!(call.callee_text(), "get_node" | "self.get_node")
            && let Some(path) = call
                .arguments()
                .first()
                .and_then(|arg| ast::string_literal(*arg))
        {
            path
        } else {
            continue;
        };
        uses.push(NodePathUse {
            path,
            line: node.line(),
            onready: in_onready(node),
        });
    }
    uses
}

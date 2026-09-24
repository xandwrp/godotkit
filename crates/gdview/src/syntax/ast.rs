//! Typed views over [`super::Node`]. Each `cast` returns `Some` only for the
//! matching kind. These are the only syntax accessors `declarations` and `net` use.

use super::{Node, SyntaxKind};

macro_rules! view {
    ($name:ident, $kind:ident) => {
        #[derive(Clone, Copy)]
        pub struct $name<'a>(pub Node<'a>);
        impl<'a> $name<'a> {
            pub fn cast(node: Node<'a>) -> Option<Self> {
                (node.kind() == SyntaxKind::$kind).then_some(Self(node))
            }
            pub fn node(&self) -> Node<'a> {
                self.0
            }
        }
    };
}

view!(SourceFile, SourceFile);
view!(ClassBody, ClassBody);
view!(ClassNameDecl, ClassNameDecl);
view!(ExtendsDecl, ExtendsDecl);
view!(SignalDecl, SignalDecl);
view!(ConstDecl, ConstDecl);
view!(VarDecl, VarDecl);
view!(FuncDecl, FuncDecl);
view!(EnumDecl, EnumDecl);
view!(ClassDecl, ClassDecl);
view!(Annotation, Annotation);
view!(CallExpr, CallExpr);

impl<'a> SourceFile<'a> {
    pub fn class_name(&self) -> Option<ClassNameDecl<'a>> {
        todo!()
    }
    pub fn extends(&self) -> Option<ExtendsDecl<'a>> {
        todo!()
    }
    /// Top-level members in source order.
    pub fn members(&self) -> impl Iterator<Item = Member<'a>> + 'a {
        std::iter::empty()
    }
}

impl<'a> ClassDecl<'a> {
    pub fn name(&self) -> Option<&'a str> {
        todo!()
    }
    pub fn extends(&self) -> Option<ExtendsDecl<'a>> {
        todo!()
    }
    pub fn members(&self) -> impl Iterator<Item = Member<'a>> + 'a {
        std::iter::empty()
    }
}

/// One declaration in a class body.
#[derive(Clone, Copy)]
pub enum Member<'a> {
    Signal(SignalDecl<'a>),
    Const(ConstDecl<'a>),
    Var(VarDecl<'a>),
    Func(FuncDecl<'a>),
    Enum(EnumDecl<'a>),
    Class(ClassDecl<'a>),
}

impl<'a> Member<'a> {
    pub fn node(&self) -> Node<'a> {
        match self {
            Member::Signal(m) => m.0,
            Member::Const(m) => m.0,
            Member::Var(m) => m.0,
            Member::Func(m) => m.0,
            Member::Enum(m) => m.0,
            Member::Class(m) => m.0,
        }
    }
    pub fn name(&self) -> Option<&'a str> {
        todo!()
    }
    /// Annotations immediately preceding this member (`@export`, `@onready`, `@rpc(...)`).
    pub fn annotations(&self) -> impl Iterator<Item = Annotation<'a>> + 'a {
        std::iter::empty()
    }
}

impl<'a> ClassNameDecl<'a> {
    pub fn name(&self) -> Option<&'a str> {
        todo!()
    }
}

impl<'a> ExtendsDecl<'a> {
    /// `Node`, `"res://base.gd"`, or `Outer.Inner` as written.
    pub fn base_text(&self) -> &'a str {
        todo!()
    }
}

impl<'a> VarDecl<'a> {
    pub fn is_static(&self) -> bool {
        todo!()
    }
    pub fn type_text(&self) -> Option<&'a str> {
        todo!()
    }
    pub fn initializer(&self) -> Option<Node<'a>> {
        todo!()
    }
}

impl<'a> FuncDecl<'a> {
    pub fn is_static(&self) -> bool {
        todo!()
    }
    pub fn parameters(&self) -> impl Iterator<Item = Parameter<'a>> + 'a {
        std::iter::empty()
    }
    pub fn return_type_text(&self) -> Option<&'a str> {
        todo!()
    }
    pub fn body(&self) -> Option<Node<'a>> {
        todo!()
    }
}

#[derive(Clone, Copy)]
pub struct Parameter<'a> {
    pub name: &'a str,
    pub type_text: Option<&'a str>,
    pub default: Option<Node<'a>>,
}

impl<'a> Annotation<'a> {
    /// Name without the `@`.
    pub fn name(&self) -> &'a str {
        todo!()
    }
    /// Argument source texts, e.g. `["any_peer", "call_local"]`.
    pub fn arguments(&self) -> Vec<&'a str> {
        todo!()
    }
}

impl<'a> CallExpr<'a> {
    /// Callee source text, e.g. `self.rpc`, `multiplayer.get_unique_id`, `rpc_id`.
    pub fn callee_text(&self) -> &'a str {
        todo!()
    }
    pub fn arguments(&self) -> Vec<Node<'a>> {
        todo!()
    }
}

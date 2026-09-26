//! What `gdkit api` answers, shaped for JSON and ready to print: signatures
//! are rendered with [`ApiIndex::signature`], descriptions with
//! [`bbcode::doc_text`]. Every order is the index's, so answers are stable.
//!
//! # Tests (tests/api.rs; inline: `first_sentence_ends_before_a_capital_only`)
//! - `answers_classes_members_globals_and_misses`
//! - `search_answers_carry_signatures_and_briefs`

use serde::Serialize;

use super::bbcode::doc_text;
use super::{
    ApiArgument, ApiClass, ApiConstant, ApiEnum, ApiIndex, ApiMethod, ApiProperty, ApiSignal,
    Global, Member, SearchHit,
};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Answer {
    Class(ClassAnswer),
    Member(MemberAnswer),
    Search(SearchAnswer),
    Miss(MissAnswer),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClassAnswer {
    pub name: String,
    /// `core`, `editor`, `builtin`, `script`, …
    pub api_type: String,
    /// Ancestors, nearest first.
    pub inherits: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<super::ScriptOrigin>,
    /// The global instance name when the engine exposes one (`Input`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub singleton: Option<String>,
    pub instantiable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<String>,
    pub brief: Option<String>,
    pub description: Option<String>,
    pub see_also: Vec<String>,
    /// Declared on this class only; ask for a member to reach inherited ones.
    pub constructors: Vec<MemberLine>,
    pub methods: Vec<MemberLine>,
    pub properties: Vec<MemberLine>,
    pub signals: Vec<MemberLine>,
    pub constants: Vec<MemberLine>,
    pub enums: Vec<MemberLine>,
}

/// One member in a class listing or search result.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MemberLine {
    pub name: String,
    pub signature: String,
    /// First sentence of the description.
    pub brief: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub deprecated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MemberAnswer {
    /// `method`, `property`, `signal`, `constant`, `enum`, `enum_value`,
    /// `utility_function`, `gdscript_function`, `annotation`, `global_enum`,
    /// `global_enum_value`, `global_constant`, `gdscript_constant`.
    pub member_kind: &'static str,
    pub name: String,
    /// The class that declares it; `None` for globals.
    pub declaring_class: Option<String>,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<ScriptLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<String>,
    pub description: Option<String>,
    pub see_also: Vec<String>,
    /// Methods and functions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<ArgumentAnswer>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
    /// Properties and typed constants.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Enums.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<MemberLine>>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScriptLocation {
    pub path: String,
    pub line: Option<usize>,
    pub from_engine: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArgumentAnswer {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    /// As the engine spells it.
    pub default: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchAnswer {
    pub term: String,
    pub results: Vec<SearchResult>,
    /// More matched than `limit`.
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchResult {
    /// `class`, or a [`MemberAnswer::member_kind`].
    pub kind: &'static str,
    pub name: String,
    pub class: Option<String>,
    pub signature: Option<String>,
    pub brief: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MissAnswer {
    /// `class`, `member`, or `name` (a class or global).
    pub missing: &'static str,
    pub query: String,
    pub class: Option<String>,
    pub suggestions: Vec<String>,
}

const SUGGESTIONS: usize = 5;

/// `gdkit api <name>`: a class, else a global.
pub fn lookup(index: &ApiIndex, name: &str) -> Answer {
    if let Some(class) = index.class(name) {
        return Answer::Class(class_answer(index, class));
    }
    if let Some(global) = index.global(name) {
        return Answer::Member(global_answer(index, global));
    }
    Answer::Miss(MissAnswer {
        missing: "name",
        query: name.to_owned(),
        class: None,
        suggestions: index.suggest(None, name, SUGGESTIONS),
    })
}

/// `gdkit api <class> <member>`, searching the class's ancestors too.
pub fn lookup_member(index: &ApiIndex, class: &str, member: &str) -> Answer {
    if index.class(class).is_none() {
        return Answer::Miss(MissAnswer {
            missing: "class",
            query: class.to_owned(),
            class: None,
            suggestions: index.suggest(None, class, SUGGESTIONS),
        });
    }
    match index.lookup_member(class, member) {
        Some(hit) => Answer::Member(member_answer(index, hit.declaring_class, hit.member)),
        None => Answer::Miss(MissAnswer {
            missing: "member",
            query: member.to_owned(),
            class: Some(class.to_owned()),
            suggestions: index.suggest(Some(class), member, SUGGESTIONS),
        }),
    }
}

/// `gdkit api search <term>`.
pub fn search(index: &ApiIndex, term: &str, limit: usize) -> Answer {
    let mut hits = index.search(term, limit.saturating_add(1));
    let truncated = hits.len() > limit;
    hits.truncate(limit);
    let results = hits
        .into_iter()
        .map(|hit| match hit {
            SearchHit::Class(class) => SearchResult {
                kind: "class",
                name: class.name.clone(),
                class: None,
                signature: None,
                brief: class
                    .brief
                    .as_deref()
                    .map(|text| first_sentence(text, &class.name)),
            },
            SearchHit::Member { class, member } => {
                let answer = member_answer(index, class, member);
                SearchResult {
                    kind: answer.member_kind,
                    name: answer.name,
                    class: Some(class.name.clone()),
                    signature: Some(answer.signature),
                    brief: member
                        .description()
                        .map(|text| first_sentence(text, &class.name)),
                }
            }
            SearchHit::Global(global) => {
                let answer = global_answer(index, global);
                SearchResult {
                    kind: answer.member_kind,
                    name: answer.name,
                    class: None,
                    signature: Some(answer.signature),
                    brief: answer
                        .description
                        .as_deref()
                        .map(|text| first_line(text).to_owned()),
                }
            }
        })
        .collect();
    Answer::Search(SearchAnswer {
        term: term.to_owned(),
        results,
        truncated,
    })
}

fn class_answer(index: &ApiIndex, class: &ApiClass) -> ClassAnswer {
    let owner = class.name.as_str();
    let rendered = class
        .description
        .as_deref()
        .map(|text| doc_text(text, Some(owner)));
    let line = |name: &str,
                signature: String,
                description: Option<&str>,
                line: Option<usize>,
                deprecated: bool| MemberLine {
        name: name.to_owned(),
        signature,
        brief: description.map(|text| first_sentence(text, owner)),
        line,
        deprecated,
    };
    ClassAnswer {
        name: class.name.clone(),
        api_type: class.api_type.clone(),
        inherits: index
            .lineage(&class.name)
            .iter()
            .skip(1)
            .map(|ancestor| ancestor.name.clone())
            .chain(missing_ancestor(index, class))
            .collect(),
        script: class.script.clone(),
        singleton: index.singleton_for(&class.name).map(|s| s.name.clone()),
        instantiable: class.instantiable,
        deprecated: class.deprecated.clone(),
        experimental: class.experimental.clone(),
        brief: class
            .brief
            .as_deref()
            .map(|text| doc_text(text, Some(owner)).text),
        description: rendered.as_ref().map(|doc| doc.text.clone()),
        see_also: rendered.map(|doc| doc.see_also).unwrap_or_default(),
        constructors: class
            .constructors
            .iter()
            .map(|c| {
                line(
                    &c.name,
                    index.signature(None, c),
                    c.description.as_deref(),
                    c.line,
                    false,
                )
            })
            .collect(),
        methods: class
            .methods
            .iter()
            .map(|m| {
                line(
                    &m.name,
                    index.signature(None, m),
                    m.description.as_deref(),
                    m.line,
                    m.deprecated.is_some(),
                )
            })
            .collect(),
        properties: class
            .properties
            .iter()
            .map(|p| {
                line(
                    &p.name,
                    property_signature(index, p),
                    p.description.as_deref(),
                    p.line,
                    p.deprecated.is_some(),
                )
            })
            .collect(),
        signals: class
            .signals
            .iter()
            .map(|s| {
                line(
                    &s.name,
                    signal_signature(index, s),
                    s.description.as_deref(),
                    s.line,
                    s.deprecated.is_some(),
                )
            })
            .collect(),
        constants: class
            .constants
            .iter()
            .map(|c| {
                line(
                    &c.name,
                    constant_signature(c),
                    c.description.as_deref(),
                    c.line,
                    c.deprecated.is_some(),
                )
            })
            .collect(),
        enums: class
            .enums
            .iter()
            .map(|e| line(&e.name, enum_signature(e), None, e.line, false))
            .collect(),
    }
}

/// A parent the index does not know (a script's unresolved base) still names the chain's end.
fn missing_ancestor(index: &ApiIndex, class: &ApiClass) -> Option<String> {
    let last = index.lineage(&class.name).pop()?;
    last.parent
        .as_ref()
        .filter(|parent| index.class(parent).is_none())
        .cloned()
}

fn member_answer(index: &ApiIndex, class: &ApiClass, member: Member<'_>) -> MemberAnswer {
    let owner = class.name.as_str();
    let mut answer = blank(member_kind(member), member.name());
    answer.declaring_class = Some(class.name.clone());
    answer.script = class.script.as_ref().map(|origin| ScriptLocation {
        path: origin.path.clone(),
        line: member_line(member).or(origin.line),
        from_engine: origin.from_engine,
    });
    describe(&mut answer, member.description(), Some(owner));
    match member {
        Member::Method(method) => fill_method(index, &mut answer, Some(owner), method),
        Member::Property(property) => {
            answer.signature = format!("{owner}.{}", property_signature(index, property));
            answer.type_ = Some(property.type_.display());
            answer.default = property.default.clone();
            lifecycle(&mut answer, &property.deprecated, &property.experimental);
        }
        Member::Signal(signal) => {
            answer.signature = format!("{owner}.{}", signal_signature(index, signal));
            answer.arguments = Some(arguments(&signal.arguments));
            lifecycle(&mut answer, &signal.deprecated, &signal.experimental);
        }
        Member::Constant(constant) => fill_constant(&mut answer, Some(owner), constant),
        Member::Enum(enum_) => fill_enum(&mut answer, Some(owner), enum_),
        Member::EnumValue {
            owner: enum_,
            value,
        } => {
            answer.signature = format!("{owner}.{} = {}", value.name, value.value);
            answer.type_ = Some(format!("{owner}.{}", enum_.name));
            answer.value = Some(value.value.to_string());
            lifecycle(&mut answer, &value.deprecated, &value.experimental);
        }
    }
    answer
}

fn global_answer(index: &ApiIndex, global: Global<'_>) -> MemberAnswer {
    let (kind, name) = match global {
        Global::Utility(_) => ("utility_function", global.name()),
        Global::GdscriptFunction(_) => ("gdscript_function", global.name()),
        Global::Annotation(_) => ("annotation", global.name()),
        Global::Enum(_) => ("global_enum", global.name()),
        Global::EnumValue { .. } => ("global_enum_value", global.name()),
        Global::Constant(_) => ("global_constant", global.name()),
        Global::GdscriptConstant(_) => ("gdscript_constant", global.name()),
    };
    let mut answer = blank(kind, name);
    match global {
        Global::Utility(method) | Global::GdscriptFunction(method) | Global::Annotation(method) => {
            describe(&mut answer, method.description.as_deref(), None);
            fill_method(index, &mut answer, None, method);
        }
        Global::Enum(enum_) => fill_enum(&mut answer, None, enum_),
        Global::EnumValue { owner, value } => {
            describe(&mut answer, value.description.as_deref(), None);
            answer.signature = format!("{} = {}", value.name, value.value);
            answer.type_ = Some(owner.name.clone());
            answer.value = Some(value.value.to_string());
        }
        Global::Constant(constant) | Global::GdscriptConstant(constant) => {
            describe(&mut answer, constant.description.as_deref(), None);
            fill_constant(&mut answer, None, constant);
        }
    }
    answer
}

fn blank(member_kind: &'static str, name: &str) -> MemberAnswer {
    MemberAnswer {
        member_kind,
        name: name.to_owned(),
        declaring_class: None,
        signature: String::new(),
        script: None,
        deprecated: None,
        experimental: None,
        description: None,
        see_also: Vec::new(),
        arguments: None,
        return_type: None,
        type_: None,
        default: None,
        value: None,
        values: None,
    }
}

fn describe(answer: &mut MemberAnswer, description: Option<&str>, owner: Option<&str>) {
    if let Some(description) = description {
        let doc = doc_text(description, owner);
        answer.description = Some(doc.text);
        answer.see_also = doc.see_also;
    }
}

fn lifecycle(
    answer: &mut MemberAnswer,
    deprecated: &Option<String>,
    experimental: &Option<String>,
) {
    answer.deprecated = deprecated.clone();
    answer.experimental = experimental.clone();
}

fn fill_method(
    index: &ApiIndex,
    answer: &mut MemberAnswer,
    owner: Option<&str>,
    method: &ApiMethod,
) {
    answer.signature = index.signature(owner, method);
    answer.arguments = Some(arguments(&method.arguments));
    if !method.name.starts_with('@') {
        answer.return_type = Some(method.return_type.display());
    }
    lifecycle(answer, &method.deprecated, &method.experimental);
}

fn fill_constant(answer: &mut MemberAnswer, owner: Option<&str>, constant: &ApiConstant) {
    answer.signature = match owner {
        Some(owner) => format!("{owner}.{}", constant_signature(constant)),
        None => constant_signature(constant),
    };
    answer.type_ = constant.type_.as_ref().map(|type_| type_.display());
    answer.value = (!constant.value.is_empty()).then(|| constant.value.clone());
    lifecycle(answer, &constant.deprecated, &constant.experimental);
}

fn fill_enum(answer: &mut MemberAnswer, owner: Option<&str>, enum_: &ApiEnum) {
    answer.signature = match owner {
        Some(owner) => format!("{owner}.{}", enum_signature(enum_)),
        None => enum_signature(enum_),
    };
    answer.values = Some(
        enum_
            .values
            .iter()
            .map(|value| MemberLine {
                name: value.name.clone(),
                signature: format!("{} = {}", value.name, value.value),
                brief: value
                    .description
                    .as_deref()
                    .map(|text| first_sentence(text, owner.unwrap_or_default())),
                line: None,
                deprecated: value.deprecated.is_some(),
            })
            .collect(),
    );
}

fn arguments(arguments: &[ApiArgument]) -> Vec<ArgumentAnswer> {
    arguments
        .iter()
        .map(|argument| ArgumentAnswer {
            name: argument.name.clone(),
            type_: argument.type_.display(),
            default: argument.default.clone(),
        })
        .collect()
}

fn member_kind(member: Member<'_>) -> &'static str {
    match member {
        Member::Method(_) => "method",
        Member::Property(_) => "property",
        Member::Signal(_) => "signal",
        Member::Enum(_) => "enum",
        Member::EnumValue { .. } => "enum_value",
        Member::Constant(_) => "constant",
    }
}

fn member_line(member: Member<'_>) -> Option<usize> {
    match member {
        Member::Method(method) => method.line,
        Member::Property(property) => property.line,
        Member::Signal(signal) => signal.line,
        Member::Enum(enum_) | Member::EnumValue { owner: enum_, .. } => enum_.line,
        Member::Constant(constant) => constant.line,
    }
}

/// `velocity: Vector3 = Vector3(0, 0, 0)`
pub fn property_signature(index: &ApiIndex, property: &ApiProperty) -> String {
    let mut text = format!("{}: {}", property.name, property.type_.display());
    if let Some(default) = &property.default {
        text.push_str(" = ");
        text.push_str(&index.default_display(&property.type_, default));
    }
    text
}

/// `signal body_entered(body: Node)`
pub fn signal_signature(index: &ApiIndex, signal: &ApiSignal) -> String {
    let parameters: Vec<String> = signal
        .arguments
        .iter()
        .map(|argument| index.parameter(argument))
        .collect();
    format!("signal {}({})", signal.name, parameters.join(", "))
}

/// `ZERO: Vector3 = Vector3(0, 0, 0)`, `NOTIFICATION_READY = 13`, or just the name when the value is unknown.
pub fn constant_signature(constant: &ApiConstant) -> String {
    let mut text = constant.name.clone();
    if let Some(type_) = &constant.type_ {
        text.push_str(": ");
        text.push_str(&type_.display());
    }
    if !constant.value.is_empty() {
        text.push_str(" = ");
        text.push_str(&constant.value);
    }
    text
}

/// `enum ProcessMode { PROCESS_MODE_INHERIT = 0, … }`; `flags` for bitfields.
pub fn enum_signature(enum_: &ApiEnum) -> String {
    let keyword = if enum_.is_bitfield { "flags" } else { "enum" };
    let values: Vec<String> = enum_
        .values
        .iter()
        .map(|value| format!("{} = {}", value.name, value.value))
        .collect();
    if values.is_empty() {
        format!("{keyword} {}", enum_.name)
    } else {
        format!("{keyword} {} {{ {} }}", enum_.name, values.join(", "))
    }
}

/// The rendered description up to its first sentence end or line break. A
/// sentence ends at `. ` before a capital, so `e.g. walk` does not end one.
fn first_sentence(bbcode: &str, owner: &str) -> String {
    let owner = (!owner.is_empty()).then_some(owner);
    let text = doc_text(bbcode, owner).text;
    let line = first_line(&text);
    let end = line.match_indices(". ").map(|(at, _)| at).find(|&at| {
        line[at + 2..]
            .chars()
            .next()
            .is_some_and(char::is_uppercase)
    });
    match end {
        Some(end) => line[..=end].to_owned(),
        None => line.to_owned(),
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn first_sentence_ends_before_a_capital_only() {
        let brief = |text: &str| super::first_sentence(text, "");
        assert_eq!(
            brief("Loop speed (e.g. walk). More."),
            "Loop speed (e.g. walk)."
        );
        assert_eq!(brief("One. two. Three."), "One. two.");
        assert_eq!(brief("No end"), "No end");
        assert_eq!(brief("Line one\nLine two"), "Line one");
    }
}

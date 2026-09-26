//! Godot class-reference XML (`--doctool`, `--doctool --gdscript-docs`) → [`ApiClass`].
//!
//! Text is dedented the way the XML indents it. Constants carrying `enum=` are
//! grouped into enums in order of first appearance. Theme items and tutorials
//! are skipped.
//!
//! # Tests (inline)
//! - `parses_methods_members_signals_constants_and_enums`
//! - `annotations_and_qualifiers_parse`
//! - `malformed_xml_is_an_error`

use std::collections::BTreeMap;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::{
    ApiArgument, ApiClass, ApiConstant, ApiEnum, ApiEnumValue, ApiMethod, ApiProperty, ApiSignal,
    ApiType,
};

/// One parsed `<class>` document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DocClass {
    pub class: ApiClass,
    /// `<annotations>`; only `@GDScript` has them.
    pub annotations: Vec<ApiMethod>,
}

/// Parses one class XML document.
pub fn parse_class(text: &str) -> crate::Result<DocClass> {
    let root = parse_tree(text)?;
    if root.name != "class" {
        return Err(error(format!("expected <class>, found <{}>", root.name)));
    }
    let mut class = ApiClass {
        name: root.attr("name").unwrap_or_default().to_owned(),
        parent: root.attr("inherits").map(str::to_owned),
        api_type: root.attr("api_type").unwrap_or_default().to_owned(),
        brief: root.child("brief_description").and_then(Element::body),
        description: root.child("description").and_then(Element::body),
        deprecated: root.lifecycle("deprecated"),
        experimental: root.lifecycle("experimental"),
        ..ApiClass::default()
    };
    class.constructors = root
        .items("constructors", "constructor")
        .map(method)
        .collect();
    class.methods = root.items("methods", "method").map(method).collect();
    class.properties = root
        .items("members", "member")
        .map(|member| ApiProperty {
            name: member.attr("name").unwrap_or_default().to_owned(),
            type_: doc_type(member),
            getter: member
                .attr("getter")
                .filter(|n| !n.is_empty())
                .map(str::to_owned),
            setter: member
                .attr("setter")
                .filter(|n| !n.is_empty())
                .map(str::to_owned),
            default: member.attr("default").map(str::to_owned),
            description: member.body(),
            deprecated: member.lifecycle("deprecated"),
            experimental: member.lifecycle("experimental"),
            line: None,
        })
        .collect();
    class.signals = root
        .items("signals", "signal")
        .map(|signal| ApiSignal {
            name: signal.attr("name").unwrap_or_default().to_owned(),
            arguments: arguments(signal),
            description: signal.child("description").and_then(Element::body),
            deprecated: signal.lifecycle("deprecated"),
            experimental: signal.lifecycle("experimental"),
            line: None,
        })
        .collect();
    for constant in root.items("constants", "constant") {
        let name = constant.attr("name").unwrap_or_default().to_owned();
        let value = constant.attr("value").unwrap_or_default().to_owned();
        let description = constant.body();
        match (constant.attr("enum"), value.parse::<i64>()) {
            (Some(enum_name), Ok(number)) => {
                let bitfield = constant.attr("is_bitfield") == Some("true");
                let position = class.enums.iter().position(|e| e.name == enum_name);
                let enum_ = match position {
                    Some(at) => &mut class.enums[at],
                    None => {
                        class.enums.push(ApiEnum {
                            name: enum_name.to_owned(),
                            is_bitfield: bitfield,
                            values: Vec::new(),
                            line: None,
                        });
                        class.enums.last_mut().expect("just pushed")
                    }
                };
                enum_.values.push(ApiEnumValue {
                    name,
                    value: number,
                    description,
                    deprecated: constant.lifecycle("deprecated"),
                    experimental: constant.lifecycle("experimental"),
                });
            }
            _ => class.constants.push(ApiConstant {
                name,
                type_: None,
                value,
                description,
                deprecated: constant.lifecycle("deprecated"),
                experimental: constant.lifecycle("experimental"),
                line: None,
            }),
        }
    }
    let annotations = root
        .items("annotations", "annotation")
        .map(method)
        .collect();
    Ok(DocClass { class, annotations })
}

fn method(element: &Element) -> ApiMethod {
    let qualifiers: Vec<&str> = element
        .attr("qualifiers")
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    let has = |word: &str| qualifiers.contains(&word);
    ApiMethod {
        name: element.attr("name").unwrap_or_default().to_owned(),
        is_static: has("static"),
        is_const: has("const"),
        is_virtual: has("virtual"),
        is_required: has("required"),
        is_vararg: has("vararg"),
        return_type: element.child("return").map_or(ApiType::Void, doc_type),
        arguments: arguments(element),
        description: element.child("description").and_then(Element::body),
        deprecated: element.lifecycle("deprecated"),
        experimental: element.lifecycle("experimental"),
        line: None,
    }
}

fn arguments(element: &Element) -> Vec<ApiArgument> {
    let mut params: Vec<(usize, &Element)> = element
        .children
        .iter()
        .filter(|child| child.name == "param")
        .enumerate()
        .map(|(order, param)| {
            let index = param.attr("index").and_then(|i| i.parse().ok());
            (index.unwrap_or(order), param)
        })
        .collect();
    params.sort_by_key(|(index, _)| *index);
    params
        .into_iter()
        .map(|(_, param)| ApiArgument {
            name: param.attr("name").unwrap_or_default().to_owned(),
            type_: doc_type(param),
            default: param.attr("default").map(str::to_owned),
        })
        .collect()
}

fn doc_type(element: &Element) -> ApiType {
    ApiType::parse_doc(
        element.attr("type").unwrap_or("Variant"),
        element.attr("enum"),
        element.attr("is_bitfield") == Some("true"),
    )
}

fn error(message: String) -> crate::Error {
    crate::Error::Parse {
        path: None,
        line: 0,
        message: format!("class reference XML: {message}"),
    }
}

#[derive(Debug, Default)]
struct Element {
    name: String,
    attributes: BTreeMap<String, String>,
    children: Vec<Element>,
    text: String,
}

impl Element {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }

    fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|child| child.name == name)
    }

    /// `<group><item/>…</group>` children named `item`.
    fn items<'a>(&'a self, group: &str, item: &'a str) -> impl Iterator<Item = &'a Element> {
        self.child(group)
            .into_iter()
            .flat_map(move |group| group.children.iter().filter(move |c| c.name == item))
    }

    /// `deprecated="…"` / `experimental="…"`: the message, empty when none was given.
    fn lifecycle(&self, name: &str) -> Option<String> {
        self.attr(name).map(|message| message.trim().to_owned())
    }

    /// Dedented text, or `None` when blank.
    fn body(&self) -> Option<String> {
        let text = dedent(&self.text);
        (!text.is_empty()).then_some(text)
    }
}

fn parse_tree(text: &str) -> crate::Result<Element> {
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Element> = Vec::new();
    loop {
        let event = reader
            .read_event()
            .map_err(|e| error(format!("at byte {}: {e}", reader.buffer_position())))?;
        match event {
            Event::Start(start) => stack.push(element(&start)?),
            Event::Empty(start) => {
                let element = element(&start)?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(element),
                    None => return Ok(element),
                }
            }
            Event::End(_) => {
                let done = stack
                    .pop()
                    .ok_or_else(|| error("unbalanced end tag".into()))?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => return Ok(done),
                }
            }
            Event::Text(text) => {
                if let Some(current) = stack.last_mut() {
                    let text = text.unescape().map_err(|e| error(e.to_string()))?;
                    current.text.push_str(&text);
                }
            }
            Event::CData(data) => {
                if let Some(current) = stack.last_mut() {
                    current
                        .text
                        .push_str(&String::from_utf8_lossy(&data.into_inner()));
                }
            }
            Event::Eof => return Err(error("no <class> element".into())),
            _ => {}
        }
    }
}

fn element(start: &BytesStart<'_>) -> crate::Result<Element> {
    let mut attributes = BTreeMap::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|e| error(e.to_string()))?;
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        let value = attribute
            .unescape_value()
            .map_err(|e| error(e.to_string()))?
            .into_owned();
        attributes.insert(key, value);
    }
    Ok(Element {
        name: String::from_utf8_lossy(start.name().as_ref()).into_owned(),
        attributes,
        ..Element::default()
    })
}

/// Drops blank edge lines and the indentation common to the rest.
fn dedent(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.iter().position(|line| !line.trim().is_empty());
    let last = lines.iter().rposition(|line| !line.trim().is_empty());
    let (Some(first), Some(last)) = (first, last) else {
        return String::new();
    };
    let lines = &lines[first..=last];
    let indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches(['\t', ' ']).len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|line| line.get(indent..).unwrap_or("").trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASS: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<class name="WeaponDefinition" inherits="Resource" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
	<brief_description>
		A weapon's static stats.
	</brief_description>
	<description>
		Longer description with [code]bbcode[/code].
		[codeblock]
		func f():
			pass
		[/codeblock]
	</description>
	<tutorials>
	</tutorials>
	<methods>
		<method name="fire" qualifiers="const">
			<return type="bool" />
			<param index="1" name="spread" type="float" default="0.5" />
			<param index="0" name="target" type="Node3D" />
			<description>
				Fires once.
			</description>
		</method>
	</methods>
	<members>
		<member name="damage" type="int" setter="" getter="" default="10">
			Damage per shot.
		</member>
		<member name="mode" type="int" setter="set_mode" getter="get_mode" enum="WeaponDefinition.Mode" default="0">
		</member>
		<member name="targets" type="Node3D[]" setter="" getter="" default="[]">
		</member>
	</members>
	<signals>
		<signal name="fired">
			<param index="0" name="count" type="int" />
			<description>
			</description>
		</signal>
	</signals>
	<constants>
		<constant name="SEMI" value="0" enum="Mode">
			One per pull.
		</constant>
		<constant name="MAX_AMMO" value="30">
		</constant>
		<constant name="AUTO" value="1" enum="Mode">
		</constant>
	</constants>
</class>
"#;

    #[test]
    fn parses_methods_members_signals_constants_and_enums() {
        let doc = parse_class(CLASS).unwrap();
        let class = &doc.class;
        assert_eq!(class.name, "WeaponDefinition");
        assert_eq!(class.parent.as_deref(), Some("Resource"));
        assert_eq!(class.brief.as_deref(), Some("A weapon's static stats."));
        assert_eq!(
            class.description.as_deref(),
            Some(
                "Longer description with [code]bbcode[/code].\n[codeblock]\nfunc f():\n\tpass\n[/codeblock]"
            )
        );
        let fire = &class.methods[0];
        assert!(fire.is_const && !fire.is_static);
        assert_eq!(fire.return_type, ApiType::parse("bool"));
        let names: Vec<_> = fire.arguments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["target", "spread"]);
        assert_eq!(fire.arguments[1].default.as_deref(), Some("0.5"));
        assert_eq!(fire.description.as_deref(), Some("Fires once."));

        assert_eq!(class.properties[0].default.as_deref(), Some("10"));
        assert_eq!(class.properties[0].getter, None);
        assert_eq!(
            class.properties[0].description.as_deref(),
            Some("Damage per shot.")
        );
        assert_eq!(class.properties[1].type_.display(), "WeaponDefinition.Mode");
        assert_eq!(class.properties[1].setter.as_deref(), Some("set_mode"));
        assert_eq!(class.properties[2].type_.display(), "Array[Node3D]");

        assert_eq!(class.signals[0].arguments[0].name, "count");
        assert_eq!(class.signals[0].description, None);
        assert_eq!(class.constants.len(), 1);
        assert_eq!(class.constants[0].name, "MAX_AMMO");
        assert_eq!(class.enums.len(), 1);
        let values: Vec<_> = class.enums[0]
            .values
            .iter()
            .map(|v| (v.name.as_str(), v.value))
            .collect();
        assert_eq!(values, [("SEMI", 0), ("AUTO", 1)]);
        assert_eq!(
            class.enums[0].values[0].description.as_deref(),
            Some("One per pull.")
        );
    }

    #[test]
    fn annotations_and_qualifiers_parse() {
        let doc = parse_class(
            r#"<class name="@GDScript">
	<methods>
		<method name="range" qualifiers="vararg">
			<return type="Array" />
		</method>
		<method name="preload">
			<return type="Resource" />
			<param index="0" name="path" type="String" />
		</method>
	</methods>
	<annotations>
		<annotation name="@export_range" qualifiers="vararg">
			<return type="void" />
			<param index="0" name="min" type="float" />
			<param index="1" name="extra_hints" type="String" default="&quot;&quot;" />
		</annotation>
	</annotations>
	<constants>
		<constant name="PI" value="3.14159265358979">
		</constant>
	</constants>
</class>"#,
        )
        .unwrap();
        assert!(doc.class.methods[0].is_vararg);
        assert_eq!(doc.class.methods[0].return_type.display(), "Array");
        assert_eq!(doc.annotations[0].name, "@export_range");
        assert_eq!(
            doc.annotations[0].arguments[1].default.as_deref(),
            Some("\"\"")
        );
        assert_eq!(doc.class.constants[0].value, "3.14159265358979");
    }

    #[test]
    fn malformed_xml_is_an_error() {
        assert!(parse_class("<class name=\"X\"><methods></class>").is_err());
        assert!(parse_class("<other/>").is_err());
        assert!(parse_class("").is_err());
    }
}

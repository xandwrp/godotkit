//! Godot's class-reference BBCode → plain text for terminals and agents.
//!
//! `[code]x[/code]` and `[kbd]` become `` `x` ``, code blocks become fenced
//! blocks (GDScript only; `[csharp]` is dropped), references such as
//! `[method move_and_slide]` or `[Node]` become `` `move_and_slide()` `` / `` `Node` ``
//! and are collected into `see_also`, qualified with the owning class when the
//! reference is not. Formatting tags are removed; unknown tags stay literal,
//! since examples print things like `[project_name]`.
//!
//! # Tests (inline)
//! - `code_and_references_render_and_references_are_collected`
//! - `codeblocks_keep_gdscript_only_and_code_is_literal`
//! - `formatting_is_stripped_links_keep_urls_and_unknown_tags_stay`

/// Rendered description.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocText {
    pub text: String,
    /// `Class.member` or `Class`, in order of first mention, without duplicates.
    pub see_also: Vec<String>,
}

/// Renders `bbcode`; `owner` qualifies unqualified member references.
pub fn doc_text(bbcode: &str, owner: Option<&str>) -> DocText {
    let mut text = String::new();
    let mut see_also: Vec<String> = Vec::new();
    let mut refer = |reference: String| {
        if !see_also.contains(&reference) {
            see_also.push(reference);
        }
    };
    let mut rest = bbcode;
    while let Some(open) = rest.find('[') {
        text.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else {
            text.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let tag = &after[..close];
        let tail = &after[close + 1..];
        if tag.is_empty() || tag.contains(['\n', '[']) {
            text.push('[');
            rest = after;
            continue;
        }
        let (name, argument) = match tag.find([' ', '=']) {
            Some(at) => (&tag[..at], tag[at + 1..].trim()),
            None => (tag, ""),
        };
        rest = tail;
        match name {
            "code" | "kbd" => {
                let (body, next) = until(tail, &format!("[/{name}]"));
                text.push('`');
                text.push_str(body);
                text.push('`');
                rest = next;
            }
            "codeblock" => {
                let (body, next) = until(tail, "[/codeblock]");
                let language = argument.strip_prefix("lang=").unwrap_or("gdscript");
                fence(&mut text, language, body, next);
                rest = next;
            }
            "codeblocks" => {
                let (body, next) = until(tail, "[/codeblocks]");
                if let Some((_, gdscript)) = body.split_once("[gdscript]") {
                    let (gdscript, _) = until(gdscript, "[/gdscript]");
                    fence(&mut text, "gdscript", gdscript, next);
                }
                rest = next;
            }
            "gdscript" => {
                let (body, next) = until(tail, "[/gdscript]");
                fence(&mut text, "gdscript", body, next);
                rest = next;
            }
            "csharp" => rest = until(tail, "[/csharp]").1,
            "img" => rest = until(tail, "[/img]").1,
            "url" => {
                let (label, next) = until(tail, "[/url]");
                text.push_str(label);
                if !argument.is_empty() && argument != label {
                    text.push_str(&format!(" ({argument})"));
                }
                rest = next;
            }
            "b" | "/b" | "i" | "/i" | "u" | "/u" | "s" | "/s" | "center" | "/center" | "color"
            | "/color" | "font" | "/font" | "font_size" | "/font_size" => {}
            "br" => text.push('\n'),
            "lb" => text.push('['),
            "rb" => text.push(']'),
            "param" => {
                text.push('`');
                text.push_str(argument);
                text.push('`');
            }
            "method" | "constructor" if !argument.is_empty() => {
                text.push_str(&format!("`{argument}()`"));
                refer(qualify(argument, owner));
            }
            "member" | "signal" | "constant" | "enum" | "theme_item" | "operator"
                if !argument.is_empty() =>
            {
                text.push_str(&format!("`{argument}`"));
                refer(qualify(argument, owner));
            }
            "annotation" if !argument.is_empty() => {
                text.push_str(&format!("`{argument}`"));
                refer(argument.to_owned());
            }
            _ if argument.is_empty() && is_class_name(name) => {
                text.push_str(&format!("`{name}`"));
                refer(name.to_owned());
            }
            _ => {
                text.push('[');
                text.push_str(tag);
                text.push(']');
            }
        }
    }
    text.push_str(rest);
    DocText {
        text: tidy(&text),
        see_also,
    }
}

/// `(body, after the closing tag)`; an unclosed tag runs to the end.
fn until<'a>(text: &'a str, closing: &str) -> (&'a str, &'a str) {
    match text.find(closing) {
        Some(at) => (&text[..at], &text[at + closing.len()..]),
        None => (text, ""),
    }
}

/// Appends a fenced block on its own lines; `next` is the text that follows it.
fn fence(text: &mut String, language: &str, body: &str, next: &str) {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("```");
    text.push_str(language);
    text.push('\n');
    text.push_str(body.trim_matches('\n'));
    text.push_str("\n```");
    if !next.starts_with('\n') {
        text.push('\n');
    }
}

fn qualify(reference: &str, owner: Option<&str>) -> String {
    match owner {
        Some(owner) if !reference.contains('.') => format!("{owner}.{reference}"),
        _ => reference.to_owned(),
    }
}

/// `[Node]`, `[@GlobalScope]`, and the lowercase builtin types.
fn is_class_name(name: &str) -> bool {
    let mut chars = name.chars();
    let starts = match chars.next() {
        Some(first) => first.is_ascii_uppercase() || first == '@',
        None => false,
    };
    (starts && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_'))
        || matches!(name, "int" | "float" | "bool")
}

/// Trims trailing spaces on each line and collapses runs of blank lines.
fn tidy(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.trim().lines() {
        let line = line.trim_end();
        if line.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.truncate(out.trim_end().len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_and_references_render_and_references_are_collected() {
        let doc = doc_text(
            "Moves by [member velocity]. See [method Node._physics_process], [method move_and_slide], [signal fired], [CharacterBody3D] and [int]. [code]delta[/code] and [param speed]. Again [member velocity].",
            Some("CharacterBody3D"),
        );
        assert_eq!(
            doc.text,
            "Moves by `velocity`. See `Node._physics_process()`, `move_and_slide()`, `fired`, `CharacterBody3D` and `int`. `delta` and `speed`. Again `velocity`."
        );
        assert_eq!(
            doc.see_also,
            [
                "CharacterBody3D.velocity",
                "Node._physics_process",
                "CharacterBody3D.move_and_slide",
                "CharacterBody3D.fired",
                "CharacterBody3D",
                "int",
            ]
        );
    }

    #[test]
    fn codeblocks_keep_gdscript_only_and_code_is_literal() {
        let doc = doc_text(
            "Example:\n[codeblocks]\n[gdscript]\nvar a = [1, 2][0]\n[/gdscript]\n[csharp]\nvar a = new int[] { 1 };\n[/csharp]\n[/codeblocks]\nIndex [code][0][/code].\n[codeblock lang=text]\n$ godot\n[/codeblock]",
            None,
        );
        assert_eq!(
            doc.text,
            "Example:\n```gdscript\nvar a = [1, 2][0]\n```\nIndex `[0]`.\n```text\n$ godot\n```"
        );
        assert!(doc.see_also.is_empty());
    }

    #[test]
    fn formatting_is_stripped_links_keep_urls_and_unknown_tags_stay() {
        let doc = doc_text(
            "[b]Note:[/b] see [url=https://easings.net/]easings.net[/url] and [url]https://x.y[/url]. Prints [project_name] [lb]x[rb][br]next",
            None,
        );
        assert_eq!(
            doc.text,
            "Note: see easings.net (https://easings.net/) and https://x.y. Prints [project_name] [x]\nnext"
        );
    }
}

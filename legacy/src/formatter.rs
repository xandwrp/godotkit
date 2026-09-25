use crate::syntax::{Element, Node, Span, SyntaxError, SyntaxKind as K, parse, tokenize};

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub line_width: usize,
    pub tab_width: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            line_width: 100,
            tab_width: 4,
        }
    }
}

fn line_start(source: &str, offset: usize) -> usize {
    source[..offset].rfind(['\r', '\n']).map_or(0, |at| at + 1)
}

fn line_end(source: &str, offset: usize) -> usize {
    source[offset..]
        .find('\n')
        .map_or(source.len(), |at| offset + at + 1)
}

fn disabled_ranges(source: &str) -> Vec<Span> {
    let (tokens, _) = tokenize(source);
    let mut ranges = Vec::new();
    let mut start = None;
    for token in tokens {
        if !matches!(
            token.kind,
            K::LineComment | K::DocComment | K::RegionComment | K::EndRegionComment
        ) {
            continue;
        }
        match source[token.range].trim() {
            "# gdkit: off" if start.is_none() => {
                start = Some(line_start(source, token.range.start));
            }
            "# gdkit: on" if start.is_some() => {
                ranges.push(Span::new(
                    start.take().unwrap(),
                    line_end(source, token.range.end),
                ));
            }
            _ => {}
        }
    }
    if let Some(start) = start {
        ranges.push(Span::new(start, source.len()));
    }
    ranges
}

fn disabled(ranges: &[Span], span: Span) -> bool {
    ranges
        .iter()
        .any(|range| span.start < range.end && span.end > range.start)
}

fn script_member_category(kind: K, exported: bool, onready: bool, private: bool) -> usize {
    if kind == K::SignalDecl {
        0
    } else if kind == K::ConstDecl {
        1
    } else if exported {
        2
    } else if onready {
        3
    } else if private {
        5
    } else {
        4
    }
}

fn visual_width(text: &str, tab_width: usize) -> usize {
    let tab_width = tab_width.max(1);
    text.chars().fold(0, |column, ch| {
        if ch == '\t' {
            column + tab_width - column % tab_width
        } else {
            column + 1
        }
    })
}

fn inlineable_statement(kind: K) -> bool {
    matches!(
        kind,
        K::ReturnStmt
            | K::BreakStmt
            | K::ContinueStmt
            | K::PassStmt
            | K::BreakpointStmt
            | K::AssertStmt
            | K::ExprStmt
            | K::VarDecl
            | K::ConstDecl
    )
}

fn inline_suite_edit(node: Node<'_>, source: &str) -> Option<Span> {
    let block = node.children().find(|child| child.kind() == K::Block)?;
    let mut statements = block.children();
    let statement = statements.next()?;
    if !inlineable_statement(statement.kind()) || statements.next().is_some() {
        return None;
    }
    let colon = node
        .children_with_tokens()
        .filter_map(|element| match element {
            Element::Token(token) if token.kind == K::Colon => Some(token),
            _ => None,
        })
        .rfind(|token| token.range.end <= block.range().start)?;
    let first = statement.tokens().find(|token| significant(token.kind))?;
    let last = statement
        .tokens()
        .filter(|token| significant(token.kind))
        .last()?;
    let owner_start = node
        .tokens()
        .find(|token| significant(token.kind))?
        .range
        .start;
    let start = line_start(source, owner_start);
    let indent = &source[start..owner_start];
    let header = &source[start..colon.range.end];
    if !indent.bytes().all(|byte| matches!(byte, b' ' | b'\t')) || header.contains(['\r', '\n']) {
        return None;
    }
    let body = &source[first.range.start..last.range.end];
    if body.contains(['\r', '\n']) {
        return None;
    }
    let gap = &source[colon.range.end..first.range.start];
    let gap = gap.trim_start_matches([' ', '\t']);
    let indentation = gap
        .strip_prefix("\r\n")
        .or_else(|| gap.strip_prefix('\n'))?;
    if indentation.is_empty() || !indentation.bytes().all(|byte| matches!(byte, b' ' | b'\t')) {
        return None;
    }
    let tail = &source[last.range.end..];
    let tail = &tail[..tail.find(['\r', '\n']).unwrap_or(tail.len())];
    if !tail.bytes().all(|byte| matches!(byte, b' ' | b'\t')) {
        return None;
    }
    for token in block.tokens() {
        if token.kind == K::Semicolon {
            return None;
        }
        if matches!(
            token.kind,
            K::LineComment | K::DocComment | K::RegionComment | K::EndRegionComment
        ) {
            let comment_indent = &source[line_start(source, token.range.start)..token.range.start];
            if token.range.start < last.range.end || comment_indent.len() > indent.len() {
                return None;
            }
        }
    }
    Some(Span::new(colon.range.end, first.range.start))
}

pub fn format_source(source: &str, options: &Options) -> Result<String, SyntaxError> {
    let parsed = parse(source);
    if let Some(error) = parsed.errors().first() {
        return Err(error.clone());
    }
    let reordered = reorder_fields(source);
    let trailed = normalize_trailing_commas(&reordered)?;
    let spaced = normalize_inline_spacing(&trailed)?;
    let normalized = normalize_whitespace(&spaced)?;
    let source = normalized.as_str();
    let parsed = parse(source);
    let disabled_ranges = disabled_ranges(source);
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for node in parsed
        .root()
        .descendants()
        .filter(|node| suite_owner(node.kind()))
    {
        if disabled(&disabled_ranges, node.range()) {
            continue;
        }
        if let Some(edit) = inline_suite_edit(node, source) {
            output.push_str(&source[cursor..edit.start]);
            output.push(' ');
            cursor = edit.end;
        }
    }
    output.push_str(&source[cursor..]);
    let spaced = normalize_inline_spacing(&output)?;
    let normalized = normalize_whitespace(&spaced)?;
    enforce_line_width(&normalized, options)
}

fn suite_owner(kind: K) -> bool {
    matches!(
        kind,
        K::FuncDecl
            | K::IfStmt
            | K::ElifClause
            | K::ElseClause
            | K::ForStmt
            | K::WhileStmt
            | K::MatchArm
            | K::LambdaExpr
            | K::Getter
            | K::Setter
    )
}

fn enforce_line_width(source: &str, options: &Options) -> Result<String, SyntaxError> {
    let expanded = expand_over_width_suites(source, options)?;
    wrap_call_arguments(&expanded, options)
}

fn expand_over_width_suites(source: &str, options: &Options) -> Result<String, SyntaxError> {
    let parsed = parse(source);
    if let Some(error) = parsed.errors().first() {
        return Err(error.clone());
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let disabled_ranges = disabled_ranges(source);
    let mut edits = Vec::new();
    for node in parsed
        .root()
        .descendants()
        .filter(|node| suite_owner(node.kind()))
    {
        if disabled(&disabled_ranges, node.range()) {
            continue;
        }
        let Some(block) = node.children().find(|child| child.kind() == K::Block) else {
            continue;
        };
        let mut statements = block.children();
        let Some(statement) = statements.next() else {
            continue;
        };
        if !inlineable_statement(statement.kind()) || statements.next().is_some() {
            continue;
        }
        let Some(colon) = node
            .children_with_tokens()
            .filter_map(|element| match element {
                Element::Token(token) if token.kind == K::Colon => Some(token),
                _ => None,
            })
            .rfind(|token| token.range.end <= block.range().start)
        else {
            continue;
        };
        let Some(first) = statement.tokens().find(|token| significant(token.kind)) else {
            continue;
        };
        let Some(last) = statement
            .tokens()
            .filter(|token| significant(token.kind))
            .last()
        else {
            continue;
        };
        let Some(owner_start) = node
            .tokens()
            .find(|token| significant(token.kind))
            .map(|token| token.range.start)
        else {
            continue;
        };
        let start = line_start(source, owner_start);
        let indent = &source[start..owner_start];
        let gap = &source[colon.range.end..first.range.start];
        let tail = &source[last.range.end..line_end(source, last.range.end)];
        let unsafe_tokens = block.tokens().any(|token| {
            if token.kind == K::Semicolon {
                return true;
            }
            if matches!(
                token.kind,
                K::LineComment | K::DocComment | K::RegionComment | K::EndRegionComment
            ) {
                let comment_indent =
                    &source[line_start(source, token.range.start)..token.range.start];
                return token.range.start < last.range.end || comment_indent.len() > indent.len();
            }
            false
        });
        if !indent.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
            || !gap.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
            || !tail
                .trim_end_matches(['\r', '\n'])
                .bytes()
                .all(|byte| matches!(byte, b' ' | b'\t'))
            || source[first.range.start..last.range.end].contains(['\r', '\n'])
            || unsafe_tokens
        {
            continue;
        }
        let line = source[start..line_end(source, last.range.end)].trim_end_matches(['\r', '\n']);
        if visual_width(line, options.tab_width) > options.line_width {
            edits.push((
                colon.range.end,
                first.range.start,
                format!("{newline}{indent}\t"),
            ));
        }
    }
    let mut output = source.to_owned();
    edits.sort_by_key(|edit| edit.0);
    for (start, end, replacement) in edits.into_iter().rev() {
        output.replace_range(start..end, &replacement);
    }
    Ok(output)
}

fn wrap_call_arguments(source: &str, options: &Options) -> Result<String, SyntaxError> {
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut output = source.to_owned();
    loop {
        let parsed = parse(&output);
        if let Some(error) = parsed.errors().first() {
            return Err(error.clone());
        }
        let disabled_ranges = disabled_ranges(&output);
        let mut candidates = Vec::new();
        for node in parsed
            .root()
            .descendants()
            .filter(|node| node.kind() == K::ArgList)
        {
            let range = node.range();
            if disabled(&disabled_ranges, range)
                || output[range.start..range.end].contains(['\r', '\n'])
                || node.children().next().is_none()
            {
                continue;
            }
            let start = line_start(&output, range.start);
            let end = line_end(&output, range.end);
            let line = output[start..end].trim_end_matches(['\r', '\n']);
            if visual_width(line, options.tab_width) > options.line_width {
                candidates.push(node);
            }
        }
        let mut edits = Vec::new();
        for node in candidates.iter().copied().filter(|node| {
            !candidates.iter().any(|other| {
                other.range().start < node.range().start && other.range().end > node.range().end
            })
        }) {
            let range = node.range();
            let start = line_start(&output, range.start);
            let before = &output[start..range.start];
            let leading = &before[..before.len() - before.trim_start_matches([' ', '\t']).len()];
            let argument_indent = format!("{leading}\t");
            let arguments: Vec<_> = node.children().collect();
            let mut replacement = String::from("(");
            replacement.push_str(newline);
            for argument in arguments {
                replacement.push_str(&argument_indent);
                let range = argument.range();
                replacement.push_str(output[range.start..range.end].trim_matches([' ', '\t']));
                replacement.push(',');
                replacement.push_str(newline);
            }
            replacement.push_str(leading);
            replacement.push(')');
            edits.push((range.start, range.end, replacement));
        }
        if edits.is_empty() {
            break;
        }
        edits.sort_by_key(|edit| edit.0);
        for (start, end, replacement) in edits.into_iter().rev() {
            output.replace_range(start..end, &replacement);
        }
    }
    normalize_call_argument_indentation(&output)
}

fn normalize_call_argument_indentation(source: &str) -> Result<String, SyntaxError> {
    let parsed = parse(source);
    if let Some(error) = parsed.errors().first() {
        return Err(error.clone());
    }
    let disabled_ranges = disabled_ranges(source);
    let mut edits = Vec::new();
    for node in parsed
        .root()
        .descendants()
        .filter(|node| node.kind() == K::ArgList)
    {
        let range = node.range();
        if disabled(&disabled_ranges, range)
            || !source[range.start..range.end].contains(['\r', '\n'])
        {
            continue;
        }
        let start = line_start(source, range.start);
        let before = &source[start..range.start];
        let leading = &before[..before.len() - before.trim_start_matches([' ', '\t']).len()];
        let argument_indent = format!("{leading}\t");
        let arguments: Vec<_> = node.children().collect();
        let close = node
            .children_with_tokens()
            .find_map(|element| match element {
                Element::Token(token) if token.kind == K::RParen => Some(token),
                _ => None,
            })
            .unwrap();
        let mut prefixes = Vec::new();
        for argument in arguments {
            let argument_start = argument
                .tokens()
                .find(|token| significant(token.kind))
                .unwrap()
                .range
                .start;
            let start = line_start(source, argument_start);
            let prefix = &source[start..argument_start];
            if start == line_start(source, range.start)
                || !prefix.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
            {
                prefixes.clear();
                break;
            }
            prefixes.push((start, argument_start, argument_indent.clone()));
        }
        if prefixes.is_empty() {
            continue;
        }
        let close_start = line_start(source, close.range.start);
        let close_prefix = &source[close_start..close.range.start];
        if close_start == line_start(source, range.start)
            || !close_prefix
                .bytes()
                .all(|byte| matches!(byte, b' ' | b'\t'))
        {
            continue;
        }
        edits.extend(prefixes);
        edits.push((close_start, close.range.start, leading.to_owned()));
    }
    let mut output = source.to_owned();
    edits.sort_by_key(|edit| edit.0);
    edits.dedup_by_key(|edit| (edit.0, edit.1));
    for (start, end, replacement) in edits.into_iter().rev() {
        output.replace_range(start..end, &replacement);
    }
    Ok(output)
}

fn normalize_trailing_commas(source: &str) -> Result<String, SyntaxError> {
    let parsed = parse(source);
    let disabled_ranges = disabled_ranges(source);
    let mut edits = Vec::new();
    for node in parsed
        .root()
        .descendants()
        .filter(|node| matches!(node.kind(), K::ArrayLit | K::DictLit | K::EnumDecl))
    {
        if disabled(&disabled_ranges, node.range()) {
            continue;
        }
        let tokens: Vec<_> = node
            .tokens()
            .filter(|token| significant(token.kind))
            .collect();
        let Some(open_index) = tokens
            .iter()
            .position(|token| matches!(token.kind, K::LBrack | K::LBrace))
        else {
            continue;
        };
        let Some(close_index) = tokens
            .iter()
            .rposition(|token| matches!(token.kind, K::RBrack | K::RBrace))
        else {
            continue;
        };
        if close_index <= open_index + 1 {
            continue;
        }
        let open = tokens[open_index];
        let close = tokens[close_index];
        let previous = tokens[close_index - 1];
        let multiline = source[open.range.end..close.range.start].contains(['\r', '\n']);
        if multiline && previous.kind != K::Comma {
            edits.push((previous.range.end, previous.range.end, ","));
        } else if !multiline && previous.kind == K::Comma {
            edits.push((previous.range.start, previous.range.end, ""));
        }
    }
    let mut output = source.to_owned();
    edits.sort_by_key(|edit| edit.0);
    for (start, end, replacement) in edits.into_iter().rev() {
        output.replace_range(start..end, replacement);
    }
    let checked = parse(&output);
    if let Some(error) = checked.errors().first() {
        return Err(error.clone());
    }
    Ok(output)
}

fn operator(kind: K) -> bool {
    matches!(
        kind,
        K::Eq
            | K::EqEq
            | K::Neq
            | K::Lt
            | K::Gt
            | K::Le
            | K::Ge
            | K::ColonEq
            | K::Arrow
            | K::Plus
            | K::Minus
            | K::Star
            | K::Slash
            | K::StarStar
            | K::Percent
            | K::Amp
            | K::Pipe
            | K::Caret
            | K::Shl
            | K::Shr
            | K::PlusEq
            | K::MinusEq
            | K::StarEq
            | K::SlashEq
            | K::StarStarEq
            | K::PercentEq
            | K::AmpEq
            | K::PipeEq
            | K::CaretEq
            | K::ShlEq
            | K::ShrEq
            | K::AmpAmp
            | K::PipePipe
            | K::Bang
            | K::Tilde
            | K::NotKw
            | K::AwaitKw
            | K::IsKw
            | K::InKw
            | K::AsKw
            | K::AndKw
            | K::OrKw
    )
}

fn expression_end(kind: K) -> bool {
    matches!(
        kind,
        K::Ident
            | K::Int
            | K::Float
            | K::String
            | K::StringName
            | K::NodePath
            | K::True
            | K::False
            | K::Null
            | K::ConstPi
            | K::ConstTau
            | K::ConstInf
            | K::ConstNan
            | K::SelfKw
            | K::SuperKw
            | K::RParen
            | K::RBrack
            | K::RBrace
    )
}

fn unary(kind: K, before: Option<K>) -> bool {
    matches!(kind, K::Plus | K::Minus | K::Bang | K::Tilde | K::Percent)
        && before.is_none_or(|kind| !expression_end(kind))
}

fn desired_gap(
    kinds: &[K],
    index: usize,
    spaced_colons: &std::collections::HashSet<usize>,
    at: usize,
) -> Option<&'static str> {
    let previous = kinds[index - 1];
    let current = kinds[index];
    let before_previous = index.checked_sub(2).map(|index| kinds[index]);
    if matches!(
        current,
        K::RParen | K::RBrack | K::Comma | K::Semicolon | K::Dot | K::DotDot
    ) {
        return Some("");
    }
    if matches!(
        previous,
        K::LParen | K::LBrack | K::Dot | K::DotDot | K::At | K::Dollar
    ) {
        return Some("");
    }
    if previous == K::LBrace {
        return Some(if current == K::RBrace { "" } else { " " });
    }
    if current == K::RBrace {
        return Some(if previous == K::LBrace { "" } else { " " });
    }
    if previous == K::Comma {
        return Some(" ");
    }
    if current == K::LBrack {
        return Some(if operator(previous) && !unary(previous, before_previous) {
            " "
        } else {
            ""
        });
    }
    if current == K::LParen {
        return Some(
            if operator(previous) && !unary(previous, before_previous)
                || matches!(previous, K::ReturnKw | K::NotKw | K::AwaitKw)
                || matches!(
                    previous,
                    K::IfKw | K::ElifKw | K::WhileKw | K::ForKw | K::MatchKw
                )
            {
                " "
            } else {
                ""
            },
        );
    }
    if current == K::Colon {
        return Some("");
    }
    if previous == K::Colon && spaced_colons.contains(&at) {
        return Some(" ");
    }
    if operator(current) {
        return Some(" ");
    }
    if operator(previous) {
        return Some(if unary(previous, before_previous) {
            ""
        } else {
            " "
        });
    }
    if matches!(previous, K::NotKw | K::AwaitKw) {
        return Some(" ");
    }
    if current == K::LBrace {
        return Some(" ");
    }
    None
}

fn normalize_inline_spacing(source: &str) -> Result<String, SyntaxError> {
    let parsed = parse(source);
    if let Some(error) = parsed.errors().first() {
        return Err(error.clone());
    }
    let mut spaced_colons = std::collections::HashSet::new();
    let disabled_ranges = disabled_ranges(source);
    for node in parsed.root().descendants().filter(|node| {
        matches!(
            node.kind(),
            K::Param
                | K::VarargParam
                | K::VarDecl
                | K::VarStmt
                | K::DictEntry
                | K::FuncDecl
                | K::IfStmt
                | K::ElifClause
                | K::ElseClause
                | K::ForStmt
                | K::WhileStmt
                | K::MatchStmt
                | K::MatchArm
                | K::InnerClassDecl
                | K::Getter
                | K::Setter
        )
    }) {
        for element in node.children_with_tokens() {
            if let Element::Token(token) = element
                && token.kind == K::Colon
            {
                spaced_colons.insert(token.range.end);
            }
        }
    }
    let (lexed, errors) = tokenize(source);
    if let Some(error) = errors.first() {
        return Err(error.clone());
    }
    let mut edits = Vec::new();
    for (index, token) in lexed.iter().enumerate() {
        if !matches!(
            token.kind,
            K::LineComment | K::DocComment | K::RegionComment | K::EndRegionComment
        ) {
            continue;
        }
        let Some(previous) = lexed[..index]
            .iter()
            .rev()
            .find(|token| token.kind != K::Whitespace)
        else {
            continue;
        };
        let gap = &source[previous.range.end..token.range.start];
        if !disabled(
            &disabled_ranges,
            Span::new(previous.range.end, token.range.end),
        ) && line_start(source, previous.range.start) == line_start(source, token.range.start)
            && gap.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
            && gap != " "
        {
            edits.push((previous.range.end, token.range.start, " "));
        }
    }
    let tokens: Vec<_> = lexed
        .into_iter()
        .filter(|token| {
            !matches!(
                token.kind,
                K::Whitespace
                    | K::NewlinePhys
                    | K::LineContinuation
                    | K::LineComment
                    | K::DocComment
                    | K::RegionComment
                    | K::EndRegionComment
                    | K::Bom
            )
        })
        .collect();
    let kinds: Vec<_> = tokens.iter().map(|token| token.kind).collect();
    for index in 1..tokens.len() {
        let previous = tokens[index - 1];
        let current = tokens[index];
        let gap = &source[previous.range.end..current.range.start];
        if !disabled(
            &disabled_ranges,
            Span::new(previous.range.end, current.range.end),
        ) && gap.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
            && let Some(desired) = desired_gap(&kinds, index, &spaced_colons, previous.range.end)
                .or_else(|| (!gap.is_empty()).then_some(" "))
            && gap != desired
        {
            edits.push((previous.range.end, current.range.start, desired));
        }
    }
    let mut output = source.to_owned();
    edits.sort_by_key(|edit| edit.0);
    for (start, end, replacement) in edits.into_iter().rev() {
        output.replace_range(start..end, replacement);
    }
    let checked = parse(&output);
    if let Some(error) = checked.errors().first() {
        return Err(error.clone());
    }
    Ok(output)
}

fn significant(kind: K) -> bool {
    !kind.is_trivia() && !kind.is_synthetic_layout() && kind != K::Eof
}

fn normalize_whitespace(source: &str) -> Result<String, SyntaxError> {
    let parsed = parse(source);
    let tokens = parsed.tokens();
    let mut starts = vec![0];
    for (at, byte) in source.bytes().enumerate() {
        if byte == b'\n' && at + 1 < source.len() {
            starts.push(at + 1);
        }
    }
    let line_index = |at: usize| {
        starts
            .partition_point(|start| *start <= at)
            .saturating_sub(1)
    };
    let mut lines: Vec<String> = source.split_inclusive('\n').map(str::to_owned).collect();
    let mut protected = vec![false; lines.len()];
    let mut protected_tail = vec![false; lines.len()];
    for range in disabled_ranges(source) {
        if !lines.is_empty() {
            let first = line_index(range.start);
            let last = line_index(range.end.saturating_sub(1));
            protected[first..last + 1].fill(true);
        }
    }
    let mut columns = std::collections::BTreeMap::from([(0, 0)]);
    let mut line_columns = vec![columns.clone(); lines.len()];
    let mut seen = vec![false; lines.len()];
    let mut column_stack = Vec::new();
    let mut previous_indent = 0usize;
    let mut logical_start = true;
    for token in tokens {
        match token.kind {
            K::Indent => {
                column_stack.push(columns.clone());
                let prefix = &source[line_start(source, token.range.start)..token.range.start];
                let column = prefix
                    .bytes()
                    .map(|b| if b == b'\t' { 4 } else { 1 })
                    .sum::<usize>();
                columns.insert(column, previous_indent + 1);
            }
            K::Dedent => {
                columns = column_stack
                    .pop()
                    .unwrap_or_else(|| std::collections::BTreeMap::from([(0, 0)]));
            }
            _ => {}
        }
        if token.kind == K::Newline {
            logical_start = true;
        }
        if !token.kind.is_synthetic_layout()
            && token.kind != K::Eof
            && token.kind != K::Whitespace
            && token.kind != K::NewlinePhys
        {
            let index = line_index(token.range.start);
            if significant(token.kind) && (logical_start || token.kind == K::FuncKw) {
                logical_start = false;
                let prefix = &source[line_start(source, token.range.start)..token.range.start];
                let prefix = &prefix[..prefix.len() - prefix.trim_start_matches([' ', '\t']).len()];
                let column = prefix
                    .bytes()
                    .map(|b| if b == b'\t' { 4 } else { 1 })
                    .sum::<usize>();
                let (&base, &level) = columns.range(..=column).next_back().unwrap();
                let unit = columns.keys().copied().find(|col| *col > 0).unwrap_or(4);
                previous_indent = level + (column - base).div_ceil(unit);
            }
            if !seen[index] {
                line_columns[index] = columns.clone();
                seen[index] = true;
            }
        }
        if matches!(token.kind, K::String | K::StringName | K::NodePath) {
            let first = line_index(token.range.start);
            let last = line_index(token.range.end.saturating_sub(1));
            if last > first {
                protected_tail[first] = true;
            }
            protected[first + 1..last + 1].fill(true);
        }
    }
    for (index, line) in lines.iter_mut().enumerate() {
        let ending = if line.ends_with("\r\n") {
            "\r\n"
        } else if line.ends_with('\n') {
            "\n"
        } else {
            ""
        };
        let content = &line[..line.len() - ending.len()];
        if protected[index] {
            continue;
        }
        let content = if protected_tail[index] {
            content
        } else {
            content.trim_end_matches([' ', '\t'])
        };
        let body = content.trim_start_matches([' ', '\t']);
        let prefix = &content[..content.len() - body.len()];
        let column = prefix
            .bytes()
            .map(|b| if b == b'\t' { 4 } else { 1 })
            .sum::<usize>();
        let columns = &line_columns[index];
        let unit = columns.keys().copied().find(|col| *col > 0).unwrap_or(4);
        let (&base, &level) = columns.range(..=column).next_back().unwrap();
        let indent = if body.is_empty() {
            0
        } else {
            level + (column - base).div_ceil(unit)
        };
        *line = format!("{}{body}{ending}", "\t".repeat(indent));
    }
    let mut spacing = std::collections::BTreeMap::new();
    for scope in parsed
        .root()
        .descendants()
        .filter(|node| matches!(node.kind(), K::SourceFile | K::ClassBody))
    {
        let mut previous: Option<(K, usize, (K, usize))> = None;
        let mut annotations = Vec::new();
        let mut annotation_start = None;
        for member in scope.children() {
            let Some(first) = member.tokens().find(|token| significant(token.kind)) else {
                continue;
            };
            let start = line_index(first.range.start);
            if member.kind() == K::Annotation {
                annotation_start.get_or_insert(start);
                if let Some(name) = member.tokens().find(|token| token.kind == K::Ident) {
                    annotations.push(source[name.range].to_owned());
                }
                continue;
            }
            let mut start = annotation_start.take().unwrap_or(start);
            let last = member
                .tokens()
                .filter(|token| significant(token.kind))
                .last()
                .unwrap();
            let end = line_index(last.range.end.saturating_sub(1));
            let private = member
                .children()
                .find(|node| node.kind() == K::Name)
                .is_some_and(|name| {
                    name.tokens()
                        .find(|token| token.kind == K::Ident)
                        .is_some_and(|token| source[token.range].starts_with('_'))
                });
            let exported = annotations
                .iter()
                .any(|name: &String| name == "export" || name.starts_with("export_"));
            let onready = annotations.iter().any(|name| name == "onready");
            annotations.clear();
            let category = if matches!(member.kind(), K::SignalDecl | K::VarDecl | K::ConstDecl) {
                (
                    K::VarDecl,
                    script_member_category(member.kind(), exported, onready, private),
                )
            } else {
                (member.kind(), 0)
            };
            if let Some((previous_kind, previous_end, previous_category)) = &previous {
                while start > previous_end + 1 && lines[start - 1].trim_start().starts_with('#') {
                    start -= 1;
                }
                if start > *previous_end {
                    let fields = matches!(
                        member.kind(),
                        K::VarDecl | K::ConstDecl | K::SignalDecl | K::EnumDecl
                    );
                    let class_documentation =
                        matches!(previous_kind, K::ExtendsClause | K::ClassNameDecl)
                            && lines[start].trim_start().starts_with("##");
                    let blanks = if class_documentation {
                        0
                    } else if member.kind() == K::FuncDecl || *previous_kind == K::FuncDecl {
                        2
                    } else if fields && category != *previous_category {
                        1
                    } else {
                        usize::from(
                            lines[previous_end + 1..start]
                                .iter()
                                .any(|line| line.trim().is_empty()),
                        )
                    };
                    spacing.insert(start, blanks);
                }
            }
            previous = Some((member.kind(), end, category));
        }
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut output = String::with_capacity(source.len());
    let mut pending = 0usize;
    for (index, line) in lines.iter().enumerate() {
        if !protected[index] && line.trim().is_empty() {
            pending += 1;
            continue;
        }
        if !output.is_empty() {
            let blanks = spacing.get(&index).copied().unwrap_or(pending.min(1));
            for _ in 0..blanks {
                output.push_str(newline);
            }
        }
        pending = 0;
        output.push_str(line);
    }
    let checked = parse(&output);
    if let Some(error) = checked.errors().first() {
        return Err(error.clone());
    }
    Ok(output)
}
fn attached_field_start(
    source: &str,
    at: usize,
    previous_end: usize,
    has_body_member: bool,
    disabled_ranges: &[Span],
) -> usize {
    let mut start = line_start(source, at);
    while start > previous_end {
        let previous_start = line_start(
            source,
            start
                .saturating_sub(1)
                .saturating_sub(usize::from(source[..start].ends_with("\r\n"))),
        );
        let line = source[previous_start..start].trim();
        if disabled(disabled_ranges, Span::new(previous_start, start))
            || previous_start < previous_end
            || !(line.is_empty() || line.starts_with('#'))
            || (!has_body_member && line.starts_with('#'))
        {
            break;
        }
        start = previous_start;
    }
    start
}

fn reorder_script_fields(source: &str) -> String {
    let parsed = parse(source);
    let scope = parsed.root();
    let disabled_ranges = disabled_ranges(source);
    if scope
        .children()
        .any(|member| disabled(&disabled_ranges, member.range()))
    {
        return source.to_owned();
    }
    let mut fields = Vec::new();
    let mut annotation_start = None;
    let mut exported = false;
    let mut onready = false;
    let mut previous_end = scope.range().start;
    let mut has_body_member = false;
    let mut body_start = None;

    for member in scope.children() {
        let Some(first) = member.tokens().find(|token| significant(token.kind)) else {
            continue;
        };
        if member.kind() == K::Annotation {
            annotation_start.get_or_insert(first.range.start);
            let name = member
                .tokens()
                .find(|token| token.kind == K::Ident)
                .map(|token| &source[token.range])
                .unwrap_or("");
            if matches!(name, "export_group" | "export_subgroup" | "export_category") {
                let at = annotation_start.take().unwrap();
                let start = attached_field_start(
                    source,
                    at,
                    previous_end,
                    has_body_member,
                    &disabled_ranges,
                );
                let last = member
                    .tokens()
                    .filter(|token| significant(token.kind))
                    .last()
                    .unwrap();
                let end = line_end(source, last.range.end);
                body_start.get_or_insert(start);
                fields.push((start, end, 2));
                previous_end = end;
                has_body_member = true;
                exported = false;
                onready = false;
            } else if name == "export" || name.starts_with("export_") {
                exported = true;
            } else if name == "onready" {
                onready = true;
            }
            continue;
        }

        let at = annotation_start.take().unwrap_or(first.range.start);
        if !matches!(member.kind(), K::ExtendsClause | K::ClassNameDecl) {
            body_start.get_or_insert(line_start(source, at));
        }
        let last = member
            .tokens()
            .filter(|token| significant(token.kind))
            .last()
            .unwrap();
        let end = line_end(source, last.range.end);
        let private = member
            .children()
            .find(|node| node.kind() == K::Name)
            .is_some_and(|name| {
                name.tokens()
                    .find(|token| token.kind == K::Ident)
                    .is_some_and(|token| source[token.range].starts_with('_'))
            });
        if matches!(member.kind(), K::SignalDecl | K::VarDecl | K::ConstDecl)
            && source[line_start(source, at)..at].trim().is_empty()
            && !source[last.range.end..end].trim().starts_with(';')
        {
            let start =
                attached_field_start(source, at, previous_end, has_body_member, &disabled_ranges);
            fields.push((
                start,
                end,
                script_member_category(member.kind(), exported, onready, private),
            ));
        }
        exported = false;
        onready = false;
        previous_end = end;
        if !matches!(member.kind(), K::ExtendsClause | K::ClassNameDecl) {
            has_body_member = true;
        }
    }

    let Some(start) = body_start else {
        return source.to_owned();
    };
    if fields.is_empty() {
        return source.to_owned();
    }
    fields.sort_by_key(|field| field.0);
    let mut remainder = String::new();
    let mut cursor = start;
    for (from, to, _) in &fields {
        if *from > cursor {
            remainder.push_str(&source[cursor..*from]);
        }
        cursor = cursor.max(*to);
    }
    remainder.push_str(&source[cursor..]);

    let mut sorted = fields.clone();
    sorted.sort_by_key(|field| field.2);
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut replacement = String::new();
    for (index, (from, to, _)) in sorted.iter().enumerate() {
        if index > 0 && !replacement.ends_with('\n') {
            replacement.push_str(newline);
        }
        replacement.push_str(&source[*from..*to]);
    }
    if !replacement.ends_with('\n') && !remainder.is_empty() {
        replacement.push_str(newline);
    }
    replacement.push_str(&remainder);

    let mut output = source[..start].to_owned();
    output.push_str(&replacement);
    output
}

fn reorder_fields(source: &str) -> String {
    let source = reorder_script_fields(source);
    let source = source.as_str();
    let parsed = parse(source);
    let disabled_ranges = disabled_ranges(source);
    let mut edits = Vec::new();
    for scope in parsed
        .root()
        .descendants()
        .filter(|node| node.kind() == K::ClassBody)
    {
        let mut fields = Vec::new();
        let mut annotation_start = None;
        let mut exported = false;
        let mut onready = false;
        let mut has_body_member = false;
        let flush = |fields: &mut Vec<(usize, usize, usize)>,
                     edits: &mut Vec<(usize, usize, String)>| {
            if fields.windows(2).any(|pair| pair[0].2 > pair[1].2) {
                let start = fields[0].0;
                let end = fields.last().unwrap().1;
                let mut sorted = fields.clone();
                sorted.sort_by_key(|field| field.2);
                let newline = if source.contains("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                };
                let mut replacement = String::new();
                for (index, (from, to, _)) in sorted.iter().enumerate() {
                    if index > 0 && !replacement.ends_with('\n') {
                        replacement.push_str(newline);
                    }
                    replacement.push_str(&source[*from..*to]);
                }
                if source[..end].ends_with('\n') && !replacement.ends_with('\n') {
                    replacement.push_str(newline);
                }
                edits.push((start, end, replacement));
            }
            fields.clear();
        };
        let mut previous_end = scope.range().start;
        for member in scope.children() {
            let Some(first) = member.tokens().find(|token| significant(token.kind)) else {
                continue;
            };
            if disabled(&disabled_ranges, member.range()) {
                flush(&mut fields, &mut edits);
                annotation_start = None;
                exported = false;
                onready = false;
                previous_end = member.range().end;
                continue;
            }
            if member.kind() == K::Annotation {
                annotation_start.get_or_insert(first.range.start);
                let name = member
                    .tokens()
                    .find(|token| token.kind == K::Ident)
                    .map(|token| &source[token.range])
                    .unwrap_or("");
                if name == "export" || name.starts_with("export_") {
                    exported = true;
                } else if name == "onready" {
                    onready = true;
                }
                if matches!(name, "export_group" | "export_subgroup" | "export_category") {
                    flush(&mut fields, &mut edits);
                    annotation_start = None;
                    previous_end = member.range().end;
                    exported = false;
                    onready = false;
                }
                continue;
            }
            let at = annotation_start.take().unwrap_or(first.range.start);
            let mut start = line_start(source, at);
            let last = member
                .tokens()
                .filter(|token| significant(token.kind))
                .last()
                .unwrap();
            let private = member
                .children()
                .find(|node| node.kind() == K::Name)
                .is_some_and(|name| {
                    name.tokens()
                        .find(|token| token.kind == K::Ident)
                        .is_some_and(|token| source[token.range].starts_with('_'))
                });
            let end = source[last.range.end..]
                .find('\n')
                .map_or(source.len(), |offset| last.range.end + offset + 1);
            if matches!(member.kind(), K::VarDecl | K::ConstDecl)
                && source[start..at].trim().is_empty()
                && !source[last.range.end..end].trim().starts_with(';')
            {
                while start > previous_end {
                    let previous_start = line_start(
                        source,
                        start
                            .saturating_sub(1)
                            .saturating_sub(usize::from(source[..start].ends_with("\r\n"))),
                    );
                    let line = source[previous_start..start].trim();
                    if disabled(&disabled_ranges, Span::new(previous_start, start))
                        || previous_start < previous_end
                        || !(line.is_empty() || line.starts_with('#'))
                        || (!has_body_member && line.starts_with('#'))
                    {
                        break;
                    }
                    start = previous_start;
                }
                fields.push((
                    start,
                    end,
                    script_member_category(member.kind(), exported, onready, private),
                ));
            } else {
                flush(&mut fields, &mut edits);
            }
            exported = false;
            onready = false;
            previous_end = end;
            if matches!(
                member.kind(),
                K::SignalDecl
                    | K::EnumDecl
                    | K::ConstDecl
                    | K::VarDecl
                    | K::FuncDecl
                    | K::InnerClassDecl
            ) {
                has_body_member = true;
            }
        }
        flush(&mut fields, &mut edits);
    }
    let mut output = source.to_owned();
    edits.sort_by_key(|edit| edit.0);
    for (start, end, replacement) in edits.into_iter().rev() {
        output.replace_range(start..end, &replacement);
    }
    output
}

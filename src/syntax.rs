use std::collections::BTreeMap;
use std::path::Path;

use tree_sitter::Language;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::error::{AppError, Result};

pub const MAX_HIGHLIGHT_BYTES: usize = 2 * 1024 * 1024;
pub const HIGHLIGHT_NAMES: [&str; 8] = [
    "comment", "string", "number", "keyword", "function", "type", "property", "variable",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxClass {
    Comment,
    String,
    Number,
    Keyword,
    Function,
    Type,
    Property,
    Variable,
}

impl SyntaxClass {
    pub const fn index(self) -> usize {
        match self {
            Self::Comment => 0,
            Self::String => 1,
            Self::Number => 2,
            Self::Keyword => 3,
            Self::Function => 4,
            Self::Type => 5,
            Self::Property => 6,
            Self::Variable => 7,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxSpan {
    pub start: usize,
    pub end: usize,
    pub class: SyntaxClass,
}

#[derive(Clone, Debug, Default)]
pub struct HighlightedSource {
    pub lines: BTreeMap<u32, Vec<SyntaxSpan>>,
    pub source_bytes: usize,
}

pub fn highlight(path: &Path, source: &[u8]) -> Result<Option<HighlightedSource>> {
    if source.len() > MAX_HIGHLIGHT_BYTES {
        return Ok(None);
    }
    let source_text = match std::str::from_utf8(source) {
        Ok(source) => source,
        Err(_) => return Ok(None),
    };
    let Some(mut config) = configuration(path)? else {
        return Ok(None);
    };
    config.configure(&HIGHLIGHT_NAMES);
    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(&config, source, None, |_| None)
        .map_err(|error| invalid("syntax_highlight_failed", error.to_string()))?;
    let offsets = line_offsets(source_text);
    let mut active = Vec::new();
    let mut lines: BTreeMap<u32, Vec<SyntaxSpan>> = BTreeMap::new();
    for event in events {
        match event.map_err(|error| invalid("syntax_highlight_failed", error.to_string()))? {
            HighlightEvent::HighlightStart(highlight) => active.push(class(highlight.0)),
            HighlightEvent::HighlightEnd => {
                active.pop();
            }
            HighlightEvent::Source { start, end } => {
                let Some(class) = active.last().copied().flatten() else {
                    continue;
                };
                project_range(source_text, &offsets, start, end, class, &mut lines);
            }
        }
    }
    Ok(Some(HighlightedSource {
        lines,
        source_bytes: source.len(),
    }))
}

fn configuration(path: &Path) -> Result<Option<HighlightConfiguration>> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let (language, name, highlights, injections, locals): (Language, _, String, _, _) =
        match extension {
            "js" | "mjs" | "cjs" => (
                tree_sitter_javascript::LANGUAGE.into(),
                "javascript",
                tree_sitter_javascript::HIGHLIGHT_QUERY.into(),
                tree_sitter_javascript::INJECTIONS_QUERY,
                tree_sitter_javascript::LOCALS_QUERY,
            ),
            "jsx" => (
                tree_sitter_javascript::LANGUAGE.into(),
                "jsx",
                format!(
                    "{}\n{}",
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                    tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
                ),
                tree_sitter_javascript::INJECTIONS_QUERY,
                tree_sitter_javascript::LOCALS_QUERY,
            ),
            "ts" => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                "typescript",
                format!(
                    "{}\n{}",
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                    tree_sitter_typescript::HIGHLIGHTS_QUERY
                ),
                "",
                tree_sitter_typescript::LOCALS_QUERY,
            ),
            "tsx" => (
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                "tsx",
                format!(
                    "{}\n{}\n{}",
                    tree_sitter_javascript::HIGHLIGHT_QUERY,
                    tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                    tree_sitter_typescript::HIGHLIGHTS_QUERY
                ),
                "",
                tree_sitter_typescript::LOCALS_QUERY,
            ),
            "html" | "htm" => (
                tree_sitter_html::LANGUAGE.into(),
                "html",
                tree_sitter_html::HIGHLIGHTS_QUERY.into(),
                tree_sitter_html::INJECTIONS_QUERY,
                "",
            ),
            "css" => (
                tree_sitter_css::LANGUAGE.into(),
                "css",
                tree_sitter_css::HIGHLIGHTS_QUERY.into(),
                "",
                "",
            ),
            _ => return Ok(None),
        };
    HighlightConfiguration::new(language, name, &highlights, injections, locals)
        .map(Some)
        .map_err(|error| invalid("syntax_query_invalid", error.to_string()))
}

fn class(index: usize) -> Option<SyntaxClass> {
    Some(match index {
        0 => SyntaxClass::Comment,
        1 => SyntaxClass::String,
        2 => SyntaxClass::Number,
        3 => SyntaxClass::Keyword,
        4 => SyntaxClass::Function,
        5 => SyntaxClass::Type,
        6 => SyntaxClass::Property,
        7 => SyntaxClass::Variable,
        _ => return None,
    })
}

fn line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    offsets.extend(source.match_indices('\n').map(|(offset, _)| offset + 1));
    offsets
}

fn project_range(
    source: &str,
    offsets: &[usize],
    start: usize,
    end: usize,
    class: SyntaxClass,
    result: &mut BTreeMap<u32, Vec<SyntaxSpan>>,
) {
    if start >= end || end > source.len() {
        return;
    }
    let first_line = offsets
        .partition_point(|offset| *offset <= start)
        .saturating_sub(1);
    let last_line = offsets
        .partition_point(|offset| *offset < end)
        .saturating_sub(1);
    for line in first_line..=last_line {
        let line_start = offsets[line];
        let raw_end = offsets.get(line + 1).copied().unwrap_or(source.len());
        let line_end = source[..raw_end].trim_end_matches(['\n', '\r']).len();
        let local_start = start
            .max(line_start)
            .min(line_end)
            .saturating_sub(line_start);
        let local_end = end.max(line_start).min(line_end).saturating_sub(line_start);
        if local_start >= local_end {
            continue;
        }
        let raw_line = &source[line_start..line_end];
        if !raw_line.is_char_boundary(local_start) || !raw_line.is_char_boundary(local_end) {
            continue;
        }
        result.entry(line as u32).or_default().push(SyntaxSpan {
            start: display_column(&raw_line[..local_start]),
            end: display_column(&raw_line[..local_end]),
            class,
        });
    }
}

fn display_column(value: &str) -> usize {
    value
        .chars()
        .map(|character| if character == '\t' { 4 } else { 1 })
        .sum()
}

fn invalid(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::InvalidInput {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_grammar_loads_and_highlights() {
        for (path, source) in [
            ("a.ts", "const value: string = 'x';"),
            ("a.tsx", "export const A = () => <div>ok</div>;"),
            ("a.js", "const value = 42;"),
            ("a.jsx", "const A = () => <div>ok</div>;"),
            ("a.html", "<main class=\"x\">ok</main>"),
            ("a.css", ".x { color: red; }"),
        ] {
            let result = highlight(Path::new(path), source.as_bytes())
                .unwrap()
                .unwrap();
            assert!(!result.lines.is_empty(), "no highlight for {path}");
        }
    }

    #[test]
    fn projects_multiline_crlf_tabs_and_emoji_to_display_columns() {
        let source = "const x = `hello\r\n\t😀 world`;\r\n";
        let result = highlight(Path::new("a.ts"), source.as_bytes())
            .unwrap()
            .unwrap();
        let second = result.lines.get(&1).unwrap();
        assert!(second.iter().any(|span| span.start == 0 && span.end >= 11));
    }

    #[test]
    fn unsupported_and_oversized_sources_fall_back() {
        assert!(
            highlight(Path::new("a.rs"), b"fn main() {}")
                .unwrap()
                .is_none()
        );
        assert!(
            highlight(Path::new("a.ts"), &vec![b'x'; MAX_HIGHLIGHT_BYTES + 1])
                .unwrap()
                .is_none()
        );
    }
}

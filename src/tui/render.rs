use std::path::Path;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use unicode_width::UnicodeWidthChar;

use super::graph::{LinkedTest, LinkedTestKind, RelatedHunk, relation_label};
use super::preview::{LineKind, Preview, SourceState, SyntaxState};
use super::{App, Focus, PaneMode, Picker};
use crate::graph::EdgeKind;
use crate::progress::{ReviewProgress, ReviewStatus};
use crate::ranking::QueueItem;

pub(super) fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let theme = app.theme();
    frame.render_widget(Block::default().style(theme.background), frame.area());
    if frame.area().width < 60 || frame.area().height < 18 {
        frame.render_widget(
            Paragraph::new("Terminal too small for review. Resize to at least 60×18 or press q.")
                .style(theme.warning)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(theme.border)
                        .title("LGR"),
                ),
            frame.area(),
        );
        return;
    }

    let list_height = if frame.area().height >= 30 { 11 } else { 7 };
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(list_height),
        Constraint::Min(8),
        Constraint::Length(1),
    ])
    .split(frame.area());
    render_status(frame, app, areas[0]);
    render_list(frame, app, areas[1]);
    render_preview(frame, app, areas[2]);
    render_footer(frame, app, areas[3]);

    if app.help {
        render_help(frame, app);
    } else if app.evidence {
        render_evidence(frame, app);
    } else if app.picker.is_some() {
        render_picker(frame, app);
    }
}

fn render_status(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = app.theme();
    let tags = if app.tag_filters.is_empty() {
        "all tags".into()
    } else {
        app.tag_filters
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("|")
    };
    let status = match app.status_filter {
        None => "all",
        Some(ReviewStatus::Reviewed) => "reviewed",
        Some(ReviewStatus::Unreviewed) => "unreviewed",
    };
    let view = app
        .current_center_title()
        .map(|title| format!("Related to: {}", clip_end(title, area.width as usize / 2)))
        .unwrap_or_else(|| "Ranked review".into());
    let line = Line::from(vec![
        Span::styled(view, theme.focus.add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(
            format!(
                "{}/{} reviewed · {} ranked · {tags} · {status}",
                app.reviewed_count(),
                app.queue.total_changes,
                app.queue.assessed_changes,
            ),
            theme.muted,
        ),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border)
                .title("LGR"),
        ),
        area,
    );
}

fn render_list(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let theme = app.theme();
    let focused = app.frame.focus == Focus::List;
    let border = if focused { theme.focus } else { theme.border };
    let width = area.width.saturating_sub(2) as usize;
    let items: Vec<ListItem<'_>> = if app.is_queue() {
        app.visible_queue()
            .into_iter()
            .map(|item| queue_row(item, &app.progress, app, width))
            .collect()
    } else {
        app.related()
            .into_iter()
            .map(|item| related_row(item, &app.progress, width, &theme))
            .collect()
    };
    let empty = items.is_empty();
    let items = if empty {
        vec![ListItem::new(if app.is_queue() {
            "No hunks match. Press t to clear tags or a for all statuses."
        } else {
            "No indexed related changes. Press v for source context, h to go back."
        })]
    } else {
        items
    };
    let title = if app.is_queue() {
        format!("Queue ({})", app.visible_queue_indices().len())
    } else {
        format!("Related changes ({})", app.related().len())
    };
    let mut state = ListState::default()
        .with_selected((!empty).then_some(app.frame.cursor))
        .with_offset(app.frame.list_offset);
    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border)
                    .title(title),
            )
            .highlight_style(theme.selected)
            .highlight_symbol("> "),
        area,
        &mut state,
    );
    app.frame.list_offset = state.offset();
}

fn queue_row<'a>(
    item: &'a QueueItem,
    progress: &ReviewProgress,
    app: &App,
    width: usize,
) -> ListItem<'a> {
    let theme = app.theme();
    let marker = if progress.status(&item.node_id) == ReviewStatus::Reviewed {
        "✓"
    } else {
        "·"
    };
    let score = item
        .score
        .map_or("--".into(), |score| format!("{score:02}"));
    let line_number = app
        .graph
        .node(&item.node_id)
        .and_then(|node| node.preferred_review_location())
        .map(|location| location.range.start.line + 1);
    let location = compact_location(item.path.as_deref(), line_number, width / 4);
    let prefix = format!("{marker} {score} ");
    let suffix = format!("  {location}");
    let title_width = width
        .saturating_sub(display_width(&prefix))
        .saturating_sub(display_width(&suffix));
    ListItem::new(Line::from(vec![
        Span::styled(prefix, theme.important),
        Span::styled(clip_end(&item.title, title_width), theme.text),
        Span::styled(suffix, theme.muted),
    ]))
}

fn related_row<'a>(
    item: &'a RelatedHunk,
    progress: &ReviewProgress,
    width: usize,
    theme: &super::theme::Theme,
) -> ListItem<'a> {
    let marker = if progress.status(&item.node_id) == ReviewStatus::Reviewed {
        "✓"
    } else {
        "·"
    };
    let score = item
        .score
        .map_or("--".into(), |score| format!("{score:02}"));
    let relation = item.primary_label();
    let prefix = format!(
        "{marker} {:<10} {} {score} ",
        relation,
        item.direction_marker()
    );
    let location = compact_location(item.path.as_deref(), item.line, width / 4);
    let suffix = format!("  {location}");
    let title_width = width
        .saturating_sub(display_width(&prefix))
        .saturating_sub(display_width(&suffix));
    let relation_style = match item.categories.iter().min_by_key(|kind| match kind {
        EdgeKind::RuntimeTest => 0,
        EdgeKind::TestReference => 1,
        EdgeKind::References => 2,
        EdgeKind::Definition => 3,
        EdgeKind::Calls => 4,
        _ => 5,
    }) {
        Some(EdgeKind::RuntimeTest | EdgeKind::TestReference) => theme.test,
        Some(EdgeKind::Definition) => theme.definition,
        _ => theme.usage,
    };
    ListItem::new(Line::from(vec![
        Span::styled(prefix, relation_style),
        Span::styled(clip_end(&item.title, title_width), theme.text),
        Span::styled(suffix, theme.muted),
    ]))
}

fn render_preview(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let theme = app.theme();
    let focused = app.frame.focus == Focus::Preview;
    let border = if focused { theme.focus } else { theme.border };
    if app.frame.pane_mode == PaneMode::Tests {
        render_tests_preview(frame, app, area, border);
        return;
    }
    if app.frame.pane_mode != PaneMode::Code {
        render_context_preview(frame, app, area, border);
        return;
    }
    let Some(preview_len) = app.selected_preview().map(|preview| preview.lines.len()) else {
        frame.render_widget(
            Paragraph::new("No captured text patch for this change.")
                .style(theme.muted)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(border)
                        .title("Preview"),
                ),
            area,
        );
        return;
    };
    let visible_height = area.height.saturating_sub(2) as usize;
    let max_offset = preview_len.saturating_sub(visible_height.max(1));
    app.frame.preview_offset = app.frame.preview_offset.min(max_offset);
    let preview = app.selected_preview().expect("preview remains selected");
    let end = (app.frame.preview_offset + visible_height).min(preview.lines.len());
    let preview_id = app.selected_preview_id().unwrap_or_default();
    let mut lines = preview_lines(
        app,
        preview,
        preview_id,
        app.frame.preview_offset,
        end,
        true,
    );
    if preview.truncated && end == preview.lines.len() && lines.len() < visible_height {
        lines.push(Line::styled(
            "… preview cache limit reached; open Diffview for full patch",
            theme.warning,
        ));
    }
    let title = format!(
        "{} · {} · {} · {}/{}",
        if app.frame.full_parent {
            "Full parent"
        } else {
            "Code"
        },
        compact_location(Some(&preview.path), None, area.width as usize / 2),
        preview.status,
        app.frame.preview_offset.saturating_add(1),
        preview.lines.len().max(1)
    );
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border)
                .title(title),
        ),
        area,
    );
}

fn render_tests_preview(
    frame: &mut Frame<'_>,
    app: &mut App,
    area: Rect,
    border: ratatui::style::Style,
) {
    let theme = app.theme();
    let tests_len = app.linked_tests().len();
    if tests_len == 0 {
        frame.render_widget(
            Paragraph::new(
                "No linked tests. Run before/after test evidence or index changed test references.",
            )
            .style(theme.muted)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border)
                    .title("Tests"),
            ),
            area,
        );
        return;
    }

    app.frame.test_cursor = app.frame.test_cursor.min(tests_len - 1);
    let list_height = (tests_len as u16 + 2)
        .min(8)
        .min(area.height.saturating_sub(5));
    let [list_area, preview_area] =
        Layout::vertical([Constraint::Length(list_height.max(3)), Constraint::Min(3)]).areas(area);
    let (changed, before, after) = app.linked_tests().iter().fold(
        (0usize, 0usize, 0usize),
        |(changed, before, after), test| match test.kind {
            LinkedTestKind::Changed => (changed + 1, before, after),
            LinkedTestKind::Before => (changed, before + 1, after),
            LinkedTestKind::After => (changed, before, after + 1),
        },
    );
    let items = app
        .linked_tests()
        .iter()
        .map(|test| test_row(test, list_area.width.saturating_sub(2) as usize, &theme))
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(Some(app.frame.test_cursor));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(theme.selected)
            .highlight_symbol("> ")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border)
                    .title(format!(
                        "Tests · {changed} changed · {before} before · {after} after"
                    )),
            ),
        list_area,
        &mut state,
    );

    let Some(test) = app.selected_test().cloned() else {
        return;
    };
    let Some(preview_len) = app.test_preview().map(|preview| preview.lines.len()) else {
        frame.render_widget(
            Paragraph::new("No captured source preview for this test evidence.")
                .style(theme.muted)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(border)
                        .title(test.title),
                ),
            preview_area,
        );
        return;
    };
    let visible_height = preview_area.height.saturating_sub(2) as usize;
    let max_offset = preview_len.saturating_sub(visible_height.max(1));
    app.frame.test_preview_offset = app.frame.test_preview_offset.min(max_offset);
    let preview = app.test_preview().expect("test preview remains selected");
    let end = (app.frame.test_preview_offset + visible_height).min(preview.lines.len());
    let mut lines = preview_lines(
        app,
        preview,
        &test.node_id,
        app.frame.test_preview_offset,
        end,
        false,
    );
    if preview.truncated && end == preview.lines.len() && lines.len() < visible_height {
        lines.push(Line::styled("… preview cache limit reached", theme.warning));
    }
    let detail = [test.granularity.as_deref(), test.status.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let title = format!(
        "{}{} · {} · {}/{}",
        test.kind_label(),
        if detail.is_empty() {
            String::new()
        } else {
            format!(" · {detail}")
        },
        compact_location(
            Some(&preview.path),
            test.line,
            preview_area.width as usize / 2
        ),
        app.frame.test_preview_offset.saturating_add(1),
        preview.lines.len().max(1)
    );
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border)
                .title(title),
        ),
        preview_area,
    );
}

fn test_row<'a>(test: &'a LinkedTest, width: usize, theme: &super::theme::Theme) -> ListItem<'a> {
    let detail = [test.granularity.as_deref(), test.status.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("/");
    let prefix = format!("{:<7} {:<11} ", test.kind_label(), detail);
    let location = compact_location(test.path.as_deref(), test.line, width / 4);
    let suffix = format!("  {location}");
    let title_width = width
        .saturating_sub(display_width(&prefix))
        .saturating_sub(display_width(&suffix));
    ListItem::new(Line::from(vec![
        Span::styled(prefix, theme.test),
        Span::styled(clip_end(&test.title, title_width), theme.text),
        Span::styled(suffix, theme.muted),
    ]))
}

fn preview_lines<'a>(
    app: &'a App,
    preview: &'a Preview,
    preview_id: &str,
    start: usize,
    end: usize,
    annotations: bool,
) -> Vec<Line<'a>> {
    let theme = app.theme();
    let syntax = app.syntax_state(preview_id);
    preview.lines[start..end]
        .iter()
        .map(|line| {
            let old = line
                .old
                .map_or("    ".into(), |value| format!("{value:>4}"));
            let new = line
                .new
                .map_or("    ".into(), |value| format!("{value:>4}"));
            let style = match line.kind {
                LineKind::Added => theme.addition_line,
                LineKind::Deleted => theme.deletion_line,
                LineKind::Header => theme.definition,
                LineKind::Note => theme.warning,
                LineKind::Context => theme.text,
                LineKind::Surrounding => theme.muted.add_modifier(Modifier::DIM),
            };
            let annotation = annotations
                .then(|| match line.kind {
                    LineKind::Added | LineKind::Context => line
                        .new
                        .and_then(|line| app.annotation_for(crate::graph::SourceSide::Right, line)),
                    LineKind::Deleted => line
                        .old
                        .and_then(|line| app.annotation_for(crate::graph::SourceSide::Left, line)),
                    _ => None,
                })
                .flatten();
            let marker_style = match line.kind {
                LineKind::Added => theme.addition_line.add_modifier(Modifier::BOLD),
                LineKind::Deleted => theme.deletion_line.add_modifier(Modifier::BOLD),
                _ => style,
            };
            let mut spans = vec![
                Span::styled(format!("{old} {new} "), on_line(theme.muted, style)),
                Span::styled(format!("{} ", line.marker), marker_style),
                Span::styled(
                    if annotation.is_some() { "◆ " } else { "  " },
                    on_line(
                        if annotation.is_some() {
                            theme.important.add_modifier(Modifier::UNDERLINED)
                        } else {
                            theme.muted
                        },
                        style,
                    ),
                ),
            ];
            let tokens = match (syntax, line.kind) {
                (Some(SyntaxState::Ready { right, .. }), LineKind::Added) => line
                    .new
                    .and_then(|line| right.as_ref()?.lines.get(&line.saturating_sub(1))),
                (Some(SyntaxState::Ready { left, .. }), LineKind::Deleted) => line
                    .old
                    .and_then(|line| left.as_ref()?.lines.get(&line.saturating_sub(1))),
                (
                    Some(SyntaxState::Ready { right, left }),
                    LineKind::Context | LineKind::Surrounding,
                ) => line
                    .new
                    .and_then(|line| right.as_ref()?.lines.get(&line.saturating_sub(1)))
                    .or_else(|| {
                        line.old
                            .and_then(|line| left.as_ref()?.lines.get(&line.saturating_sub(1)))
                    }),
                _ => None,
            };
            spans.extend(styled_syntax(
                &line.text,
                tokens.map(Vec::as_slice),
                style,
                &theme,
            ));
            Line::from(spans)
        })
        .collect()
}

fn on_line(mut style: ratatui::style::Style, line: ratatui::style::Style) -> ratatui::style::Style {
    if let Some(background) = line.bg {
        style = style.bg(background);
    }
    style.add_modifier(line.add_modifier)
}

fn render_context_preview(
    frame: &mut Frame<'_>,
    app: &mut App,
    area: Rect,
    border: ratatui::style::Style,
) {
    let theme = app.theme();
    let context = app.context_lines();
    let visible_height = area.height.saturating_sub(2) as usize;
    let max_offset = context.len().saturating_sub(visible_height.max(1));
    app.frame.context_offset = app.frame.context_offset.min(max_offset);
    let end = (app.frame.context_offset + visible_height).min(context.len());
    let lines: Vec<Line<'_>> = context[app.frame.context_offset..end]
        .iter()
        .map(|line| {
            if line.starts_with("──") {
                Line::styled(line, theme.focus.add_modifier(Modifier::BOLD))
            } else if line.starts_with('[') {
                Line::styled(line, theme.usage)
            } else if line.starts_with("Passed") {
                Line::styled(line, theme.test)
            } else if line.starts_with("Failed") {
                Line::styled(line, theme.error)
            } else {
                Line::styled(line, theme.text)
            }
        })
        .collect();
    let title = match app.frame.pane_mode {
        PaneMode::Context => "Context",
        PaneMode::SessionContext => "Session context",
        PaneMode::Code | PaneMode::Tests => unreachable!(),
    };
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border)
                .title(format!(
                    "{title} · {}/{}",
                    app.frame.context_offset.saturating_add(1),
                    context.len().max(1)
                )),
        ),
        area,
    );
}

fn styled_syntax<'a>(
    text: &'a str,
    tokens: Option<&[crate::syntax::SyntaxSpan]>,
    base: ratatui::style::Style,
    theme: &super::theme::Theme,
) -> Vec<Span<'a>> {
    let Some(tokens) = tokens else {
        return vec![Span::styled(text, base)];
    };
    let mut result = Vec::new();
    let mut cursor = 0usize;
    for token in tokens {
        let start = char_to_byte(text, token.start).max(cursor).min(text.len());
        let end = char_to_byte(text, token.end).max(start).min(text.len());
        if cursor < start {
            result.push(Span::styled(&text[cursor..start], base));
        }
        if start < end {
            result.push(Span::styled(
                &text[start..end],
                on_line(theme.syntax[token.class.index()], base),
            ));
        }
        cursor = cursor.max(end);
    }
    if cursor < text.len() {
        result.push(Span::styled(&text[cursor..], base));
    }
    if result.is_empty() {
        result.push(Span::styled(text, base));
    }
    result
}

fn char_to_byte(value: &str, index: usize) -> usize {
    value
        .char_indices()
        .nth(index)
        .map_or(value.len(), |(offset, _)| offset)
}

fn render_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = app.theme();
    let message = app.message.as_deref().unwrap_or(
        "l related  h back  i code/context  x tests  I session  p parent  t tags  Space toggle+next  o Diffview  ? help  q quit",
    );
    let style = if app.message.is_some() {
        theme.error
    } else {
        theme.muted
    };
    frame.render_widget(
        Paragraph::new(clip_end(message, area.width as usize)).style(style),
        area,
    );
}

fn render_picker(frame: &mut Frame<'_>, app: &App) {
    let theme = app.theme();
    let area = centered(frame.area(), 64, 70);
    frame.render_widget(Clear, area);
    match app.picker.as_ref() {
        Some(Picker::Tags(picker)) => {
            let mut lines = vec![Line::from(vec![
                Span::styled("Search: ", theme.muted),
                Span::styled(&picker.query, theme.text),
            ])];
            let all_style = if picker.cursor == 0 {
                theme.selected
            } else {
                theme.text
            };
            lines.push(Line::styled("[ ] All tags", all_style));
            for (index, (tag, count)) in picker.visible().into_iter().enumerate() {
                let checked = if picker.pending.contains(tag) {
                    "x"
                } else {
                    " "
                };
                let style = if picker.cursor == index + 1 {
                    theme.selected
                } else {
                    theme.text
                };
                lines.push(Line::styled(format!("[{checked}] {tag} ({count})"), style));
            }
            frame.render_widget(
                Paragraph::new(lines).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(theme.focus)
                        .title("Tags · type to search · Space toggle · Enter apply · Esc cancel"),
                ),
                area,
            );
        }
        Some(Picker::Themes(picker)) => {
            let lines = picker
                .options
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    let marker = if name == &app.theme_name {
                        "●"
                    } else {
                        "○"
                    };
                    Line::styled(
                        format!("{marker} {name}"),
                        if index == picker.cursor {
                            theme.selected
                        } else {
                            theme.text
                        },
                    )
                })
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(lines).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(theme.focus)
                        .title("Theme · Enter apply · Esc cancel"),
                ),
                area,
            );
        }
        None => {}
    }
}

fn render_help(frame: &mut Frame<'_>, app: &App) {
    let theme = app.theme();
    let area = centered(frame.area(), 76, 76);
    frame.render_widget(Clear, area);
    let lines = [
        "j/k, arrows    move in focused list or preview",
        "Tab            switch list / preview focus",
        "[/]            scroll preview while list stays focused",
        "i              toggle Code / Context for selected item",
        "x              toggle linked Tests with source preview",
        "j/k in Tests   choose test; [/] scroll its preview",
        "I              toggle session context overview",
        "p              toggle review unit / full parent hunk",
        "l/Right, g     show related changed hunks",
        "h/Left/Esc     back; g returns to queue",
        "t              choose multiple tag filters",
        "T              choose a built-in or configured theme",
        "u/r/a          unreviewed / reviewed / all statuses",
        "Space          toggle reviewed and open next queue item",
        "o/Enter        open exact hunk in Diffview",
        "c              add comment in editor",
        "e              export comments to review.md",
        "v              inspect relation evidence/source context",
        "+/-            adjust priority manually",
        "q, Ctrl+C      quit safely",
        "",
        "Press Esc or ? to close help.",
    ];
    frame.render_widget(
        Paragraph::new(lines.into_iter().map(Line::raw).collect::<Vec<_>>())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme.focus)
                    .title("Review keys"),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_evidence(frame: &mut Frame<'_>, app: &App) {
    let theme = app.theme();
    let area = centered(frame.area(), 88, 80);
    frame.render_widget(Clear, area);
    let center = app
        .frame
        .center
        .as_deref()
        .or_else(|| app.selected_id())
        .unwrap_or("");
    let mut lines = Vec::new();
    let related_items = app.related();
    if let Some(related) = related_items.get(app.frame.cursor) {
        lines.push(Line::styled(
            format!(
                "{} {} · {}% · via {}",
                related.direction_marker(),
                related
                    .categories
                    .iter()
                    .map(relation_label)
                    .collect::<Vec<_>>()
                    .join(", "),
                related.confidence,
                related
                    .via_symbols
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            theme.usage,
        ));
        for id in &related.evidence_ids {
            lines.push(Line::styled(format!("  edge {id}"), theme.muted));
        }
    }
    let contexts = app.graph.source_context(center);
    if !contexts.is_empty() {
        lines.push(Line::styled("Unchanged source context", theme.definition));
        for context in contexts.iter().take(area.height.saturating_sub(6) as usize) {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "{} {:<10} ",
                        context.direction.marker(),
                        relation_label(&context.relation)
                    ),
                    theme.usage,
                ),
                Span::raw(format!(
                    "{} · {}:{} · {} · {}% · edge {} · node {}",
                    context.symbol,
                    tail_path(&context.path, 36),
                    context.line,
                    context.side,
                    context.confidence,
                    context.evidence_id,
                    context.node_id
                )),
            ]));
            match app.source_state(&context.node_id) {
                Some(SourceState::Loading) => {
                    lines.push(Line::styled("       loading excerpt…", theme.muted));
                }
                Some(SourceState::Ready {
                    lines: excerpt,
                    truncated,
                }) => {
                    for source in excerpt.iter().take(10) {
                        lines.push(Line::styled(format!("       {source}"), theme.text));
                    }
                    if *truncated {
                        lines.push(Line::styled(
                            "       … source limited to 64 KiB",
                            theme.warning,
                        ));
                    }
                }
                Some(SourceState::Error(error)) => {
                    lines.push(Line::styled(format!("       {error}"), theme.error));
                }
                None if context.source_file.is_none() => {
                    lines.push(Line::styled(
                        "       captured source unavailable",
                        theme.warning,
                    ));
                }
                None => {}
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::styled(
            "No indexed relation evidence for this hunk. Analysis coverage may be partial.",
            theme.warning,
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.focus)
                .title("Evidence · v close"),
        ),
        area,
    );
}

fn centered(area: Rect, max_width: u16, percent_height: u16) -> Rect {
    let width = area.width.min(max_width);
    let [horizontal] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let height = horizontal.height.saturating_mul(percent_height) / 100;
    let [result] = Layout::vertical([Constraint::Length(height.max(6))])
        .flex(Flex::Center)
        .areas(horizontal);
    result
}

fn compact_location(path: Option<&str>, line: Option<u32>, width: usize) -> String {
    let path = path.unwrap_or("(general)");
    let line = line.map_or(String::new(), |line| format!(":{line}"));
    let remaining = width.saturating_sub(display_width(&line));
    format!("{}{}", tail_path(path, remaining), line)
}

fn tail_path(value: &str, width: usize) -> String {
    let basename = Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(value);
    if display_width(basename) <= width {
        return basename.to_owned();
    }
    clip_start(value, width)
}

pub(super) fn clip_end(value: &str, width: usize) -> String {
    if display_width(value) <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut cells = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if cells + character_width + 1 > width {
            break;
        }
        result.push(character);
        cells += character_width;
    }
    result.push('…');
    result
}

pub(super) fn clip_start(value: &str, width: usize) -> String {
    if display_width(value) <= width {
        return value.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut tail = String::new();
    let mut cells = 1;
    for character in value.chars().rev() {
        let character_width = character.width().unwrap_or(0);
        if cells + character_width > width {
            break;
        }
        tail.insert(0, character);
        cells += character_width;
    }
    format!("…{tail}")
}

fn display_width(value: &str) -> usize {
    value
        .chars()
        .map(|character| character.width().unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_uses_display_cells_and_keeps_path_tail() {
        assert_eq!(clip_end("change 🔐 behavior", 10), "change 🔐…");
        assert_eq!(
            clip_start("frontend/components/editor.test.tsx", 18),
            "…s/editor.test.tsx"
        );
    }

    #[test]
    fn syntax_spans_preserve_unicode_text_and_theme_role() {
        let theme = super::super::theme::Theme::resolve(
            "night-owl",
            &std::collections::BTreeMap::new(),
            false,
        );
        let spans = styled_syntax(
            "const 😀 = 1",
            Some(&[crate::syntax::SyntaxSpan {
                start: 0,
                end: 5,
                class: crate::syntax::SyntaxClass::Keyword,
            }]),
            theme.addition_line,
            &theme,
        );
        assert_eq!(
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "const 😀 = 1"
        );
        assert_eq!(
            spans[0].style.fg,
            theme.syntax[crate::syntax::SyntaxClass::Keyword.index()].fg
        );
        assert_eq!(spans[0].style.bg, theme.addition_line.bg);
    }
}

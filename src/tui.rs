use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, IsTerminal, Stdout};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::error::{AppError, Result};
use crate::graph::ChangeGraph;
use crate::progress::{ReviewProgress, ReviewStatus};
use crate::ranking::{Queue, QueueItem};
use crate::settings::TuiSettings;
use crate::snapshot::Snapshot;

mod context;
mod graph;
mod preview;
mod render;
pub(crate) mod theme;

use context::ReviewContext;
use graph::{Index as GraphIndex, LinkedTest, RelatedHunk};
use preview::{SourceLoader, SourceState, Store as PreviewStore, SyntaxLoader, SyntaxState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Focus {
    List,
    Preview,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum PaneMode {
    #[default]
    Code,
    Context,
    Tests,
    SessionContext,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ViewFrame {
    pub(super) center: Option<String>,
    pub(super) cursor: usize,
    pub(super) list_offset: usize,
    pub(super) preview_offset: usize,
    pub(super) context_offset: usize,
    pub(super) test_cursor: usize,
    pub(super) test_preview_offset: usize,
    pub(super) pane_mode: PaneMode,
    pub(super) full_parent: bool,
    pub(super) focus: Focus,
}

impl Default for ViewFrame {
    fn default() -> Self {
        Self {
            center: None,
            cursor: 0,
            list_offset: 0,
            preview_offset: 0,
            context_offset: 0,
            test_cursor: 0,
            test_preview_offset: 0,
            pane_mode: PaneMode::Code,
            full_parent: false,
            focus: Focus::List,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct TagPicker {
    pub(super) query: String,
    pub(super) cursor: usize,
    pub(super) pending: BTreeSet<String>,
    pub(super) options: Vec<(String, usize)>,
    original_id: Option<String>,
}

impl TagPicker {
    pub(super) fn visible(&self) -> Vec<&(String, usize)> {
        let needle = self.query.to_lowercase();
        self.options
            .iter()
            .filter(|(tag, _)| needle.is_empty() || tag.to_lowercase().contains(&needle))
            .collect()
    }
}

#[derive(Clone, Debug)]
pub(super) struct ThemePicker {
    pub(super) cursor: usize,
    pub(super) options: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) enum Picker {
    Tags(TagPicker),
    Themes(ThemePicker),
}

struct App {
    pub(super) queue: Queue,
    pub(super) progress: ReviewProgress,
    pub(super) frame: ViewFrame,
    pub(super) history: Vec<ViewFrame>,
    pub(super) tag_filters: BTreeSet<String>,
    pub(super) status_filter: Option<ReviewStatus>,
    pub(super) manual_scores: BTreeMap<String, u8>,
    pub(super) message: Option<String>,
    pub(super) picker: Option<Picker>,
    pub(super) help: bool,
    pub(super) evidence: bool,
    pub(super) theme_settings: TuiSettings,
    pub(super) theme_name: String,
    pub(super) no_color: bool,
    pub(super) graph: GraphIndex,
    pub(super) previews: PreviewStore,
    pub(super) review_context: ReviewContext,
    pub(super) source_loader: SourceLoader,
    pub(super) syntax_loader: SyntaxLoader,
    source_generation: u64,
    syntax_generation: u64,
}

pub enum Action<'a> {
    Open(&'a str),
    Comment(&'a str),
    Export,
}

pub struct RunOptions<'a> {
    pub progress_path: &'a Path,
    pub theme: TuiSettings,
    pub no_color: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyResult {
    Continue,
    ProgressChanged,
    Exit,
}

impl App {
    pub fn new(
        queue: Queue,
        graph: &ChangeGraph,
        snapshot: &Snapshot,
        progress: ReviewProgress,
        theme_settings: TuiSettings,
        no_color: bool,
    ) -> Self {
        let cursor = progress
            .selected
            .as_ref()
            .and_then(|selected| {
                queue
                    .items
                    .iter()
                    .position(|item| &item.node_id == selected)
            })
            .unwrap_or(0);
        let review_context = ReviewContext::load(snapshot, graph);
        let review_units = crate::review_units::load(snapshot).ok().flatten();
        let previews = PreviewStore::from_snapshot(snapshot, review_units.as_ref(), graph);
        let graph = GraphIndex::new(graph, &queue, snapshot);
        let theme_name = theme_settings.theme.clone();
        let mut app = Self {
            queue,
            progress,
            frame: ViewFrame {
                cursor,
                ..ViewFrame::default()
            },
            history: Vec::new(),
            tag_filters: BTreeSet::new(),
            status_filter: None,
            manual_scores: BTreeMap::new(),
            message: None,
            picker: None,
            help: false,
            evidence: false,
            theme_settings,
            theme_name,
            no_color,
            graph,
            previews,
            review_context,
            source_loader: SourceLoader::new(),
            syntax_loader: SyntaxLoader::new(),
            source_generation: 0,
            syntax_generation: 0,
        };
        app.refresh_syntax();
        app
    }

    pub(super) fn is_queue(&self) -> bool {
        self.frame.center.is_none()
    }

    pub(super) fn visible_queue_indices(&self) -> Vec<usize> {
        self.queue
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                (self.tag_filters.is_empty()
                    || item.tags.iter().any(|tag| self.tag_filters.contains(tag)))
                    && self
                        .status_filter
                        .is_none_or(|status| self.progress.status(&item.node_id) == status)
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub(super) fn visible_queue(&self) -> Vec<&QueueItem> {
        self.visible_queue_indices()
            .into_iter()
            .map(|index| &self.queue.items[index])
            .collect()
    }

    pub(super) fn related(&self) -> Vec<&RelatedHunk> {
        self.frame
            .center
            .as_deref()
            .map(|center| self.graph.related(center))
            .unwrap_or(&[])
            .iter()
            .filter(|item| {
                !item
                    .categories
                    .contains(&crate::graph::EdgeKind::RuntimeTest)
                    && (self.tag_filters.is_empty()
                        || item.tags.iter().any(|tag| self.tag_filters.contains(tag)))
                    && self
                        .status_filter
                        .is_none_or(|status| self.progress.status(&item.node_id) == status)
            })
            .collect()
    }

    pub(super) fn linked_tests(&self) -> &[LinkedTest] {
        self.selected_id()
            .map(|id| self.graph.linked_tests(id))
            .unwrap_or(&[])
    }

    pub(super) fn selected_test(&self) -> Option<&LinkedTest> {
        self.linked_tests().get(self.frame.test_cursor)
    }

    pub(super) fn selected_id(&self) -> Option<&str> {
        if self.is_queue() {
            self.visible_queue()
                .get(self.frame.cursor)
                .map(|item| item.node_id.as_str())
        } else {
            self.related()
                .get(self.frame.cursor)
                .map(|item| item.node_id.as_str())
                .or(self.frame.center.as_deref())
        }
    }

    pub(super) fn selected_preview(&self) -> Option<&preview::Preview> {
        self.selected_preview_id()
            .and_then(|id| self.previews.get(id))
    }

    pub(super) fn selected_preview_id(&self) -> Option<&str> {
        let id = self.selected_id()?;
        if self.frame.full_parent {
            self.graph
                .node(id)
                .filter(|node| node.kind == crate::graph::NodeKind::ReviewUnit)
                .and_then(|node| node.hunk_ids.first())
                .map(String::as_str)
                .or(Some(id))
        } else {
            Some(id)
        }
    }

    pub(super) fn test_preview(&self) -> Option<&preview::Preview> {
        self.selected_test()
            .and_then(|test| self.previews.get(&test.node_id))
    }

    fn active_preview_id(&self) -> Option<&str> {
        if self.frame.pane_mode == PaneMode::Tests {
            self.selected_test().map(|test| test.node_id.as_str())
        } else {
            self.selected_preview_id()
        }
    }

    pub(super) fn context_lines(&self) -> Vec<String> {
        if self.frame.pane_mode == PaneMode::SessionContext {
            return self.review_context.session_lines();
        }
        let Some(id) = self.selected_id() else {
            return Vec::new();
        };
        self.review_context.item_lines(id, self.graph.node(id))
    }

    pub(super) fn annotation_for(&self, side: crate::graph::SourceSide, line: u32) -> Option<&str> {
        let id = self.selected_id()?;
        self.review_context
            .annotation_for(id, self.graph.node(id), side, line)
    }

    pub(super) fn current_center_title(&self) -> Option<&str> {
        let center = self.frame.center.as_deref()?;
        self.queue
            .items
            .iter()
            .find(|item| item.node_id == center)
            .map(|item| item.title.as_str())
    }

    pub(super) fn reviewed_count(&self) -> usize {
        self.queue
            .items
            .iter()
            .filter(|item| self.progress.status(&item.node_id) == ReviewStatus::Reviewed)
            .count()
    }

    pub(super) fn theme(&self) -> theme::Theme {
        theme::Theme::resolve(&self.theme_name, &self.theme_settings.themes, self.no_color)
    }

    fn list_len(&self) -> usize {
        if self.is_queue() {
            self.visible_queue_indices().len()
        } else {
            self.related().len()
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let length = self.list_len();
        if length == 0 {
            self.frame.cursor = 0;
            return;
        }
        self.frame.cursor = self
            .frame
            .cursor
            .saturating_add_signed(delta)
            .min(length - 1);
        self.frame.preview_offset = 0;
        self.frame.context_offset = 0;
        self.frame.test_cursor = 0;
        self.frame.test_preview_offset = 0;
        self.frame.full_parent = false;
        self.message = None;
        self.refresh_source_context();
        self.refresh_syntax();
    }

    fn refresh_source_context(&mut self) {
        if !self.evidence {
            return;
        }
        self.source_generation = self.source_generation.wrapping_add(1);
        let center = self
            .frame
            .center
            .as_deref()
            .or_else(|| self.selected_id())
            .unwrap_or("")
            .to_owned();
        let contexts = self.graph.source_context(&center).to_vec();
        for context in contexts {
            if let Some(path) = context.source_file {
                self.source_loader.request(
                    self.source_generation,
                    context.node_id,
                    path,
                    context.line,
                );
            }
        }
    }

    fn poll_source_context(&mut self) {
        self.source_loader.poll(self.source_generation);
        self.syntax_loader.poll(self.syntax_generation);
        if self.theme_settings.syntax_highlighting {
            let selected = self.active_preview_id().map(str::to_owned);
            if let Some(id) = selected
                && self.syntax_loader.state(&id).is_none()
                && let Some(preview) = self.previews.get(&id)
            {
                self.syntax_loader
                    .request(self.syntax_generation, id, preview);
            }
        }
    }

    fn refresh_syntax(&mut self) {
        if !self.theme_settings.syntax_highlighting {
            return;
        }
        self.syntax_generation = self.syntax_generation.wrapping_add(1);
        let Some(id) = self.active_preview_id().map(str::to_owned) else {
            return;
        };
        if let Some(preview) = self.previews.get(&id) {
            self.syntax_loader
                .request(self.syntax_generation, id, preview);
        }
    }

    pub(super) fn syntax_state(&self, id: &str) -> Option<&SyntaxState> {
        self.syntax_loader.state(id)
    }

    pub(super) fn source_state(&self, key: &str) -> Option<&SourceState> {
        self.source_loader.state(key)
    }

    fn enter_related(&mut self) {
        let Some(selected) = self.selected_id().map(str::to_owned) else {
            return;
        };
        let previous = self.frame.clone();
        self.history.push(previous);
        self.frame = ViewFrame {
            center: Some(selected),
            ..ViewFrame::default()
        };
        self.message = None;
        self.refresh_source_context();
        self.refresh_syntax();
    }

    fn move_test_selection(&mut self, delta: isize) {
        let length = self.linked_tests().len();
        if length == 0 {
            self.frame.test_cursor = 0;
            return;
        }
        self.frame.test_cursor = self
            .frame
            .test_cursor
            .saturating_add_signed(delta)
            .min(length - 1);
        self.frame.test_preview_offset = 0;
        self.message = None;
        self.refresh_syntax();
    }

    fn back(&mut self) {
        if let Some(previous) = self.history.pop() {
            self.frame = previous;
        } else {
            self.frame = ViewFrame::default();
        }
        self.message = None;
        self.refresh_source_context();
        self.refresh_syntax();
    }

    fn return_queue(&mut self) {
        if let Some(root) = self.history.first().cloned() {
            self.frame = root;
        } else {
            self.frame = ViewFrame::default();
        }
        self.history.clear();
        self.message = None;
        self.refresh_source_context();
        self.refresh_syntax();
    }

    fn adjust_score(&mut self, delta: i16) {
        let Some(id) = self.selected_id().map(str::to_owned) else {
            return;
        };
        if let Some(item) = self.queue.items.iter_mut().find(|item| item.node_id == id) {
            let next = (item.score.unwrap_or(0) as i16 + delta).clamp(0, 100) as u8;
            item.score = Some(next);
            item.assessed = true;
            self.manual_scores.insert(id, next);
        }
    }

    fn open_tag_picker(&mut self) {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for item in self.queue.items.iter().filter(|item| {
            self.status_filter
                .is_none_or(|status| self.progress.status(&item.node_id) == status)
        }) {
            for tag in &item.tags {
                *counts.entry(tag.clone()).or_default() += 1;
            }
        }
        self.picker = Some(Picker::Tags(TagPicker {
            query: String::new(),
            cursor: 0,
            pending: self.tag_filters.clone(),
            options: counts.into_iter().collect(),
            original_id: self.selected_id().map(str::to_owned),
        }));
    }

    fn open_theme_picker(&mut self) {
        let options = theme::available(&self.theme_settings);
        let cursor = options
            .iter()
            .position(|name| name == &self.theme_name)
            .unwrap_or(0);
        self.picker = Some(Picker::Themes(ThemePicker { cursor, options }));
    }

    fn handle_picker_key(&mut self, key: KeyEvent) {
        let mut apply_tags: Option<(BTreeSet<String>, Option<String>)> = None;
        let mut apply_theme: Option<String> = None;
        let mut close = false;
        match self.picker.as_mut() {
            Some(Picker::Tags(picker)) => match key.code {
                KeyCode::Esc => close = true,
                KeyCode::Up => picker.cursor = picker.cursor.saturating_sub(1),
                KeyCode::Down => picker.cursor = (picker.cursor + 1).min(picker.visible().len()),
                KeyCode::Char(' ') => {
                    if picker.cursor == 0 {
                        picker.pending.clear();
                    } else if let Some((tag, _)) = picker.visible().get(picker.cursor - 1) {
                        let tag = (*tag).clone();
                        if !picker.pending.remove(&tag) {
                            picker.pending.insert(tag);
                        }
                    }
                }
                KeyCode::Enter => {
                    apply_tags = Some((picker.pending.clone(), picker.original_id.clone()));
                    close = true;
                }
                KeyCode::Backspace => {
                    picker.query.pop();
                    picker.cursor = 0;
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    picker.query.push(character);
                    picker.cursor = 0;
                }
                _ => {}
            },
            Some(Picker::Themes(picker)) => match key.code {
                KeyCode::Esc => close = true,
                KeyCode::Up | KeyCode::Char('k') => picker.cursor = picker.cursor.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    picker.cursor = (picker.cursor + 1).min(picker.options.len().saturating_sub(1))
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    apply_theme = picker.options.get(picker.cursor).cloned();
                    close = true;
                }
                _ => {}
            },
            None => {}
        }
        if let Some((filters, original_id)) = apply_tags {
            self.tag_filters = filters;
            self.frame.cursor = if self.is_queue() {
                let visible = self.visible_queue();
                original_id
                    .as_deref()
                    .and_then(|id| visible.iter().position(|item| item.node_id == id))
                    .unwrap_or(0)
            } else {
                let visible = self.related();
                original_id
                    .as_deref()
                    .and_then(|id| visible.iter().position(|item| item.node_id == id))
                    .unwrap_or(0)
            };
            self.frame.list_offset = 0;
            self.frame.preview_offset = 0;
        }
        if let Some(theme) = apply_theme {
            self.theme_name = theme;
        }
        if close {
            self.picker = None;
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> KeyResult {
        if self.picker.is_some() {
            self.handle_picker_key(key);
            return KeyResult::Continue;
        }
        if self.help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.help = false;
            }
            return KeyResult::Continue;
        }
        match key.code {
            KeyCode::Char('q') => return KeyResult::Exit,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('t') => self.open_tag_picker(),
            KeyCode::Char('T') => self.open_theme_picker(),
            KeyCode::Char('v') => {
                self.evidence = !self.evidence;
                self.refresh_source_context();
            }
            KeyCode::Char('i') => {
                self.frame.pane_mode = match self.frame.pane_mode {
                    PaneMode::Code => PaneMode::Context,
                    PaneMode::Context | PaneMode::Tests | PaneMode::SessionContext => {
                        PaneMode::Code
                    }
                };
                self.refresh_syntax();
            }
            KeyCode::Char('x') => {
                self.frame.pane_mode = if self.frame.pane_mode == PaneMode::Tests {
                    PaneMode::Code
                } else {
                    PaneMode::Tests
                };
                self.frame.test_cursor = self
                    .frame
                    .test_cursor
                    .min(self.linked_tests().len().saturating_sub(1));
                self.refresh_syntax();
            }
            KeyCode::Char('I') => {
                self.frame.pane_mode = if self.frame.pane_mode == PaneMode::SessionContext {
                    PaneMode::Code
                } else {
                    PaneMode::SessionContext
                };
            }
            KeyCode::Char('p') => {
                self.frame.full_parent = !self.frame.full_parent;
                self.refresh_syntax();
            }
            KeyCode::Tab => {
                self.frame.focus = match self.frame.focus {
                    Focus::List => Focus::Preview,
                    Focus::Preview => Focus::List,
                }
            }
            KeyCode::Char('[') => match self.frame.pane_mode {
                PaneMode::Code => {
                    self.frame.preview_offset = self.frame.preview_offset.saturating_sub(5)
                }
                PaneMode::Tests => {
                    self.frame.test_preview_offset =
                        self.frame.test_preview_offset.saturating_sub(5)
                }
                _ => self.frame.context_offset = self.frame.context_offset.saturating_sub(5),
            },
            KeyCode::Char(']') => match self.frame.pane_mode {
                PaneMode::Code => self.frame.preview_offset += 5,
                PaneMode::Tests => self.frame.test_preview_offset += 5,
                _ => self.frame.context_offset += 5,
            },
            KeyCode::PageUp => match self.frame.focus {
                Focus::List => self.move_selection(-10),
                Focus::Preview => match self.frame.pane_mode {
                    PaneMode::Code => {
                        self.frame.preview_offset = self.frame.preview_offset.saturating_sub(10)
                    }
                    PaneMode::Tests => {
                        self.frame.test_preview_offset =
                            self.frame.test_preview_offset.saturating_sub(10)
                    }
                    _ => self.frame.context_offset = self.frame.context_offset.saturating_sub(10),
                },
            },
            KeyCode::PageDown => match self.frame.focus {
                Focus::List => self.move_selection(10),
                Focus::Preview => match self.frame.pane_mode {
                    PaneMode::Code => self.frame.preview_offset += 10,
                    PaneMode::Tests => self.frame.test_preview_offset += 10,
                    _ => self.frame.context_offset += 10,
                },
            },
            KeyCode::Down | KeyCode::Char('j') => match self.frame.focus {
                Focus::List => self.move_selection(1),
                Focus::Preview => match self.frame.pane_mode {
                    PaneMode::Code => self.frame.preview_offset += 1,
                    PaneMode::Tests => self.move_test_selection(1),
                    _ => self.frame.context_offset += 1,
                },
            },
            KeyCode::Up | KeyCode::Char('k') => match self.frame.focus {
                Focus::List => self.move_selection(-1),
                Focus::Preview => match self.frame.pane_mode {
                    PaneMode::Code => {
                        self.frame.preview_offset = self.frame.preview_offset.saturating_sub(1)
                    }
                    PaneMode::Tests => self.move_test_selection(-1),
                    _ => self.frame.context_offset = self.frame.context_offset.saturating_sub(1),
                },
            },
            KeyCode::Right | KeyCode::Char('l') => self.enter_related(),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Esc if !self.is_queue() => self.back(),
            KeyCode::Char('g') if self.is_queue() => self.enter_related(),
            KeyCode::Char('g') => self.return_queue(),
            KeyCode::Char(' ') => {
                if let Some(id) = self.selected_id().map(str::to_owned) {
                    let queue_cursor = self.is_queue().then_some(self.frame.cursor);
                    let next = if self.progress.status(&id) == ReviewStatus::Reviewed {
                        ReviewStatus::Unreviewed
                    } else {
                        ReviewStatus::Reviewed
                    };
                    self.progress.set_status(id.clone(), next);
                    if let Some(cursor) = queue_cursor {
                        let current = self
                            .visible_queue()
                            .iter()
                            .position(|item| item.node_id == id);
                        self.frame.cursor = current.unwrap_or(cursor);
                        self.move_selection(if current.is_some() { 1 } else { 0 });
                    }
                    return KeyResult::ProgressChanged;
                }
            }
            KeyCode::Char('u') => {
                self.status_filter = Some(ReviewStatus::Unreviewed);
                self.frame.cursor = 0;
                self.frame.list_offset = 0;
            }
            KeyCode::Char('r') => {
                self.status_filter = Some(ReviewStatus::Reviewed);
                self.frame.cursor = 0;
                self.frame.list_offset = 0;
            }
            KeyCode::Char('a') => {
                self.status_filter = None;
                self.frame.cursor = 0;
                self.frame.list_offset = 0;
            }
            KeyCode::Char('+') | KeyCode::Char('=') => self.adjust_score(5),
            KeyCode::Char('-') => self.adjust_score(-5),
            _ => {}
        }
        self.frame.cursor = self.frame.cursor.min(self.list_len().saturating_sub(1));
        KeyResult::Continue
    }
}

pub fn run(
    queue: Queue,
    graph: &ChangeGraph,
    snapshot: &Snapshot,
    progress: ReviewProgress,
    options: RunOptions<'_>,
    mut action: impl FnMut(Action<'_>) -> Result<()>,
) -> Result<BTreeMap<String, u8>> {
    ensure_interactive_terminal()?;
    let interrupted = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGHUP] {
        signal_hook::flag::register(signal, Arc::clone(&interrupted))?;
    }
    let mut terminal = TerminalGuard::new()?;
    let mut app = App::new(
        queue,
        graph,
        snapshot,
        progress,
        options.theme,
        options.no_color,
    );
    loop {
        if interrupted.load(Ordering::Relaxed) {
            break;
        }
        terminal
            .terminal
            .draw(|frame| render::draw(frame, &mut app))?;
        app.poll_source_context();
        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                break;
            }
            if app.picker.is_none()
                && !app.help
                && matches!(
                    key.code,
                    KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('c')
                )
            {
                if let Some(id) = app.selected_id().map(str::to_owned) {
                    terminal.suspend()?;
                    let requested = if key.code == KeyCode::Char('c') {
                        Action::Comment(&id)
                    } else {
                        Action::Open(&id)
                    };
                    app.message = action(requested).err().map(|error| error.to_string());
                    terminal.resume()?;
                }
                continue;
            }
            if app.picker.is_none() && !app.help && key.code == KeyCode::Char('e') {
                terminal.suspend()?;
                app.message = action(Action::Export).err().map(|error| error.to_string());
                terminal.resume()?;
                continue;
            }
            match app.handle_key(key) {
                KeyResult::Exit => break,
                KeyResult::ProgressChanged => {
                    let revision = app.progress.revision;
                    app.progress.selected = app.selected_id().map(str::to_owned);
                    app.progress.save_checked(options.progress_path, revision)?;
                }
                KeyResult::Continue => {}
            }
        }
    }
    Ok(app.manual_scores)
}

fn ensure_interactive_terminal() -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(invalid(
            "tui_requires_terminal",
            "TUI requires interactive stdin and stdout; use `lgr graph queue SESSION` and `lgr graph hunks SESSION HUNK_ID` in scripts",
        ));
    }
    if std::env::var("TERM").is_ok_and(|term| term == "dumb") {
        return Err(invalid(
            "unsupported_terminal",
            "TERM=dumb cannot run the TUI; use `lgr graph queue SESSION` and `lgr graph hunks SESSION HUNK_ID`",
        ));
    }
    Ok(())
}

fn invalid(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::InvalidInput {
        code,
        message: message.into(),
    }
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        Ok(Self {
            terminal: Terminal::new(CrosstermBackend::new(stdout))?,
        })
    }

    fn suspend(&mut self) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
        Ok(())
    }

    fn resume(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(self.terminal.backend_mut(), EnterAlternateScreen)?;
        self.terminal.clear()?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EdgeKind, GraphEdge, GraphNode, SourceLocation, SourceSide};
    use crate::model::{GraphRevisionId, SnapshotId};
    use crate::position::{Position, TextRange};
    use crate::ranking::{QueueItem, RankingMetrics, TitleSource};
    use crate::snapshot::{FileChange, GitPath, Hunk, SnapshotInput};
    use chrono::Utc;
    use ratatui::backend::TestBackend;

    fn location(path: &str, start: u32, end: u32) -> SourceLocation {
        SourceLocation {
            side: SourceSide::Right,
            path: GitPath::from_bytes(path.as_bytes().to_vec()),
            range: TextRange {
                start: Position {
                    line: start,
                    character: 0,
                },
                end: Position {
                    line: end,
                    character: 0,
                },
            },
        }
    }

    fn queue_item(id: &str, title: &str, path: &str, tag: &str, score: u8) -> QueueItem {
        QueueItem {
            node_id: id.into(),
            kind: crate::graph::NodeKind::Hunk,
            name: "@@ raw header".into(),
            title: title.into(),
            title_source: TitleSource::Model,
            path: Some(path.into()),
            score: Some(score),
            tags: vec![tag.into()],
            rationale: Some("Important behavior".into()),
            confidence: Some(1.0),
            inherited_from: None,
            assessed: true,
        }
    }

    fn fixture() -> (Queue, ChangeGraph, Snapshot) {
        let h1 = GraphNode {
            id: "h1".into(),
            kind: crate::graph::NodeKind::Hunk,
            name: "@@ raw one".into(),
            symbol_kind: None,
            changed: true,
            locations: vec![location("src/auth.ts", 9, 13)],
            selection_range: None,
            hunk_ids: vec!["h1".into()],
        };
        let symbol = GraphNode {
            id: "symbol".into(),
            kind: crate::graph::NodeKind::Symbol,
            name: "authenticate".into(),
            symbol_kind: Some("function".into()),
            changed: true,
            locations: vec![location("src/auth.ts", 4, 20)],
            selection_range: None,
            hunk_ids: vec!["h1".into()],
        };
        let h2 = GraphNode {
            id: "h2".into(),
            kind: crate::graph::NodeKind::Hunk,
            name: "@@ raw two".into(),
            symbol_kind: None,
            changed: true,
            locations: vec![location("tests/auth.test.ts", 39, 43)],
            selection_range: None,
            hunk_ids: vec!["h2".into()],
        };
        let reference = GraphNode {
            id: "reference".into(),
            kind: crate::graph::NodeKind::Reference,
            name: "authenticate".into(),
            symbol_kind: None,
            changed: false,
            locations: vec![location("tests/auth.test.ts", 40, 40)],
            selection_range: None,
            hunk_ids: Vec::new(),
        };
        let graph = ChangeGraph {
            revision: GraphRevisionId::parse("grf_test").unwrap(),
            snapshot_id: "snp_test".into(),
            fingerprint: "fixture".into(),
            nodes: [h1, symbol, h2, reference]
                .into_iter()
                .map(|node| (node.id.clone(), node))
                .collect(),
            edges: vec![GraphEdge {
                id: "edge".into(),
                from: "symbol".into(),
                to: "reference".into(),
                kind: EdgeKind::TestReference,
                producer: "lsp".into(),
                evidence_kind: "resolved".into(),
                confidence: 1.0,
                location: None,
            }],
            coverage: Vec::new(),
            unfinished_frontier: Vec::new(),
        };
        let mut test_item = queue_item(
            "h2",
            "Cover expired tokens",
            "tests/auth.test.ts",
            "tests",
            88,
        );
        test_item.tags.push("behavior".into());
        let queue = Queue {
            graph_revision: "grf_test".into(),
            stale: false,
            fully_ranked: true,
            finalized: true,
            context_digest: None,
            metrics: RankingMetrics::default(),
            assessed_changes: 2,
            total_changes: 2,
            items: vec![
                queue_item("h1", "Reject expired tokens", "src/auth.ts", "behavior", 92),
                test_item,
            ],
        };
        let make_file = |path: &str, id: &str, start: u32| FileChange {
            status: "M".into(),
            old_path: Some(GitPath::from_bytes(path.as_bytes().to_vec())),
            new_path: Some(GitPath::from_bytes(path.as_bytes().to_vec())),
            old_mode: "100644".into(),
            new_mode: "100644".into(),
            old_object: "old".into(),
            new_object: "new".into(),
            before_blob: None,
            after_blob: None,
            binary: false,
            submodule: false,
            hunks: vec![Hunk {
                id: id.into(),
                old_start: start,
                old_count: 2,
                new_start: start,
                new_count: 2,
                header: format!("@@ -{start},2 +{start},2 @@"),
                patch: format!(
                    "@@ -{start},2 +{start},2 @@\n-old value\n+new value\n context one\n context two\n context three\n context four\n context five\n context six\n context seven\n context eight\n"
                ),
            }],
        };
        let snapshot = Snapshot {
            id: SnapshotId::parse("snp_test").unwrap(),
            repository: "/repo".into(),
            input: SnapshotInput::Uncommitted,
            original_base: "base".into(),
            original_head: "head".into(),
            comparison_base: "base".into(),
            before_commit: "base".into(),
            after_commit: "head".into(),
            captured_at: Utc::now(),
            source_fingerprint: "fixture".into(),
            files: vec![
                make_file("src/auth.ts", "h1", 10),
                make_file("tests/auth.test.ts", "h2", 40),
            ],
            storage_dir: "/snapshot".into(),
        };
        (queue, graph, snapshot)
    }

    fn app() -> App {
        let (queue, graph, snapshot) = fixture();
        App::new(
            queue,
            &graph,
            &snapshot,
            ReviewProgress::default(),
            TuiSettings::default(),
            true,
        )
    }

    #[test]
    fn follows_related_hunks_and_restores_complete_view_frame() {
        let mut app = app();
        app.tag_filters.insert("behavior".into());
        app.frame.preview_offset = 3;
        app.frame.list_offset = 2;
        app.handle_key(KeyEvent::from(KeyCode::Char('l')));
        assert_eq!(app.frame.center.as_deref(), Some("h1"));
        assert_eq!(app.selected_id(), Some("h2"));
        app.handle_key(KeyEvent::from(KeyCode::Char('l')));
        assert_eq!(app.frame.center.as_deref(), Some("h2"));
        app.handle_key(KeyEvent::from(KeyCode::Char('h')));
        assert_eq!(app.frame.center.as_deref(), Some("h1"));
        app.handle_key(KeyEvent::from(KeyCode::Char('h')));
        assert!(app.is_queue());
        assert_eq!(app.frame.preview_offset, 3);
        assert_eq!(app.frame.list_offset, 2);
        assert!(app.tag_filters.contains("behavior"));
        assert_eq!(app.reviewed_count(), 0);
    }

    #[test]
    fn space_marks_reviewed_and_advances_through_queue() {
        let mut app = app();
        assert_eq!(app.selected_id(), Some("h1"));

        assert_eq!(
            app.handle_key(KeyEvent::from(KeyCode::Char(' '))),
            KeyResult::ProgressChanged
        );
        assert_eq!(app.progress.status("h1"), ReviewStatus::Reviewed);
        assert_eq!(app.selected_id(), Some("h2"));

        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
        assert_eq!(app.progress.status("h2"), ReviewStatus::Reviewed);
        assert_eq!(app.reviewed_count(), 2);
        assert_eq!(app.selected_id(), Some("h2"));
    }

    #[test]
    fn code_context_and_session_modes_keep_independent_offsets_and_progress() {
        let mut app = app();
        let progress = app.progress.revision;
        app.frame.preview_offset = 4;
        app.handle_key(KeyEvent::from(KeyCode::Char('i')));
        assert_eq!(app.frame.pane_mode, PaneMode::Context);
        app.frame.focus = Focus::Preview;
        app.handle_key(KeyEvent::from(KeyCode::Down));
        assert_eq!(app.frame.context_offset, 1);
        app.handle_key(KeyEvent::from(KeyCode::Char('i')));
        assert_eq!(app.frame.pane_mode, PaneMode::Code);
        assert_eq!(app.frame.preview_offset, 4);
        app.handle_key(KeyEvent::from(KeyCode::Char('I')));
        assert_eq!(app.frame.pane_mode, PaneMode::SessionContext);
        app.handle_key(KeyEvent::from(KeyCode::Char('I')));
        assert_eq!(app.frame.pane_mode, PaneMode::Code);
        assert_eq!(app.progress.revision, progress);
    }

    #[test]
    fn tests_tab_lists_changed_tests_and_keeps_its_own_preview_state() {
        let mut app = app();
        assert_eq!(app.linked_tests().len(), 1);
        assert_eq!(app.linked_tests()[0].node_id, "h2");

        app.handle_key(KeyEvent::from(KeyCode::Char('x')));
        assert_eq!(app.frame.pane_mode, PaneMode::Tests);
        assert_eq!(
            app.test_preview().map(|preview| preview.path.as_str()),
            Some("tests/auth.test.ts")
        );
        app.frame.focus = Focus::Preview;
        app.handle_key(KeyEvent::from(KeyCode::Char(']')));
        assert_eq!(app.frame.test_preview_offset, 5);
        assert_eq!(app.frame.preview_offset, 0);

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| render::draw(frame, &mut app))
            .unwrap();
        let content = buffer_text(terminal.backend().buffer(), 100, 30);
        assert!(content.contains("Tests · 1 changed · 0 before · 0 after"));
        assert!(content.contains("Cover expired tokens"));
    }

    #[test]
    fn tag_picker_is_modal_and_applies_or_filters() {
        let mut app = app();
        app.handle_key(KeyEvent::from(KeyCode::Char('t')));
        assert!(matches!(app.picker, Some(Picker::Tags(_))));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
        assert_eq!(app.reviewed_count(), 0);
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert!(app.picker.is_none());
        assert_eq!(app.tag_filters.len(), 1);
        assert_eq!(app.visible_queue().len(), 1);
    }

    #[test]
    fn theme_picker_includes_builtins_and_custom_palettes() {
        let mut app = app();
        app.theme_settings.themes.insert(
            "custom".into(),
            crate::settings::ThemePalette {
                background: "#000000".into(),
                foreground: "#ffffff".into(),
                muted: "#777777".into(),
                border: "#888888".into(),
                focus: "#00ffff".into(),
                selection: "#005555".into(),
                important: "#ffff00".into(),
                test: "#00ff00".into(),
                usage: "#00ffff".into(),
                definition: "#ff00ff".into(),
                addition: "#00ff00".into(),
                deletion: "#ff0000".into(),
                warning: "#ffff00".into(),
                error: "#ff0000".into(),
                syntax: crate::settings::SyntaxPalette::default(),
            },
        );
        app.handle_key(KeyEvent::from(KeyCode::Char('T')));
        let Some(Picker::Themes(picker)) = &app.picker else {
            panic!("theme picker")
        };
        assert!(picker.options.contains(&"night-owl".into()));
        assert!(picker.options.contains(&"custom".into()));
    }

    #[test]
    fn responsive_render_keeps_semantic_title_and_patch_visible() {
        for (width, height) in [(80, 24), (100, 30), (120, 40)] {
            let mut app = app();
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|frame| render::draw(frame, &mut app))
                .unwrap();
            let content = buffer_text(terminal.backend().buffer(), width, height);
            assert!(content.contains("Reject expired tokens"));
            assert!(content.contains("old value"));
            assert!(content.contains("context six"));
            assert!(!content.contains("@@ raw one"));
        }
    }

    #[test]
    fn stateful_list_scrolls_to_the_hundredth_item() {
        let mut app = app();
        let template = app.queue.items[0].clone();
        app.queue.items = (0..100)
            .map(|index| QueueItem {
                node_id: format!("h{index}"),
                title: format!("Semantic change {index:03}"),
                ..template.clone()
            })
            .collect();
        app.queue.total_changes = 100;
        app.move_selection(99);
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render::draw(frame, &mut app))
            .unwrap();
        let content = buffer_text(terminal.backend().buffer(), 80, 24);
        assert!(content.contains("Semantic change 099"));
        assert!(app.frame.list_offset > 0);
    }

    fn buffer_text(buffer: &ratatui::buffer::Buffer, width: u16, height: u16) -> String {
        let mut output = String::new();
        for y in 0..height {
            for x in 0..width {
                output.push_str(buffer[(x, y)].symbol());
            }
            output.push('\n');
        }
        output
    }

    #[test]
    #[ignore = "acceptance benchmark"]
    fn benchmark_large_cached_navigation() {
        use std::time::Instant;

        let (queue, graph, mut snapshot) = large_fixture();
        let captured = tempfile::TempDir::new().unwrap();
        snapshot.storage_dir = captured.path().into();
        for side in ["before", "after"] {
            let source = snapshot.storage_dir.join(side).join("src/large.ts");
            std::fs::create_dir_all(source.parent().unwrap()).unwrap();
            std::fs::write(
                source,
                (0..1_100)
                    .map(|line| format!("export const value{line}: number = {line};\n"))
                    .collect::<String>(),
            )
            .unwrap();
        }

        let plain_settings = TuiSettings {
            syntax_highlighting: false,
            ..TuiSettings::default()
        };
        let plain_started = Instant::now();
        let mut plain_app = App::new(
            queue.clone(),
            &graph,
            &snapshot,
            ReviewProgress::default(),
            plain_settings,
            true,
        );
        let mut plain_terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        plain_terminal
            .draw(|frame| render::draw(frame, &mut plain_app))
            .unwrap();
        let plain_first_frame_ms = plain_started.elapsed().as_secs_f64() * 1_000.0;

        let highlight_started = Instant::now();
        let mut app = App::new(
            queue,
            &graph,
            &snapshot,
            ReviewProgress::default(),
            TuiSettings::default(),
            true,
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        while highlight_started.elapsed().as_secs_f64() < 1.0 {
            app.poll_source_context();
            let ready = app
                .selected_preview_id()
                .and_then(|id| app.syntax_state(id))
                .is_some_and(|state| !matches!(state, SyntaxState::Loading));
            if ready {
                break;
            }
            std::thread::yield_now();
        }
        terminal
            .draw(|frame| render::draw(frame, &mut app))
            .unwrap();
        let highlighted_first_frame_ms = highlight_started.elapsed().as_secs_f64() * 1_000.0;
        let warm_started = Instant::now();
        terminal
            .draw(|frame| render::draw(frame, &mut app))
            .unwrap();
        let warm_frame_ms = warm_started.elapsed().as_secs_f64() * 1_000.0;
        let mut timings = Vec::new();
        for index in 0..200 {
            if app.is_queue() {
                app.frame.cursor = index % 100;
            }
            let started = Instant::now();
            app.handle_key(KeyEvent::from(if app.is_queue() {
                KeyCode::Char('l')
            } else {
                KeyCode::Char('h')
            }));
            terminal
                .draw(|frame| render::draw(frame, &mut app))
                .unwrap();
            timings.push(started.elapsed().as_secs_f64() * 1_000.0);
        }
        timings.sort_by(f64::total_cmp);
        let median = timings[timings.len() / 2];
        let p95 = timings[timings.len() * 95 / 100];
        let peak_rss_kib = peak_rss_kib();
        eprintln!(
            "LGR_PERF host={}-{} nodes={} edges={} leaf_units=100 actions=200 plain_cold_ms={plain_first_frame_ms:.3} highlight_cold_ms={highlighted_first_frame_ms:.3} highlight_warm_ms={warm_frame_ms:.3} median_ms={median:.3} p95_ms={p95:.3} peak_rss_kib={peak_rss_kib} io=0 child_processes=0 cache=cold+warm",
            std::env::consts::OS,
            std::env::consts::ARCH,
            graph.nodes.len(),
            graph.edges.len(),
        );
        assert!(
            plain_first_frame_ms <= 1_000.0,
            "plain first frame was {plain_first_frame_ms:.3} ms"
        );
        assert!(
            highlighted_first_frame_ms <= 1_000.0,
            "highlighted first frame was {highlighted_first_frame_ms:.3} ms"
        );
        assert!(
            warm_frame_ms <= 100.0,
            "warm frame was {warm_frame_ms:.3} ms"
        );
        assert!(p95 <= 50.0, "cached action p95 was {p95:.3} ms");
    }

    fn peak_rss_kib() -> i64 {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
        if status != 0 {
            return 0;
        }
        let rss = unsafe { usage.assume_init() }.ru_maxrss;
        if cfg!(target_os = "macos") {
            rss / 1024
        } else {
            rss
        }
    }

    fn large_fixture() -> (Queue, ChangeGraph, Snapshot) {
        let mut nodes = BTreeMap::new();
        let mut edges = Vec::new();
        let mut items = Vec::new();
        let mut hunks = Vec::new();
        let path = "src/large.ts";
        for index in 0..100usize {
            let hunk_id = format!("h{index}");
            let symbol_id = format!("s{index}");
            let reference_id = format!("r{index}");
            let line = (index * 10) as u32;
            let target_line = (((index + 1) % 100) * 10) as u32;
            nodes.insert(
                hunk_id.clone(),
                GraphNode {
                    id: hunk_id.clone(),
                    kind: crate::graph::NodeKind::Hunk,
                    name: format!("@@ hunk {index}"),
                    symbol_kind: None,
                    changed: true,
                    locations: vec![location(path, line, line + 3)],
                    selection_range: None,
                    hunk_ids: vec![hunk_id.clone()],
                },
            );
            nodes.insert(
                symbol_id.clone(),
                GraphNode {
                    id: symbol_id.clone(),
                    kind: crate::graph::NodeKind::Symbol,
                    name: format!("function{index}"),
                    symbol_kind: Some("function".into()),
                    changed: true,
                    locations: vec![location(path, line, line + 5)],
                    selection_range: None,
                    hunk_ids: vec![hunk_id.clone()],
                },
            );
            nodes.insert(
                reference_id.clone(),
                GraphNode {
                    id: reference_id.clone(),
                    kind: crate::graph::NodeKind::Reference,
                    name: format!("function{}", (index + 1) % 100),
                    symbol_kind: None,
                    changed: false,
                    locations: vec![location(path, target_line + 1, target_line + 1)],
                    selection_range: None,
                    hunk_ids: Vec::new(),
                },
            );
            edges.push(GraphEdge {
                id: format!("e{index}"),
                from: symbol_id,
                to: reference_id,
                kind: EdgeKind::Calls,
                producer: "fixture".into(),
                evidence_kind: "resolved".into(),
                confidence: 1.0,
                location: None,
            });
            items.push(queue_item(
                &hunk_id,
                &format!("Change semantic behavior {index:03}"),
                path,
                "behavior",
                100 - index as u8,
            ));
            hunks.push(Hunk {
                id: hunk_id,
                old_start: line + 1,
                old_count: 2,
                new_start: line + 1,
                new_count: 2,
                header: format!("@@ -{},2 +{},2 @@", line + 1, line + 1),
                patch: format!(
                    "@@ -{},2 +{},2 @@\n-old {index}\n+new {index}\n context\n",
                    line + 1,
                    line + 1
                ),
            });
        }
        for index in 100..9_800usize {
            let id = format!("r{index}");
            nodes.insert(
                id.clone(),
                GraphNode {
                    id: id.clone(),
                    kind: crate::graph::NodeKind::Reference,
                    name: format!("external{index}"),
                    symbol_kind: None,
                    changed: false,
                    locations: vec![location("src/unchanged.ts", index as u32, index as u32)],
                    selection_range: None,
                    hunk_ids: Vec::new(),
                },
            );
            edges.push(GraphEdge {
                id: format!("e{index}"),
                from: "s0".into(),
                to: id,
                kind: EdgeKind::References,
                producer: "fixture".into(),
                evidence_kind: "resolved".into(),
                confidence: 1.0,
                location: None,
            });
        }
        for index in 9_800..10_000usize {
            edges.push(GraphEdge {
                id: format!("e{index}"),
                from: format!("s{}", index % 100),
                to: format!("r{}", index % 100),
                kind: EdgeKind::Calls,
                producer: "fixture".into(),
                evidence_kind: "resolved".into(),
                confidence: 1.0,
                location: None,
            });
        }
        assert_eq!(nodes.len(), 10_000);
        assert_eq!(edges.len(), 10_000);
        let graph = ChangeGraph {
            revision: GraphRevisionId::parse("grf_large").unwrap(),
            snapshot_id: "snp_large".into(),
            fingerprint: "large".into(),
            nodes,
            edges,
            coverage: Vec::new(),
            unfinished_frontier: Vec::new(),
        };
        let queue = Queue {
            graph_revision: "grf_large".into(),
            stale: false,
            fully_ranked: true,
            finalized: true,
            context_digest: None,
            metrics: RankingMetrics::default(),
            assessed_changes: 100,
            total_changes: 100,
            items,
        };
        let snapshot = Snapshot {
            id: SnapshotId::parse("snp_large").unwrap(),
            repository: "/repo".into(),
            input: SnapshotInput::Uncommitted,
            original_base: "base".into(),
            original_head: "head".into(),
            comparison_base: "base".into(),
            before_commit: "base".into(),
            after_commit: "head".into(),
            captured_at: Utc::now(),
            source_fingerprint: "large".into(),
            files: vec![FileChange {
                status: "M".into(),
                old_path: Some(GitPath::from_bytes(path.as_bytes().to_vec())),
                new_path: Some(GitPath::from_bytes(path.as_bytes().to_vec())),
                old_mode: "100644".into(),
                new_mode: "100644".into(),
                old_object: "old".into(),
                new_object: "new".into(),
                before_blob: None,
                after_blob: None,
                binary: false,
                submodule: false,
                hunks,
            }],
            storage_dir: "/snapshot".into(),
        };
        (queue, graph, snapshot)
    }
}

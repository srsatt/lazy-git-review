use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use crate::graph::{ChangeGraph, NodeKind, SourceSide};
use crate::review_units::ReviewUnits;
use crate::snapshot::Snapshot;
use crate::syntax::{self, HighlightedSource};

const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_LINE_CELLS: usize = 4_096;
const MAX_SOURCE_BYTES: u64 = 64 * 1024;
const MAX_CONTEXT_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
const SURROUNDING_CONTEXT_LINES: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LineKind {
    Header,
    Surrounding,
    Context,
    Added,
    Deleted,
    Note,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DisplayLine {
    pub(super) old: Option<u32>,
    pub(super) new: Option<u32>,
    pub(super) kind: LineKind,
    pub(super) marker: char,
    pub(super) text: String,
}

#[derive(Clone, Debug)]
pub(super) struct Preview {
    pub(super) path: String,
    pub(super) status: String,
    pub(super) lines: Vec<DisplayLine>,
    pub(super) truncated: bool,
    pub(super) left_source: Option<PathBuf>,
    pub(super) right_source: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(super) enum SyntaxState {
    Loading,
    Ready {
        left: Option<HighlightedSource>,
        right: Option<HighlightedSource>,
    },
    Plain,
    Error,
}

struct SyntaxRequest {
    generation: u64,
    key: String,
    left: Option<PathBuf>,
    right: Option<PathBuf>,
}

struct SyntaxResponse {
    generation: u64,
    key: String,
    state: SyntaxState,
    bytes: usize,
}

pub(super) struct SyntaxLoader {
    sender: Sender<SyntaxRequest>,
    receiver: Receiver<SyntaxResponse>,
    states: BTreeMap<String, (u64, SyntaxState)>,
    order: VecDeque<String>,
    cached_bytes: usize,
}

impl SyntaxLoader {
    pub(super) fn new() -> Self {
        let (request_sender, request_receiver) = mpsc::channel::<SyntaxRequest>();
        let (response_sender, response_receiver) = mpsc::channel::<SyntaxResponse>();
        std::thread::spawn(move || {
            while let Ok(request) = request_receiver.recv() {
                let result = load_syntax(request.left.as_ref(), request.right.as_ref());
                let (state, bytes) = match result {
                    Ok((left, right)) => {
                        let bytes = left.as_ref().map_or(0, |value| value.source_bytes)
                            + right.as_ref().map_or(0, |value| value.source_bytes);
                        if left.is_none() && right.is_none() {
                            (SyntaxState::Plain, 0)
                        } else {
                            (SyntaxState::Ready { left, right }, bytes)
                        }
                    }
                    Err(_) => (SyntaxState::Error, 0),
                };
                if response_sender
                    .send(SyntaxResponse {
                        generation: request.generation,
                        key: request.key,
                        state,
                        bytes,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            sender: request_sender,
            receiver: response_receiver,
            states: BTreeMap::new(),
            order: VecDeque::new(),
            cached_bytes: 0,
        }
    }

    pub(super) fn request(&mut self, generation: u64, key: String, preview: &Preview) {
        if let Some((pending_generation, state)) = self.states.get(&key)
            && (!matches!(state, SyntaxState::Loading) || *pending_generation == generation)
        {
            return;
        }
        self.states
            .insert(key.clone(), (generation, SyntaxState::Loading));
        if self
            .sender
            .send(SyntaxRequest {
                generation,
                key: key.clone(),
                left: preview.left_source.clone(),
                right: preview.right_source.clone(),
            })
            .is_err()
        {
            self.states.insert(key, (generation, SyntaxState::Error));
        }
    }

    pub(super) fn poll(&mut self, generation: u64) {
        while let Ok(response) = self.receiver.try_recv() {
            self.accept_response(generation, response);
        }
    }

    fn accept_response(&mut self, generation: u64, response: SyntaxResponse) {
        if response.generation != generation {
            if self
                .states
                .get(&response.key)
                .is_some_and(|(stored, state)| {
                    *stored == response.generation && matches!(state, SyntaxState::Loading)
                })
            {
                self.states.remove(&response.key);
            }
            return;
        }
        while self.cached_bytes.saturating_add(response.bytes) > MAX_CACHE_BYTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some((_, SyntaxState::Ready { left, right })) = self.states.remove(&oldest) {
                self.cached_bytes = self.cached_bytes.saturating_sub(
                    left.as_ref().map_or(0, |value| value.source_bytes)
                        + right.as_ref().map_or(0, |value| value.source_bytes),
                );
            }
        }
        self.cached_bytes = self.cached_bytes.saturating_add(response.bytes);
        self.order.push_back(response.key.clone());
        self.states
            .insert(response.key, (response.generation, response.state));
    }

    pub(super) fn state(&self, id: &str) -> Option<&SyntaxState> {
        self.states.get(id).map(|(_, state)| state)
    }
}

fn load_syntax(
    left: Option<&PathBuf>,
    right: Option<&PathBuf>,
) -> crate::Result<(Option<HighlightedSource>, Option<HighlightedSource>)> {
    fn one(path: Option<&PathBuf>) -> crate::Result<Option<HighlightedSource>> {
        let Some(path) = path.filter(|path| path.is_file()) else {
            return Ok(None);
        };
        let metadata = std::fs::metadata(path)?;
        if metadata.len() > syntax::MAX_HIGHLIGHT_BYTES as u64 {
            return Ok(None);
        }
        syntax::highlight(path, &std::fs::read(path)?)
    }
    Ok((one(left)?, one(right)?))
}

#[derive(Default)]
pub(super) struct Store {
    previews: BTreeMap<String, Preview>,
}

#[derive(Clone, Debug)]
pub(super) enum SourceState {
    Loading,
    Ready { lines: Vec<String>, truncated: bool },
    Error(String),
}

struct SourceRequest {
    generation: u64,
    key: String,
    path: PathBuf,
    line: u32,
}

struct SourceResponse {
    generation: u64,
    key: String,
    state: SourceState,
}

pub(super) struct SourceLoader {
    sender: Sender<SourceRequest>,
    receiver: Receiver<SourceResponse>,
    states: BTreeMap<String, SourceState>,
}

impl SourceLoader {
    pub(super) fn new() -> Self {
        let (request_sender, request_receiver) = mpsc::channel::<SourceRequest>();
        let (response_sender, response_receiver) = mpsc::channel::<SourceResponse>();
        std::thread::spawn(move || {
            while let Ok(request) = request_receiver.recv() {
                let state = load_excerpt(&request.path, request.line);
                if response_sender
                    .send(SourceResponse {
                        generation: request.generation,
                        key: request.key,
                        state,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            sender: request_sender,
            receiver: response_receiver,
            states: BTreeMap::new(),
        }
    }

    pub(super) fn request(&mut self, generation: u64, key: String, path: PathBuf, line: u32) {
        if self
            .states
            .get(&key)
            .is_some_and(|state| !matches!(state, SourceState::Loading))
        {
            return;
        }
        self.states.insert(key.clone(), SourceState::Loading);
        if self
            .sender
            .send(SourceRequest {
                generation,
                key: key.clone(),
                path,
                line,
            })
            .is_err()
        {
            self.states
                .insert(key, SourceState::Error("source worker stopped".into()));
        }
    }

    pub(super) fn poll(&mut self, current_generation: u64) {
        while let Ok(response) = self.receiver.try_recv() {
            if response.generation == current_generation {
                self.states.insert(response.key, response.state);
            } else {
                self.states.remove(&response.key);
            }
        }
    }

    pub(super) fn state(&self, key: &str) -> Option<&SourceState> {
        self.states.get(key)
    }
}

impl Store {
    pub(super) fn from_snapshot(
        snapshot: &Snapshot,
        units: Option<&ReviewUnits>,
        graph: &ChangeGraph,
    ) -> Self {
        let mut previews = BTreeMap::new();
        let mut cached_bytes = 0usize;
        for (file_index, file) in snapshot.files.iter().enumerate() {
            let path = file
                .new_path
                .as_ref()
                .or(file.old_path.as_ref())
                .map(|path| path.display.clone())
                .unwrap_or_else(|| "(non-text change)".into());
            let left_source = file.old_path.as_ref().and_then(|path| {
                path.to_path_buf()
                    .ok()
                    .map(|path| snapshot.storage_dir.join("before").join(path))
            });
            let right_source = file.new_path.as_ref().and_then(|path| {
                path.to_path_buf()
                    .ok()
                    .map(|path| snapshot.storage_dir.join("after").join(path))
            });
            let left_lines = load_context_source(left_source.as_ref());
            let right_lines = load_context_source(right_source.as_ref());
            if file.hunks.is_empty() {
                let kind = if file.binary {
                    "binary"
                } else if file.submodule {
                    "submodule"
                } else {
                    "metadata"
                };
                previews.insert(
                    format!("f_{file_index:08x}"),
                    Preview {
                        path: path.clone(),
                        status: file.status.clone(),
                        lines: vec![DisplayLine {
                            old: None,
                            new: None,
                            kind: LineKind::Note,
                            marker: '!',
                            text: format!(
                                "{kind} change · mode {} → {} · object {} → {}",
                                file.old_mode, file.new_mode, file.old_object, file.new_object
                            ),
                        }],
                        truncated: false,
                        left_source: left_source.clone(),
                        right_source: right_source.clone(),
                    },
                );
            }
            for hunk in &file.hunks {
                if cached_bytes >= MAX_CACHE_BYTES {
                    break;
                }
                let remaining = MAX_CACHE_BYTES - cached_bytes;
                let (lines, truncated, bytes) =
                    parse_patch(&hunk.patch, hunk.old_start, hunk.new_start, remaining);
                let lines = expand_context(
                    lines,
                    left_lines.as_deref(),
                    right_lines.as_deref(),
                    SURROUNDING_CONTEXT_LINES,
                );
                cached_bytes += bytes;
                previews.insert(
                    hunk.id.clone(),
                    Preview {
                        path: path.clone(),
                        status: file.status.clone(),
                        lines,
                        truncated,
                        left_source: left_source.clone(),
                        right_source: right_source.clone(),
                    },
                );
                if let Some(units) = units {
                    for unit in units
                        .units
                        .iter()
                        .filter(|unit| unit.parent_hunk_id == hunk.id)
                    {
                        let (patch, old_start, new_start) =
                            crate::review_units::unit_patch_with_coordinates(
                                snapshot, units, &unit.id,
                            )
                            .unwrap_or_else(|| {
                                (hunk.patch.clone(), hunk.old_start, hunk.new_start)
                            });
                        let (lines, truncated, bytes) = parse_patch(
                            &patch,
                            old_start,
                            new_start,
                            MAX_CACHE_BYTES.saturating_sub(cached_bytes),
                        );
                        let lines = expand_context(
                            lines,
                            left_lines.as_deref(),
                            right_lines.as_deref(),
                            SURROUNDING_CONTEXT_LINES,
                        );
                        cached_bytes = cached_bytes.saturating_add(bytes);
                        previews.insert(
                            unit.id.clone(),
                            Preview {
                                path: path.clone(),
                                status: format!(
                                    "{} · part {}/{}",
                                    file.status, unit.part, unit.parts
                                ),
                                lines,
                                truncated,
                                left_source: left_source.clone(),
                                right_source: right_source.clone(),
                            },
                        );
                    }
                }
            }
        }
        for node in graph
            .nodes
            .values()
            .filter(|node| node.kind == NodeKind::Test)
        {
            if previews.contains_key(&node.id) {
                continue;
            }
            let Some(location) = node.preferred_review_location() else {
                continue;
            };
            let side = match location.side {
                SourceSide::Left => "before",
                SourceSide::Right => "after",
            };
            let Ok(relative) = location.path.to_path_buf() else {
                continue;
            };
            let source_path = snapshot.storage_dir.join(side).join(relative);
            let Ok(source) = std::fs::read_to_string(&source_path) else {
                continue;
            };
            let source_lines: Vec<_> = source.lines().collect();
            let target = location.range.start.line as usize;
            let start = target.saturating_sub(4);
            let end = (target + 6).min(source_lines.len());
            let lines = source_lines[start..end]
                .iter()
                .enumerate()
                .map(|(offset, line)| DisplayLine {
                    old: (location.side == SourceSide::Left).then_some((start + offset + 1) as u32),
                    new: (location.side == SourceSide::Right)
                        .then_some((start + offset + 1) as u32),
                    kind: LineKind::Context,
                    marker: ' ',
                    text: sanitize(line),
                })
                .collect();
            previews.insert(
                node.id.clone(),
                Preview {
                    path: location.path.display.clone(),
                    status: "unchanged test · execution evidence".into(),
                    lines,
                    truncated: false,
                    left_source: (location.side == SourceSide::Left).then_some(source_path.clone()),
                    right_source: (location.side == SourceSide::Right).then_some(source_path),
                },
            );
        }
        Self { previews }
    }

    pub(super) fn get(&self, id: &str) -> Option<&Preview> {
        self.previews.get(id)
    }
}

fn parse_patch(
    patch: &str,
    old_start: u32,
    new_start: u32,
    max_bytes: usize,
) -> (Vec<DisplayLine>, bool, usize) {
    let mut old = old_start;
    let mut new = new_start;
    let mut consumed = 0usize;
    let mut truncated = false;
    let mut lines = Vec::new();
    for raw in patch.lines() {
        if consumed.saturating_add(raw.len()) > max_bytes {
            truncated = true;
            break;
        }
        consumed += raw.len() + 1;
        let (kind, marker, old_line, new_line, content) = if raw.starts_with("@@") {
            (LineKind::Header, '@', None, None, raw)
        } else if let Some(content) = raw.strip_prefix('+') {
            let line = new;
            new += 1;
            (LineKind::Added, '+', None, Some(line), content)
        } else if let Some(content) = raw.strip_prefix('-') {
            let line = old;
            old += 1;
            (LineKind::Deleted, '-', Some(line), None, content)
        } else if let Some(content) = raw.strip_prefix(' ') {
            let old_line = old;
            let new_line = new;
            old += 1;
            new += 1;
            (
                LineKind::Context,
                ' ',
                Some(old_line),
                Some(new_line),
                content,
            )
        } else {
            (LineKind::Note, '!', None, None, raw)
        };
        lines.push(DisplayLine {
            old: old_line,
            new: new_line,
            kind,
            marker,
            text: sanitize(content),
        });
    }
    (lines, truncated, consumed)
}

fn load_context_source(path: Option<&PathBuf>) -> Option<Vec<String>> {
    let path = path?;
    if std::fs::metadata(path).ok()?.len() > MAX_CONTEXT_SOURCE_BYTES {
        return None;
    }
    Some(
        std::fs::read_to_string(path)
            .ok()?
            .lines()
            .map(str::to_owned)
            .collect(),
    )
}

fn expand_context(
    mut lines: Vec<DisplayLine>,
    left: Option<&[String]>,
    right: Option<&[String]>,
    radius: usize,
) -> Vec<DisplayLine> {
    if radius == 0 || (left.is_none() && right.is_none()) {
        return lines;
    }
    let body: Vec<_> = lines
        .iter()
        .filter(|line| !matches!(line.kind, LineKind::Header | LineKind::Note))
        .collect();
    if body.is_empty() {
        return lines;
    }
    let first_old = body.iter().filter_map(|line| line.old).min();
    let first_new = body.iter().filter_map(|line| line.new).min();
    let last_old = body.iter().filter_map(|line| line.old).max();
    let last_new = body.iter().filter_map(|line| line.new).max();

    let mut before = Vec::new();
    for distance in (1..=radius as u32).rev() {
        let old = first_old.and_then(|line| line.checked_sub(distance));
        let new = first_new.and_then(|line| line.checked_sub(distance));
        if let Some(text) = source_line(right, new).or_else(|| source_line(left, old)) {
            before.push(DisplayLine {
                old,
                new,
                kind: LineKind::Surrounding,
                marker: ' ',
                text: sanitize(text),
            });
        }
    }

    let mut after = Vec::new();
    for distance in 1..=radius as u32 {
        let old = last_old.and_then(|line| line.checked_add(distance));
        let new = last_new.and_then(|line| line.checked_add(distance));
        if let Some(text) = source_line(right, new).or_else(|| source_line(left, old)) {
            after.push(DisplayLine {
                old,
                new,
                kind: LineKind::Surrounding,
                marker: ' ',
                text: sanitize(text),
            });
        }
    }

    let insert_at = lines
        .iter()
        .take_while(|line| line.kind == LineKind::Header)
        .count();
    lines.splice(insert_at..insert_at, before);
    lines.extend(after);
    lines
}

fn source_line(source: Option<&[String]>, line: Option<u32>) -> Option<&str> {
    let index = line?.checked_sub(1)? as usize;
    source?.get(index).map(String::as_str)
}

fn sanitize(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars() {
        match character {
            '\t' => result.push_str("    "),
            character if character.is_control() => result.push('�'),
            character => result.push(character),
        }
        if result.chars().count() >= MAX_LINE_CELLS {
            result.push('…');
            break;
        }
    }
    result
}

fn load_excerpt(path: &PathBuf, target_line: u32) -> SourceState {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) => return SourceState::Error(format!("cannot read source: {error}")),
    };
    let mut bytes = Vec::new();
    if let Err(error) = file.by_ref().take(MAX_SOURCE_BYTES).read_to_end(&mut bytes) {
        return SourceState::Error(format!("cannot read source: {error}"));
    }
    let truncated = file.read(&mut [0]).is_ok_and(|count| count > 0);
    let text = String::from_utf8_lossy(&bytes);
    let all: Vec<_> = text.lines().collect();
    let target = target_line.saturating_sub(1) as usize;
    if target >= all.len() && truncated {
        return SourceState::Error(
            "source location is beyond the 64 KiB excerpt; use graph source for continuation"
                .into(),
        );
    }
    let start = target.saturating_sub(4);
    let end = (target + 6).min(all.len());
    SourceState::Ready {
        lines: all[start..end]
            .iter()
            .enumerate()
            .map(|(offset, line)| format!("{:>5}  {}", start + offset + 1, sanitize(line)))
            .collect(),
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_both_coordinates_and_sanitizes_controls() {
        let (lines, truncated, _) = parse_patch(
            "@@ -4,2 +4,2 @@\n-old\tvalue\n+new\u{7}value\n same\n",
            4,
            4,
            1_024,
        );
        assert!(!truncated);
        assert_eq!((lines[1].old, lines[1].new), (Some(4), None));
        assert_eq!((lines[2].old, lines[2].new), (None, Some(4)));
        assert_eq!((lines[3].old, lines[3].new), (Some(5), Some(5)));
        assert_eq!(lines[1].text, "old    value");
        assert_eq!(lines[2].text, "new�value");
    }

    #[test]
    fn expands_hunk_with_dimmed_surrounding_source_coordinates() {
        let source = (1..=12)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>();
        let (lines, _, _) = parse_patch("@@ -5 +5 @@\n-old\n+new\n", 5, 5, 1_024);
        let lines = expand_context(lines, Some(&source), Some(&source), 2);

        assert_eq!(lines[1].kind, LineKind::Surrounding);
        assert_eq!((lines[1].old, lines[1].new), (Some(3), Some(3)));
        assert_eq!(lines[1].text, "line 3");
        assert_eq!(lines.last().unwrap().text, "line 7");
    }

    #[test]
    fn truncates_at_cache_boundary() {
        let (lines, truncated, _) = parse_patch("@@ x\n+abcdef\n", 1, 1, 5);
        assert_eq!(lines.len(), 1);
        assert!(truncated);
    }

    #[test]
    fn source_excerpt_is_bounded_sanitized_and_reports_missing_files() {
        let mut source = tempfile::NamedTempFile::new().unwrap();
        writeln!(source, "first\tline").unwrap();
        writeln!(source, "{}", "x".repeat(MAX_LINE_CELLS + 20)).unwrap();

        let SourceState::Ready { lines, truncated } = load_excerpt(&source.path().to_path_buf(), 2)
        else {
            panic!("expected source excerpt");
        };
        assert!(!truncated);
        assert!(lines[0].contains("first    line"));
        assert!(lines[1].ends_with('…'));

        let SourceState::Error(message) = load_excerpt(&source.path().with_extension("missing"), 1)
        else {
            panic!("expected recoverable source error");
        };
        assert!(message.starts_with("cannot read source:"));
    }

    #[test]
    fn source_excerpt_reports_when_requested_line_is_beyond_bounded_read() {
        let mut source = tempfile::NamedTempFile::new().unwrap();
        source
            .write_all(&vec![b'a'; MAX_SOURCE_BYTES as usize + 1])
            .unwrap();

        let SourceState::Error(message) = load_excerpt(&source.path().to_path_buf(), u32::MAX)
        else {
            panic!("expected bounded-read error");
        };
        assert!(message.contains("beyond the 64 KiB excerpt"));
    }

    #[test]
    fn stale_syntax_response_cannot_erase_newer_request() {
        let mut loader = SyntaxLoader::new();
        loader
            .states
            .insert("item".into(), (2, SyntaxState::Loading));
        loader.accept_response(
            2,
            SyntaxResponse {
                generation: 1,
                key: "item".into(),
                state: SyntaxState::Plain,
                bytes: 0,
            },
        );
        assert!(matches!(loader.state("item"), Some(SyntaxState::Loading)));
        loader.accept_response(
            2,
            SyntaxResponse {
                generation: 2,
                key: "item".into(),
                state: SyntaxState::Plain,
                bytes: 0,
            },
        );
        assert!(matches!(loader.state("item"), Some(SyntaxState::Plain)));
    }
}

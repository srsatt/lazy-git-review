use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use async_lsp::lsp_types::{DocumentSymbol, DocumentSymbolResponse, SymbolInformation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::model::GraphRevisionId;
use crate::position::{Position, TextRange};
use crate::snapshot::{GitPath, Snapshot};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceSide {
    Left,
    Right,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Hunk,
    ReviewUnit,
    Symbol,
    FileChange,
    Reference,
    Test,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceLocation {
    pub side: SourceSide,
    pub path: GitPath,
    pub range: TextRange,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    pub symbol_kind: Option<String>,
    pub changed: bool,
    pub locations: Vec<SourceLocation>,
    pub selection_range: Option<TextRange>,
    pub hunk_ids: Vec<String>,
}

impl GraphNode {
    pub fn preferred_review_location(&self) -> Option<&SourceLocation> {
        self.locations
            .iter()
            .find(|location| {
                location.side == SourceSide::Right && location.range.start != location.range.end
            })
            .or_else(|| {
                self.locations
                    .iter()
                    .find(|location| location.range.start != location.range.end)
            })
            .or_else(|| {
                self.locations
                    .iter()
                    .find(|location| location.side == SourceSide::Right)
            })
            .or_else(|| self.locations.first())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Contains,
    Overlaps,
    References,
    Calls,
    TestReference,
    Counterpart,
    Definition,
    RuntimeTest,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GraphEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub producer: String,
    pub evidence_kind: String,
    pub confidence: f32,
    pub location: Option<SourceLocation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CoverageEntry {
    pub project: PathBuf,
    pub side: SourceSide,
    pub profile: String,
    pub server_version: Option<String>,
    pub position_encoding: Option<String>,
    pub status: String,
    pub supported_relations: Vec<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChangeGraph {
    pub revision: GraphRevisionId,
    pub snapshot_id: String,
    pub fingerprint: String,
    pub nodes: BTreeMap<String, GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub coverage: Vec<CoverageEntry>,
    pub unfinished_frontier: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct NormalizedSymbol {
    pub name: String,
    pub detail: Option<String>,
    pub kind: String,
    pub range: TextRange,
    pub selection_range: TextRange,
    pub children: Vec<NormalizedSymbol>,
}

impl ChangeGraph {
    pub fn from_snapshot(snapshot: &Snapshot) -> Self {
        let mut graph = Self {
            revision: GraphRevisionId::new(),
            snapshot_id: snapshot.id.to_string(),
            fingerprint: snapshot_fingerprint(snapshot),
            nodes: BTreeMap::new(),
            edges: Vec::new(),
            coverage: Vec::new(),
            unfinished_frontier: Vec::new(),
        };
        for (file_index, file) in snapshot.files.iter().enumerate() {
            if file.hunks.is_empty() {
                let id = format!("f_{file_index:08x}");
                let locations = file
                    .old_path
                    .iter()
                    .map(|path| SourceLocation {
                        side: SourceSide::Left,
                        path: path.clone(),
                        range: line_range(0, 0),
                    })
                    .chain(file.new_path.iter().map(|path| SourceLocation {
                        side: SourceSide::Right,
                        path: path.clone(),
                        range: line_range(0, 0),
                    }))
                    .collect();
                graph.nodes.insert(
                    id.clone(),
                    GraphNode {
                        id,
                        kind: NodeKind::FileChange,
                        name: file.status.clone(),
                        symbol_kind: None,
                        changed: true,
                        locations,
                        selection_range: None,
                        hunk_ids: Vec::new(),
                    },
                );
            }
            for hunk in &file.hunks {
                let mut locations = Vec::new();
                if let Some(path) = &file.old_path {
                    locations.push(SourceLocation {
                        side: SourceSide::Left,
                        path: path.clone(),
                        range: line_range(hunk.old_start.saturating_sub(1), hunk.old_count),
                    });
                }
                if let Some(path) = &file.new_path {
                    locations.push(SourceLocation {
                        side: SourceSide::Right,
                        path: path.clone(),
                        range: line_range(hunk.new_start.saturating_sub(1), hunk.new_count),
                    });
                }
                graph.nodes.insert(
                    hunk.id.clone(),
                    GraphNode {
                        id: hunk.id.clone(),
                        kind: NodeKind::Hunk,
                        name: hunk.header.clone(),
                        symbol_kind: None,
                        changed: true,
                        locations,
                        selection_range: None,
                        hunk_ids: vec![hunk.id.clone()],
                    },
                );
            }
        }
        graph
    }

    pub fn add_document_symbols(
        &mut self,
        side: SourceSide,
        path: GitPath,
        response: DocumentSymbolResponse,
    ) -> Vec<String> {
        let symbols = normalize_symbols(response);
        let mut ids = Vec::new();
        for symbol in symbols {
            self.add_symbol(side, path.clone(), symbol, None, &mut ids);
        }
        ids
    }

    fn add_symbol(
        &mut self,
        side: SourceSide,
        path: GitPath,
        symbol: NormalizedSymbol,
        parent: Option<String>,
        ids: &mut Vec<String>,
    ) {
        let id = symbol_id(side, &path, &symbol);
        let mut hunk_ids = Vec::new();
        for hunk in self
            .nodes
            .values()
            .filter(|node| node.kind == NodeKind::Hunk)
        {
            if hunk.locations.iter().any(|location| {
                location.side == side
                    && location.path.bytes_base64 == path.bytes_base64
                    && ranges_overlap(location.range, symbol.range)
            }) {
                hunk_ids.push(hunk.id.clone());
            }
        }
        let changed = !hunk_ids.is_empty();
        self.nodes.insert(
            id.clone(),
            GraphNode {
                id: id.clone(),
                kind: NodeKind::Symbol,
                name: symbol.name.clone(),
                symbol_kind: Some(symbol.kind.clone()),
                changed,
                locations: vec![SourceLocation {
                    side,
                    path: path.clone(),
                    range: symbol.range,
                }],
                selection_range: Some(symbol.selection_range),
                hunk_ids: hunk_ids.clone(),
            },
        );
        if let Some(parent) = parent {
            self.add_edge(
                parent,
                id.clone(),
                EdgeKind::Contains,
                "lsp",
                "resolved",
                1.0,
                None,
            );
        }
        for hunk in hunk_ids {
            self.add_edge(
                id.clone(),
                hunk,
                EdgeKind::Overlaps,
                "git+lsp",
                "resolved",
                1.0,
                None,
            );
        }
        ids.push(id.clone());
        for child in symbol.children {
            self.add_symbol(side, path.clone(), child, Some(id.clone()), ids);
        }
    }

    pub fn add_reference(
        &mut self,
        from: &str,
        location: SourceLocation,
        resolved: bool,
    ) -> String {
        let to = location_node_id(&location);
        self.nodes.entry(to.clone()).or_insert_with(|| GraphNode {
            id: to.clone(),
            kind: NodeKind::Reference,
            name: location.path.display.clone(),
            symbol_kind: None,
            changed: false,
            locations: vec![location.clone()],
            selection_range: None,
            hunk_ids: Vec::new(),
        });
        let test = is_test_path(&location.path.display);
        self.add_edge(
            from.into(),
            to.clone(),
            if test {
                EdgeKind::TestReference
            } else {
                EdgeKind::References
            },
            "lsp",
            if resolved { "resolved" } else { "heuristic" },
            if resolved { 1.0 } else { 0.45 },
            Some(location),
        );
        to
    }

    pub fn add_location_relation(
        &mut self,
        symbol_id: &str,
        location: SourceLocation,
        kind: EdgeKind,
        symbol_to_location: bool,
        evidence_kind: &str,
        confidence: f32,
    ) -> String {
        let location_id = location_node_id(&location);
        self.nodes
            .entry(location_id.clone())
            .or_insert_with(|| GraphNode {
                id: location_id.clone(),
                kind: NodeKind::Reference,
                name: location.path.display.clone(),
                symbol_kind: None,
                changed: false,
                locations: vec![location.clone()],
                selection_range: None,
                hunk_ids: Vec::new(),
            });
        let (from, to) = if symbol_to_location {
            (symbol_id.to_owned(), location_id.clone())
        } else {
            (location_id.clone(), symbol_id.to_owned())
        };
        self.add_edge(
            from,
            to,
            kind,
            "lsp",
            evidence_kind,
            confidence,
            Some(location),
        );
        location_id
    }

    pub fn link_counterparts(&mut self, snapshot: &Snapshot) {
        for file in &snapshot.files {
            let (Some(old_path), Some(new_path)) = (&file.old_path, &file.new_path) else {
                continue;
            };
            let left: Vec<_> = self
                .nodes
                .values()
                .filter(|node| {
                    node.kind == NodeKind::Symbol
                        && node.locations.iter().any(|location| {
                            location.side == SourceSide::Left
                                && location.path.bytes_base64 == old_path.bytes_base64
                        })
                })
                .map(|node| (node.id.clone(), node.name.clone(), node.symbol_kind.clone()))
                .collect();
            let right: Vec<_> = self
                .nodes
                .values()
                .filter(|node| {
                    node.kind == NodeKind::Symbol
                        && node.locations.iter().any(|location| {
                            location.side == SourceSide::Right
                                && location.path.bytes_base64 == new_path.bytes_base64
                        })
                })
                .map(|node| (node.id.clone(), node.name.clone(), node.symbol_kind.clone()))
                .collect();
            for (left_id, left_name, left_kind) in &left {
                let candidates: Vec<_> = right
                    .iter()
                    .filter(|(_, name, kind)| name == left_name && kind == left_kind)
                    .collect();
                if candidates.len() == 1 {
                    self.add_edge(
                        left_id.clone(),
                        candidates[0].0.clone(),
                        EdgeKind::Counterpart,
                        "symbol_identity",
                        "heuristic",
                        if old_path.bytes_base64 == new_path.bytes_base64 {
                            0.9
                        } else {
                            0.75
                        },
                        None,
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_edge(
        &mut self,
        from: String,
        to: String,
        kind: EdgeKind,
        producer: &str,
        evidence_kind: &str,
        confidence: f32,
        location: Option<SourceLocation>,
    ) {
        let raw = format!("{from}:{to}:{kind:?}:{producer}:{evidence_kind}");
        let digest = hex::encode(Sha256::digest(raw.as_bytes()));
        let id = format!("e_{}", &digest[..20]);
        if !self.edges.iter().any(|edge| edge.id == id) {
            self.edges.push(GraphEdge {
                id,
                from,
                to,
                kind,
                producer: producer.into(),
                evidence_kind: evidence_kind.into(),
                confidence,
                location,
            });
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self> {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    pub fn walk(
        &self,
        seeds: &[String],
        kinds: &BTreeSet<EdgeKind>,
        depth: usize,
        limit: usize,
    ) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut queue: VecDeque<(String, usize)> =
            seeds.iter().cloned().map(|id| (id, 0)).collect();
        while let Some((id, current_depth)) = queue.pop_front() {
            if !seen.insert(id.clone()) || seen.len() >= limit {
                continue;
            }
            if current_depth >= depth {
                continue;
            }
            for edge in &self.edges {
                if !kinds.is_empty() && !kinds.contains(&edge.kind) {
                    continue;
                }
                if edge.from == id {
                    queue.push_back((edge.to.clone(), current_depth + 1));
                }
                if edge.to == id {
                    queue.push_back((edge.from.clone(), current_depth + 1));
                }
            }
        }
        seen.into_iter().collect()
    }
}

pub fn normalize_symbols(response: DocumentSymbolResponse) -> Vec<NormalizedSymbol> {
    match response {
        DocumentSymbolResponse::Nested(symbols) => {
            symbols.into_iter().map(normalize_nested).collect()
        }
        DocumentSymbolResponse::Flat(symbols) => symbols.into_iter().map(normalize_flat).collect(),
    }
}

fn normalize_nested(symbol: DocumentSymbol) -> NormalizedSymbol {
    NormalizedSymbol {
        name: symbol.name,
        detail: symbol.detail,
        kind: format!("{:?}", symbol.kind),
        range: convert_range(symbol.range),
        selection_range: convert_range(symbol.selection_range),
        children: symbol
            .children
            .unwrap_or_default()
            .into_iter()
            .map(normalize_nested)
            .collect(),
    }
}

#[allow(deprecated)]
fn normalize_flat(symbol: SymbolInformation) -> NormalizedSymbol {
    NormalizedSymbol {
        name: symbol.name,
        detail: None,
        kind: format!("{:?}", symbol.kind),
        range: convert_range(symbol.location.range),
        selection_range: convert_range(symbol.location.range),
        children: Vec::new(),
    }
}

fn convert_range(range: async_lsp::lsp_types::Range) -> TextRange {
    TextRange {
        start: Position {
            line: range.start.line,
            character: range.start.character,
        },
        end: Position {
            line: range.end.line,
            character: range.end.character,
        },
    }
}

fn line_range(start: u32, count: u32) -> TextRange {
    TextRange {
        start: Position {
            line: start,
            character: 0,
        },
        end: Position {
            line: start.saturating_add(count),
            character: 0,
        },
    }
}

fn ranges_overlap(a: TextRange, b: TextRange) -> bool {
    (a.start.line, a.start.character) < (b.end.line, b.end.character)
        && (b.start.line, b.start.character) < (a.end.line, a.end.character)
}

fn symbol_id(side: SourceSide, path: &GitPath, symbol: &NormalizedSymbol) -> String {
    let raw = format!(
        "{side:?}:{}:{}:{}:{}:{}",
        path.bytes_base64,
        symbol.name,
        symbol.range.start.line,
        symbol.range.start.character,
        symbol.kind
    );
    let digest = hex::encode(Sha256::digest(raw.as_bytes()));
    format!("s_{}", &digest[..20])
}

fn location_node_id(location: &SourceLocation) -> String {
    let raw = format!(
        "{:?}:{}:{:?}",
        location.side, location.path.bytes_base64, location.range
    );
    let digest = hex::encode(Sha256::digest(raw.as_bytes()));
    format!("r_{}", &digest[..20])
}

pub fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.starts_with("test/")
        || lower.starts_with("tests/")
        || lower.starts_with("__tests__/")
        || lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || [".test.", ".spec.", "_test."]
            .iter()
            .any(|marker| lower.contains(marker))
}

fn snapshot_fingerprint(snapshot: &Snapshot) -> String {
    let mut digest = Sha256::new();
    digest.update(snapshot.id.as_str());
    digest.update(&snapshot.source_fingerprint);
    hex::encode(digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git;
    use crate::snapshot::{SnapshotInput, capture};
    use async_lsp::lsp_types::{
        DocumentSymbol, DocumentSymbolResponse, Position as LspPosition, Range, SymbolKind,
    };
    use tempfile::TempDir;

    #[test]
    #[allow(deprecated)]
    fn associates_nested_symbols_with_one_hunk_without_duplication() {
        let dir = TempDir::new().unwrap();
        git::run(dir.path(), &[git::os("init"), git::os("-q")]).unwrap();
        fs::write(
            dir.path().join("a.ts"),
            "export function a() { return 1; }\n",
        )
        .unwrap();
        git::run(dir.path(), &[git::os("add"), git::os(".")]).unwrap();
        git::run(
            dir.path(),
            &[
                git::os("-c"),
                git::os("user.name=T"),
                git::os("-c"),
                git::os("user.email=t@e"),
                git::os("commit"),
                git::os("-qm"),
                git::os("base"),
            ],
        )
        .unwrap();
        let base = git::text(dir.path(), &[git::os("rev-parse"), git::os("HEAD")]).unwrap();
        fs::write(
            dir.path().join("a.ts"),
            "export function a() { return 2; }\n",
        )
        .unwrap();
        git::run(dir.path(), &[git::os("add"), git::os(".")]).unwrap();
        git::run(
            dir.path(),
            &[
                git::os("-c"),
                git::os("user.name=T"),
                git::os("-c"),
                git::os("user.email=t@e"),
                git::os("commit"),
                git::os("-qm"),
                git::os("head"),
            ],
        )
        .unwrap();
        let head = git::text(dir.path(), &[git::os("rev-parse"), git::os("HEAD")]).unwrap();
        let data = TempDir::new().unwrap();
        let snapshot = capture(
            dir.path(),
            SnapshotInput::Revisions { base, head },
            data.path(),
        )
        .unwrap();
        let path = snapshot.files[0].new_path.clone().unwrap();
        let mut graph = ChangeGraph::from_snapshot(&snapshot);
        let hunk = graph
            .nodes
            .values()
            .find(|node| node.kind == NodeKind::Hunk)
            .unwrap();
        assert_eq!(
            hunk.preferred_review_location().unwrap().side,
            SourceSide::Right
        );
        let range = Range::new(LspPosition::new(0, 0), LspPosition::new(0, 34));
        let symbol = DocumentSymbol {
            name: "a".into(),
            detail: None,
            kind: SymbolKind::FUNCTION,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        };
        let ids = graph.add_document_symbols(
            SourceSide::Right,
            path,
            DocumentSymbolResponse::Nested(vec![symbol]),
        );
        assert_eq!(ids.len(), 1);
        assert_eq!(graph.nodes[&ids[0]].hunk_ids.len(), 1);
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|e| e.kind == EdgeKind::Overlaps)
                .count(),
            1
        );
    }

    #[test]
    fn classifies_resolved_and_heuristic_tests() {
        assert!(is_test_path("src/a.test.ts"));
        assert!(is_test_path("tests/a.ts"));
        assert!(!is_test_path("src/latest.ts"));
    }
}

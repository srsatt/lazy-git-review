use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, Result};
use crate::graph::{ChangeGraph, EdgeKind, GraphNode, NodeKind, SourceLocation, SourceSide};
use crate::model::GraphRevisionId;
use crate::position::{Position, TextRange};
use crate::settings::ReviewUnitSettings;
use crate::snapshot::{FileChange, Hunk, Snapshot};

pub const REVIEW_UNITS_SCHEMA_VERSION: u32 = 1;
const PARTITION_ALGORITHM: &str = "captured-symbol-window-v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OwnedPatchRow {
    pub patch_row: usize,
    pub side: SourceSide,
    pub line: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewUnit {
    pub id: String,
    pub parent_hunk_id: String,
    pub part: usize,
    pub parts: usize,
    pub title: String,
    pub changed_lines: usize,
    pub context_patch_start: usize,
    pub context_patch_end: usize,
    pub owned_rows: Vec<OwnedPatchRow>,
    pub old_range: Option<TextRange>,
    pub new_range: Option<TextRange>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewUnits {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub graph_revision: String,
    pub projection_revision: u64,
    pub algorithm: String,
    pub config_digest: String,
    pub active: bool,
    pub units: Vec<ReviewUnit>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PartitionPreview {
    pub split_hunks: usize,
    pub raw_hunks: usize,
    pub leaf_units: usize,
    pub review_units: ReviewUnits,
}

#[derive(Clone, Copy, Debug)]
struct PatchRow {
    patch_row: usize,
    side: SourceSide,
    line: u32,
}

pub fn path(snapshot: &Snapshot) -> PathBuf {
    snapshot.storage_dir.join("review-units.json")
}

pub fn backup_path(snapshot: &Snapshot) -> PathBuf {
    snapshot.storage_dir.join("review-units.backup.json")
}

pub fn build(snapshot: &Snapshot, graph: &ChangeGraph, config: &ReviewUnitSettings) -> ReviewUnits {
    let mut units = Vec::new();
    for file in &snapshot.files {
        for hunk in &file.hunks {
            let rows = changed_rows(hunk);
            if rows.len() <= config.threshold {
                continue;
            }
            let boundaries = semantic_boundaries(graph, file, hunk, &rows);
            let groups = partition_rows(&rows, &boundaries, config.target, config.hard_ceiling);
            let parts = groups.len();
            for (index, group) in groups.into_iter().enumerate() {
                let first = group.first().expect("partition group is non-empty");
                let last = group.last().expect("partition group is non-empty");
                let previous_changed = rows
                    .iter()
                    .rev()
                    .find(|row| row.patch_row < first.patch_row)
                    .map_or(1, |row| row.patch_row + 1);
                let next_changed = rows
                    .iter()
                    .find(|row| row.patch_row > last.patch_row)
                    .map_or(hunk.patch.lines().count(), |row| row.patch_row);
                let context_patch_start = first
                    .patch_row
                    .saturating_sub(config.context_lines)
                    .max(previous_changed);
                let context_patch_end = (last.patch_row + config.context_lines + 1)
                    .min(next_changed)
                    .min(hunk.patch.lines().count());
                let title = semantic_title(graph, file, hunk, group, index, parts);
                let owned_rows = group
                    .iter()
                    .map(|row| OwnedPatchRow {
                        patch_row: row.patch_row,
                        side: row.side,
                        line: row.line,
                    })
                    .collect::<Vec<_>>();
                let raw = format!(
                    "{}:{PARTITION_ALGORITHM}:{}",
                    hunk.id,
                    owned_rows
                        .iter()
                        .map(|row| format!("{}:{:?}:{}", row.patch_row, row.side, row.line))
                        .collect::<Vec<_>>()
                        .join(",")
                );
                let digest = hex::encode(Sha256::digest(raw.as_bytes()));
                units.push(ReviewUnit {
                    id: format!("u_{}", &digest[..20]),
                    parent_hunk_id: hunk.id.clone(),
                    part: index + 1,
                    parts,
                    title,
                    changed_lines: group.len(),
                    context_patch_start,
                    context_patch_end,
                    old_range: side_range(group, SourceSide::Left),
                    new_range: side_range(group, SourceSide::Right),
                    owned_rows,
                });
            }
        }
    }
    let config_digest = digest_json(config);
    ReviewUnits {
        schema_version: REVIEW_UNITS_SCHEMA_VERSION,
        snapshot_id: snapshot.id.to_string(),
        graph_revision: graph.revision.to_string(),
        projection_revision: 1,
        algorithm: PARTITION_ALGORITHM.into(),
        config_digest,
        active: true,
        units,
    }
}

pub fn preview(
    snapshot: &Snapshot,
    graph: &ChangeGraph,
    config: &ReviewUnitSettings,
) -> PartitionPreview {
    let review_units = build(snapshot, graph, config);
    let split_hunks = review_units
        .units
        .iter()
        .map(|unit| unit.parent_hunk_id.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let raw_hunks = snapshot.files.iter().map(|file| file.hunks.len()).sum();
    PartitionPreview {
        leaf_units: raw_hunks - split_hunks + review_units.units.len(),
        split_hunks,
        raw_hunks,
        review_units,
    }
}

pub fn load(snapshot: &Snapshot) -> Result<Option<ReviewUnits>> {
    let path = path(snapshot);
    if !path.is_file() {
        return Ok(None);
    }
    let units: ReviewUnits = serde_json::from_slice(&fs::read(path)?)?;
    validate(snapshot, &units)?;
    Ok(Some(units))
}

pub fn check_revision(snapshot: &Snapshot, expected_revision: u64) -> Result<()> {
    let actual = load(snapshot)?.map_or(0, |units| units.projection_revision);
    if actual != expected_revision {
        return Err(AppError::RevisionConflict {
            expected: expected_revision as i64,
            actual: actual as i64,
        });
    }
    Ok(())
}

pub fn save_checked(
    snapshot: &Snapshot,
    mut units: ReviewUnits,
    expected_revision: u64,
) -> Result<()> {
    if let Some(current) = load(snapshot)? {
        if current.projection_revision != expected_revision {
            return Err(AppError::RevisionConflict {
                expected: expected_revision as i64,
                actual: current.projection_revision as i64,
            });
        }
        fs::copy(path(snapshot), backup_path(snapshot))?;
        units.projection_revision = expected_revision + 1;
    } else if expected_revision != 0 {
        return Err(AppError::RevisionConflict {
            expected: expected_revision as i64,
            actual: 0,
        });
    } else {
        let mut disabled = units.clone();
        disabled.active = false;
        disabled.projection_revision = 0;
        atomic_write(
            &backup_path(snapshot),
            &serde_json::to_vec_pretty(&disabled)?,
        )?;
    }
    atomic_write(&path(snapshot), &serde_json::to_vec_pretty(&units)?)
}

pub fn rollback(snapshot: &Snapshot, expected_revision: u64) -> Result<ReviewUnits> {
    let current = load(snapshot)?
        .ok_or_else(|| invalid("review_units_missing", "no active review-unit projection"))?;
    if current.projection_revision != expected_revision {
        return Err(AppError::RevisionConflict {
            expected: expected_revision as i64,
            actual: current.projection_revision as i64,
        });
    }
    let backup = backup_path(snapshot);
    if !backup.is_file() {
        return Err(invalid(
            "review_units_backup_missing",
            "no review-unit backup is available",
        ));
    }
    let mut restored: ReviewUnits = serde_json::from_slice(&fs::read(&backup)?)?;
    restored.projection_revision = expected_revision + 1;
    validate(snapshot, &restored)?;
    atomic_write(&path(snapshot), &serde_json::to_vec_pretty(&restored)?)?;
    Ok(restored)
}

pub fn project_graph(
    mut graph: ChangeGraph,
    projection: Option<&ReviewUnits>,
) -> Result<ChangeGraph> {
    let Some(projection) = projection.filter(|projection| projection.active) else {
        return Ok(graph);
    };
    if projection.graph_revision != graph.revision.as_str() {
        return Err(invalid(
            "stale_projection_graph",
            "review-unit projection targets another graph revision",
        ));
    }
    if projection.units.is_empty() {
        return Ok(graph);
    }
    let mut new_nodes = Vec::new();
    let mut new_edges = Vec::new();
    for unit in &projection.units {
        let parent = graph.nodes.get(&unit.parent_hunk_id).ok_or_else(|| {
            invalid(
                "review_unit_parent_missing",
                format!("parent {} is absent", unit.parent_hunk_id),
            )
        })?;
        let mut locations = Vec::new();
        for parent_location in &parent.locations {
            let range = match parent_location.side {
                SourceSide::Left => unit.old_range,
                SourceSide::Right => unit.new_range,
            };
            if let Some(range) = range {
                locations.push(SourceLocation {
                    side: parent_location.side,
                    path: parent_location.path.clone(),
                    range,
                });
            }
        }
        new_nodes.push((
            unit.id.clone(),
            GraphNode {
                id: unit.id.clone(),
                kind: NodeKind::ReviewUnit,
                name: unit.title.clone(),
                symbol_kind: Some("review_unit".into()),
                changed: true,
                locations,
                selection_range: None,
                hunk_ids: vec![unit.parent_hunk_id.clone()],
            },
        ));
        new_edges.push((unit.parent_hunk_id.clone(), unit.id.clone()));
    }
    for (id, node) in new_nodes {
        graph.nodes.insert(id, node);
    }
    for (parent, child) in new_edges {
        graph.add_edge(
            parent,
            child,
            EdgeKind::Contains,
            "review-units",
            "partition",
            1.0,
            None,
        );
    }
    let raw = format!(
        "{}:{}:{}",
        graph.revision, projection.projection_revision, projection.config_digest
    );
    let digest = hex::encode(Sha256::digest(raw.as_bytes()));
    graph.revision = GraphRevisionId::parse(format!("grf_{}", &digest[..24]))
        .map_err(|message| invalid("invalid_projection_revision", message))?;
    Ok(graph)
}

pub fn active_leaf_ids(graph: &ChangeGraph) -> BTreeSet<String> {
    let split_parents: BTreeSet<&str> = graph
        .nodes
        .values()
        .filter(|node| node.kind == NodeKind::ReviewUnit)
        .flat_map(|node| node.hunk_ids.iter().map(String::as_str))
        .collect();
    graph
        .nodes
        .values()
        .filter(|node| {
            node.kind == NodeKind::ReviewUnit
                || node.kind == NodeKind::FileChange
                || (node.kind == NodeKind::Hunk && !split_parents.contains(node.id.as_str()))
        })
        .map(|node| node.id.clone())
        .collect()
}

pub fn parent_hunk_id<'a>(graph: &'a ChangeGraph, id: &'a str) -> Option<&'a str> {
    let node = graph.nodes.get(id)?;
    if node.kind == NodeKind::ReviewUnit {
        node.hunk_ids.first().map(String::as_str)
    } else if node.kind == NodeKind::Hunk {
        Some(id)
    } else {
        None
    }
}

pub fn find<'a>(projection: Option<&'a ReviewUnits>, id: &str) -> Option<&'a ReviewUnit> {
    projection?.units.iter().find(|unit| unit.id == id)
}

pub fn unit_patch(snapshot: &Snapshot, projection: &ReviewUnits, id: &str) -> Option<String> {
    unit_patch_with_coordinates(snapshot, projection, id).map(|(patch, _, _)| patch)
}

pub fn unit_patch_with_coordinates(
    snapshot: &Snapshot,
    projection: &ReviewUnits,
    id: &str,
) -> Option<(String, u32, u32)> {
    let unit = find(Some(projection), id)?;
    let hunk = snapshot
        .files
        .iter()
        .flat_map(|file| &file.hunks)
        .find(|hunk| hunk.id == unit.parent_hunk_id)?;
    let lines: Vec<_> = hunk.patch.lines().collect();
    let mut old = hunk.old_start;
    let mut new = hunk.new_start;
    for line in lines.iter().take(unit.context_patch_start).skip(1) {
        match line.as_bytes().first().copied() {
            Some(b'-') => old += 1,
            Some(b'+') => new += 1,
            Some(b' ') => {
                old += 1;
                new += 1;
            }
            _ => {}
        }
    }
    let mut result = String::new();
    result.push_str(&hunk.header);
    result.push('\n');
    for row in unit.context_patch_start.max(1)..unit.context_patch_end {
        if let Some(line) = lines.get(row) {
            result.push_str(line);
            result.push('\n');
        }
    }
    Some((result, old, new))
}

fn validate(snapshot: &Snapshot, units: &ReviewUnits) -> Result<()> {
    if units.schema_version != REVIEW_UNITS_SCHEMA_VERSION
        || units.snapshot_id != snapshot.id.as_str()
    {
        return Err(invalid(
            "review_units_mismatch",
            "review units have an unsupported schema or snapshot",
        ));
    }
    let hunks: BTreeSet<&str> = snapshot
        .files
        .iter()
        .flat_map(|file| file.hunks.iter().map(|h| h.id.as_str()))
        .collect();
    let mut ids = BTreeSet::new();
    let mut ownership: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    for unit in &units.units {
        if !hunks.contains(unit.parent_hunk_id.as_str())
            || !ids.insert(unit.id.as_str())
            || unit.changed_lines != unit.owned_rows.len()
        {
            return Err(invalid(
                "invalid_review_units",
                "review unit identity, parent, or row count is invalid",
            ));
        }
        for row in &unit.owned_rows {
            if !ownership
                .entry(&unit.parent_hunk_id)
                .or_default()
                .insert(row.patch_row)
            {
                return Err(invalid(
                    "duplicate_review_unit_row",
                    "a changed patch row has multiple owners",
                ));
            }
        }
    }
    for file in &snapshot.files {
        for hunk in &file.hunks {
            if let Some(owned) = ownership.get(hunk.id.as_str()) {
                let expected: BTreeSet<_> = changed_rows(hunk)
                    .into_iter()
                    .map(|row| row.patch_row)
                    .collect();
                if *owned != expected {
                    return Err(invalid(
                        "incomplete_review_unit_rows",
                        format!("review units do not exactly cover {}", hunk.id),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn changed_rows(hunk: &Hunk) -> Vec<PatchRow> {
    let mut old_line = hunk.old_start.saturating_sub(1);
    let mut new_line = hunk.new_start.saturating_sub(1);
    let mut rows = Vec::new();
    for (patch_row, line) in hunk.patch.lines().enumerate().skip(1) {
        match line.as_bytes().first().copied() {
            Some(b'-') => {
                rows.push(PatchRow {
                    patch_row,
                    side: SourceSide::Left,
                    line: old_line,
                });
                old_line += 1;
            }
            Some(b'+') => {
                rows.push(PatchRow {
                    patch_row,
                    side: SourceSide::Right,
                    line: new_line,
                });
                new_line += 1;
            }
            Some(b' ') => {
                old_line += 1;
                new_line += 1;
            }
            _ => {}
        }
    }
    rows
}

fn semantic_boundaries(
    graph: &ChangeGraph,
    file: &FileChange,
    hunk: &Hunk,
    rows: &[PatchRow],
) -> BTreeSet<usize> {
    let mut result = BTreeSet::new();
    for node in graph
        .nodes
        .values()
        .filter(|node| node.kind == NodeKind::Symbol && node.hunk_ids.contains(&hunk.id))
    {
        for location in &node.locations {
            let path_matches = match location.side {
                SourceSide::Left => file.old_path.as_ref(),
                SourceSide::Right => file.new_path.as_ref(),
            }
            .is_some_and(|path| path.bytes_base64 == location.path.bytes_base64);
            if !path_matches {
                continue;
            }
            if let Some((index, _)) = rows.iter().enumerate().rev().find(|(_, row)| {
                row.side == location.side
                    && row.line >= location.range.start.line
                    && row.line < location.range.end.line
            }) {
                result.insert(index + 1);
            }
        }
    }
    result
}

fn partition_rows<'a>(
    rows: &'a [PatchRow],
    boundaries: &BTreeSet<usize>,
    target: usize,
    ceiling: usize,
) -> Vec<&'a [PatchRow]> {
    let mut result = Vec::new();
    let mut start = 0;
    while rows.len() - start > ceiling {
        let desired = (start + target).min(start + ceiling);
        let minimum = start + target.saturating_div(2).max(1);
        let end = boundaries
            .range(minimum..=start + ceiling)
            .min_by_key(|candidate| candidate.abs_diff(desired))
            .copied()
            .unwrap_or(desired);
        result.push(&rows[start..end]);
        start = end;
    }
    if start < rows.len() {
        result.push(&rows[start..]);
    }
    result
}

fn semantic_title(
    graph: &ChangeGraph,
    file: &FileChange,
    hunk: &Hunk,
    rows: &[PatchRow],
    index: usize,
    parts: usize,
) -> String {
    let middle = rows[rows.len() / 2];
    if let Some(symbol) = graph
        .nodes
        .values()
        .filter(|node| node.kind == NodeKind::Symbol && node.hunk_ids.contains(&hunk.id))
        .filter(|node| {
            node.locations.iter().any(|location| {
                location.side == middle.side
                    && middle.line >= location.range.start.line
                    && middle.line < location.range.end.line
            })
        })
        .min_by_key(|node| {
            node.locations
                .iter()
                .map(|location| {
                    location
                        .range
                        .end
                        .line
                        .saturating_sub(location.range.start.line)
                })
                .min()
                .unwrap_or(u32::MAX)
        })
    {
        return if parts > 1 {
            format!("{} · part {} of {}", symbol.name, index + 1, parts)
        } else {
            symbol.name.clone()
        };
    }
    let name = file
        .new_path
        .as_ref()
        .or(file.old_path.as_ref())
        .and_then(|path| Path::new(&path.display).file_name()?.to_str())
        .unwrap_or("change");
    format!("{name} · part {} of {parts}", index + 1)
}

fn side_range(rows: &[PatchRow], side: SourceSide) -> Option<TextRange> {
    let mut lines = rows
        .iter()
        .filter(|row| row.side == side)
        .map(|row| row.line);
    let first = lines.next()?;
    let (minimum, maximum) = lines.fold((first, first), |(min, max), line| {
        (min.min(line), max.max(line))
    });
    Some(TextRange {
        start: Position {
            line: minimum,
            character: 0,
        },
        end: Position {
            line: maximum + 1,
            character: 0,
        },
    })
}

fn digest_json(value: &impl Serialize) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(value).expect("serializable settings"),
    ))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
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
    use crate::graph::{GraphNode, SourceLocation};
    use crate::snapshot::{GitPath, Hunk};

    fn added_hunk(lines: usize) -> Hunk {
        let body = (0..lines)
            .map(|line| format!("+const value{line} = '😀';\r\n"))
            .collect::<String>();
        Hunk {
            id: "h_large".into(),
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: lines as u32,
            header: format!("@@ -0,0 +1,{lines} @@"),
            patch: format!("@@ -0,0 +1,{lines} @@\n{body}"),
        }
    }

    #[test]
    fn fallback_partition_is_bounded_stable_and_complete() {
        let hunk = added_hunk(300);
        let rows = changed_rows(&hunk);
        let groups = partition_rows(&rows, &BTreeSet::new(), 80, 120);
        assert_eq!(groups.iter().map(|group| group.len()).sum::<usize>(), 300);
        assert!(groups.iter().all(|group| group.len() <= 120));
        let owned: BTreeSet<_> = groups
            .iter()
            .flat_map(|group| group.iter().map(|row| row.patch_row))
            .collect();
        assert_eq!(owned.len(), 300);
    }

    #[test]
    fn oversized_symbol_uses_semantic_titles_with_stable_complete_units() {
        let hunk = added_hunk(300);
        let path = GitPath::from_bytes(b"src/large.tsx".to_vec());
        let snapshot = snapshot_with_file(FileChange {
            status: "A".into(),
            old_path: None,
            new_path: Some(path.clone()),
            old_mode: String::new(),
            new_mode: "100644".into(),
            old_object: String::new(),
            new_object: "new".into(),
            before_blob: None,
            after_blob: None,
            binary: false,
            submodule: false,
            hunks: vec![hunk],
        });
        let mut graph = ChangeGraph::from_snapshot(&snapshot);
        graph.nodes.insert(
            "symbol_large".into(),
            GraphNode {
                id: "symbol_large".into(),
                kind: NodeKind::Symbol,
                name: "renderLargeEditor".into(),
                symbol_kind: Some("function".into()),
                changed: true,
                locations: vec![SourceLocation {
                    side: SourceSide::Right,
                    path,
                    range: TextRange {
                        start: Position {
                            line: 0,
                            character: 0,
                        },
                        end: Position {
                            line: 300,
                            character: 0,
                        },
                    },
                }],
                selection_range: None,
                hunk_ids: vec!["h_large".into()],
            },
        );
        let config = ReviewUnitSettings::default();
        let first = build(&snapshot, &graph, &config);
        let second = build(&snapshot, &graph, &config);
        assert_eq!(first.units, second.units);
        assert_eq!(
            first
                .units
                .iter()
                .map(|unit| unit.changed_lines)
                .sum::<usize>(),
            300
        );
        assert!(first.units.iter().all(|unit| unit.changed_lines <= 120));
        assert!(
            first
                .units
                .iter()
                .all(|unit| unit.title.starts_with("renderLargeEditor · part "))
        );
    }

    #[test]
    fn replacement_and_deletion_keep_exact_sides() {
        let hunk = Hunk {
            id: "h_replace".into(),
            old_start: 4,
            old_count: 2,
            new_start: 4,
            new_count: 1,
            header: "@@ -4,2 +4 @@".into(),
            patch: "@@ -4,2 +4 @@\n-old\n-away\n+new\n".into(),
        };
        let rows = changed_rows(&hunk);
        assert_eq!(
            rows.iter()
                .filter(|row| row.side == SourceSide::Left)
                .count(),
            2
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row.side == SourceSide::Right)
                .count(),
            1
        );
        assert_eq!(side_range(&rows, SourceSide::Left).unwrap().start.line, 3);
        assert_eq!(side_range(&rows, SourceSide::Right).unwrap().start.line, 3);
    }

    #[test]
    fn metadata_change_has_no_review_units() {
        let snapshot = snapshot_with_file(FileChange {
            status: "M".into(),
            old_path: Some(GitPath::from_bytes(b"mode.ts".to_vec())),
            new_path: Some(GitPath::from_bytes(b"mode.ts".to_vec())),
            old_mode: "100644".into(),
            new_mode: "100755".into(),
            old_object: String::new(),
            new_object: String::new(),
            before_blob: None,
            after_blob: None,
            binary: false,
            submodule: false,
            hunks: vec![],
        });
        let graph = ChangeGraph::from_snapshot(&snapshot);
        assert!(
            build(&snapshot, &graph, &ReviewUnitSettings::default())
                .units
                .is_empty()
        );
    }

    #[test]
    fn renamed_deletion_units_keep_old_side_path_and_raw_parent_patch() {
        let body = (0..130)
            .map(|line| format!("-removed_{line}\n"))
            .collect::<String>();
        let snapshot = snapshot_with_file(FileChange {
            status: "R100".into(),
            old_path: Some(GitPath::from_bytes(b"src/old-name.ts".to_vec())),
            new_path: Some(GitPath::from_bytes(b"src/new-name.ts".to_vec())),
            old_mode: "100644".into(),
            new_mode: "100644".into(),
            old_object: "old".into(),
            new_object: "new".into(),
            before_blob: None,
            after_blob: None,
            binary: false,
            submodule: false,
            hunks: vec![Hunk {
                id: "h_rename_delete".into(),
                old_start: 1,
                old_count: 130,
                new_start: 1,
                new_count: 0,
                header: "@@ -1,130 +1,0 @@".into(),
                patch: format!("@@ -1,130 +1,0 @@\n{body}"),
            }],
        });
        let graph = ChangeGraph::from_snapshot(&snapshot);
        let projection = build(&snapshot, &graph, &ReviewUnitSettings::default());
        assert!(projection.units.len() > 1);
        let projected = project_graph(graph, Some(&projection)).unwrap();
        for unit in &projection.units {
            assert!(unit.new_range.is_none());
            let node = &projected.nodes[&unit.id];
            let location = node.preferred_review_location().unwrap();
            assert_eq!(location.side, SourceSide::Left);
            assert_eq!(location.path.display, "src/old-name.ts");
            assert!(
                unit_patch(&snapshot, &projection, &unit.id)
                    .unwrap()
                    .starts_with("@@ -1,130 +1,0 @@")
            );
        }
        assert!(projected.nodes.contains_key("h_rename_delete"));
    }

    #[test]
    fn empty_projection_preserves_raw_graph_revision_and_contract() {
        let snapshot = snapshot_with_file(FileChange {
            status: "M".into(),
            old_path: Some(GitPath::from_bytes(b"src/small.ts".to_vec())),
            new_path: Some(GitPath::from_bytes(b"src/small.ts".to_vec())),
            old_mode: "100644".into(),
            new_mode: "100644".into(),
            old_object: "old".into(),
            new_object: "new".into(),
            before_blob: None,
            after_blob: None,
            binary: false,
            submodule: false,
            hunks: vec![Hunk {
                id: "h_small".into(),
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                header: "@@ -1 +1 @@".into(),
                patch: "@@ -1 +1 @@\n-old\n+new\n".into(),
            }],
        });
        let graph = ChangeGraph::from_snapshot(&snapshot);
        let revision = graph.revision.clone();
        let projection = build(&snapshot, &graph, &ReviewUnitSettings::default());
        assert!(projection.units.is_empty());
        let projected = project_graph(graph, Some(&projection)).unwrap();
        assert_eq!(projected.revision, revision);
        assert!(projected.nodes.contains_key("h_small"));
        assert!(
            projected
                .nodes
                .values()
                .all(|node| node.kind != NodeKind::ReviewUnit)
        );
    }

    fn snapshot_with_file(file: FileChange) -> Snapshot {
        Snapshot {
            id: crate::model::SnapshotId::new(),
            repository: PathBuf::from("."),
            input: crate::snapshot::SnapshotInput::Uncommitted,
            original_base: String::new(),
            original_head: String::new(),
            comparison_base: String::new(),
            before_commit: String::new(),
            after_commit: String::new(),
            captured_at: chrono::Utc::now(),
            source_fingerprint: String::new(),
            files: vec![file],
            storage_dir: PathBuf::new(),
        }
    }
}

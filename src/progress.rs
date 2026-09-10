use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::graph::{ChangeGraph, EdgeKind, NodeKind};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    #[default]
    Unreviewed,
    Reviewed,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ReviewProgress {
    pub revision: u64,
    pub selected: Option<String>,
    pub statuses: BTreeMap<String, ReviewStatus>,
}

impl ReviewProgress {
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            Ok(serde_json::from_slice(&fs::read(path)?)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save_checked(&mut self, path: &Path, expected_revision: u64) -> Result<()> {
        let actual = Self::load(path)?.revision;
        if actual != expected_revision {
            return Err(AppError::RevisionConflict {
                expected: expected_revision as i64,
                actual: actual as i64,
            });
        }
        self.revision = expected_revision + 1;
        let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, path)?;
        Ok(())
    }

    pub fn status(&self, id: &str) -> ReviewStatus {
        self.statuses.get(id).copied().unwrap_or_default()
    }

    pub fn status_in_graph(&self, graph: &ChangeGraph, id: &str) -> ReviewStatus {
        let children = graph
            .edges
            .iter()
            .filter(|edge| {
                edge.from == id
                    && edge.kind == EdgeKind::Contains
                    && graph
                        .nodes
                        .get(&edge.to)
                        .is_some_and(|node| node.kind == NodeKind::ReviewUnit)
            })
            .map(|edge| edge.to.as_str())
            .collect::<Vec<_>>();
        if children.is_empty() {
            self.status(id)
        } else if children
            .iter()
            .all(|child| self.status(child) == ReviewStatus::Reviewed)
        {
            ReviewStatus::Reviewed
        } else {
            ReviewStatus::Unreviewed
        }
    }

    pub fn set_status_in_graph(&mut self, graph: &ChangeGraph, id: &str, status: ReviewStatus) {
        let children = graph
            .edges
            .iter()
            .filter(|edge| {
                edge.from == id
                    && edge.kind == EdgeKind::Contains
                    && graph
                        .nodes
                        .get(&edge.to)
                        .is_some_and(|node| node.kind == NodeKind::ReviewUnit)
            })
            .map(|edge| edge.to.clone())
            .collect::<Vec<_>>();
        if children.is_empty() {
            self.set_status(id.to_owned(), status);
        } else {
            for child in children {
                self.set_status(child, status);
            }
        }
    }

    pub fn set_status(&mut self, id: String, status: ReviewStatus) {
        self.statuses.insert(id, status);
    }

    pub fn carry(&self, mappings: &[(String, String)]) -> Self {
        let lookup: BTreeMap<_, _> = mappings.iter().cloned().collect();
        let statuses = self
            .statuses
            .iter()
            .filter_map(|(old, status)| lookup.get(old).map(|new| (new.clone(), *status)))
            .collect();
        Self {
            revision: 0,
            selected: self
                .selected
                .as_ref()
                .and_then(|old| lookup.get(old).cloned()),
            statuses,
        }
    }
}

pub fn progress_path(snapshot_dir: &Path) -> PathBuf {
    snapshot_dir.join("progress.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{GraphEdge, GraphNode};
    use crate::model::GraphRevisionId;
    use std::collections::BTreeMap;

    #[test]
    fn stale_writer_cannot_overwrite_progress() {
        let root = tempfile::TempDir::new().unwrap();
        let path = root.path().join("progress.json");
        let mut first = ReviewProgress::default();
        let mut second = ReviewProgress::default();
        first.selected = Some("h1".into());
        first.save_checked(&path, 0).unwrap();
        second.selected = Some("h2".into());
        assert!(second.save_checked(&path, 0).is_err());
        assert_eq!(
            ReviewProgress::load(&path).unwrap().selected.as_deref(),
            Some("h1")
        );
    }

    #[test]
    fn parent_status_is_derived_from_all_review_unit_children() {
        let mut nodes = BTreeMap::new();
        for (id, kind) in [
            ("parent", NodeKind::Hunk),
            ("unit-1", NodeKind::ReviewUnit),
            ("unit-2", NodeKind::ReviewUnit),
        ] {
            nodes.insert(
                id.into(),
                GraphNode {
                    id: id.into(),
                    kind,
                    name: id.into(),
                    symbol_kind: None,
                    changed: true,
                    locations: vec![],
                    selection_range: None,
                    hunk_ids: vec![],
                },
            );
        }
        let mut edges = Vec::new();
        for (index, child) in ["unit-1", "unit-2"].into_iter().enumerate() {
            edges.push(GraphEdge {
                id: format!("edge-{index}"),
                from: "parent".into(),
                to: child.into(),
                kind: EdgeKind::Contains,
                producer: "test".into(),
                evidence_kind: "exact".into(),
                confidence: 1.0,
                location: None,
            });
        }
        let graph = ChangeGraph {
            snapshot_id: "snp_test".into(),
            revision: GraphRevisionId::new(),
            fingerprint: "test".into(),
            nodes,
            edges,
            coverage: Vec::new(),
            unfinished_frontier: Vec::new(),
        };
        let mut progress = ReviewProgress::default();
        progress.set_status("unit-1".into(), ReviewStatus::Reviewed);
        assert_eq!(
            progress.status_in_graph(&graph, "parent"),
            ReviewStatus::Unreviewed
        );
        progress.set_status_in_graph(&graph, "parent", ReviewStatus::Reviewed);
        assert_eq!(
            progress.status_in_graph(&graph, "parent"),
            ReviewStatus::Reviewed
        );
        assert_eq!(progress.status("unit-2"), ReviewStatus::Reviewed);
    }
}

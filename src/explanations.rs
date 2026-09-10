use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::context::ContextBundle;
use crate::error::{AppError, Result};
use crate::graph::{ChangeGraph, SourceSide};
use crate::review_units;
use crate::snapshot::Snapshot;

pub const EXPLANATIONS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExplanationAnnotation {
    pub side: SourceSide,
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExplanationUpdate {
    pub item_id: String,
    pub text: String,
    #[serde(default)]
    pub annotations: Vec<ExplanationAnnotation>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Explanation {
    pub item_id: String,
    pub text: String,
    pub annotations: Vec<ExplanationAnnotation>,
    pub evidence_ids: Vec<String>,
    pub authority: String,
    pub created_at: DateTime<Utc>,
    pub input_digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExplanationStore {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub graph_revision: String,
    pub projection_revision: u64,
    pub context_digest: Option<String>,
    pub test_evidence_digest: Option<String>,
    pub revision: u64,
    #[serde(default)]
    pub entries: BTreeMap<String, Explanation>,
    #[serde(default)]
    pub manual_notes: BTreeMap<String, String>,
}

impl ExplanationStore {
    pub fn new(
        snapshot: &Snapshot,
        graph: &ChangeGraph,
        context_digest: Option<String>,
        test_evidence_digest: Option<String>,
    ) -> Self {
        Self {
            schema_version: EXPLANATIONS_SCHEMA_VERSION,
            snapshot_id: snapshot.id.to_string(),
            graph_revision: graph.revision.to_string(),
            projection_revision: review_units::load(snapshot)
                .ok()
                .flatten()
                .map_or(0, |projection| projection.projection_revision),
            context_digest,
            test_evidence_digest,
            revision: 0,
            entries: BTreeMap::new(),
            manual_notes: BTreeMap::new(),
        }
    }

    pub fn load_or_new(
        path: &Path,
        snapshot: &Snapshot,
        graph: &ChangeGraph,
        context_digest: Option<String>,
        test_evidence_digest: Option<String>,
    ) -> Result<Self> {
        if path.is_file() {
            let store: Self = serde_json::from_slice(&fs::read(path)?)?;
            if store.schema_version != EXPLANATIONS_SCHEMA_VERSION
                || store.snapshot_id != snapshot.id.as_str()
            {
                return Err(invalid(
                    "explanations_snapshot_mismatch",
                    "explanations use an unsupported schema or another snapshot",
                ));
            }
            Ok(store)
        } else {
            Ok(Self::new(
                snapshot,
                graph,
                context_digest,
                test_evidence_digest,
            ))
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_batch(
        &mut self,
        snapshot: &Snapshot,
        graph: &ChangeGraph,
        context: &ContextBundle,
        extra_evidence: &BTreeSet<String>,
        expected_revision: u64,
        expected_graph_revision: &str,
        updates: Vec<ExplanationUpdate>,
    ) -> Result<()> {
        if self.revision != expected_revision {
            return Err(AppError::RevisionConflict {
                expected: expected_revision as i64,
                actual: self.revision as i64,
            });
        }
        if graph.revision.as_str() != expected_graph_revision {
            return Err(invalid(
                "stale_graph_revision",
                format!(
                    "explanation batch targets {expected_graph_revision}, current graph is {}",
                    graph.revision
                ),
            ));
        }
        let graph_evidence: BTreeSet<&str> = graph
            .nodes
            .keys()
            .map(String::as_str)
            .chain(graph.edges.iter().map(|edge| edge.id.as_str()))
            .chain(context.entries.iter().map(|entry| entry.id.as_str()))
            .collect();
        for update in &updates {
            if !graph.nodes.contains_key(&update.item_id) {
                return Err(invalid(
                    "unknown_explanation_item",
                    format!("item {} does not exist", update.item_id),
                ));
            }
            validate_text(&update.item_id, &update.text, 4 * 1024)?;
            if update.annotations.len() > 8 {
                return Err(invalid(
                    "too_many_annotations",
                    format!("item {} has more than 8 annotations", update.item_id),
                ));
            }
            for evidence in &update.evidence_ids {
                if !graph_evidence.contains(evidence.as_str()) && !extra_evidence.contains(evidence)
                {
                    return Err(invalid(
                        "unknown_explanation_evidence",
                        format!("evidence {evidence} does not exist"),
                    ));
                }
            }
            for annotation in &update.annotations {
                validate_annotation(snapshot, annotation)?;
            }
        }
        let input_digest = input_digest(
            snapshot,
            graph,
            self.context_digest.as_deref(),
            self.test_evidence_digest.as_deref(),
        );
        for update in updates {
            self.entries.insert(
                update.item_id.clone(),
                Explanation {
                    item_id: update.item_id,
                    text: update.text,
                    annotations: update.annotations,
                    evidence_ids: update.evidence_ids,
                    authority: "model_interpretation".into(),
                    created_at: Utc::now(),
                    input_digest: input_digest.clone(),
                },
            );
        }
        self.graph_revision = graph.revision.to_string();
        self.revision += 1;
        Ok(())
    }

    pub fn set_manual_note(
        &mut self,
        graph: &ChangeGraph,
        item_id: &str,
        note: String,
        expected_revision: u64,
    ) -> Result<()> {
        if self.revision != expected_revision {
            return Err(AppError::RevisionConflict {
                expected: expected_revision as i64,
                actual: self.revision as i64,
            });
        }
        if !graph.nodes.contains_key(item_id) {
            return Err(invalid(
                "unknown_explanation_item",
                "manual note item does not exist",
            ));
        }
        validate_text(item_id, &note, 4 * 1024)?;
        self.manual_notes.insert(item_id.into(), note);
        self.revision += 1;
        Ok(())
    }

    pub fn is_stale(
        &self,
        snapshot: &Snapshot,
        graph: &ChangeGraph,
        context_digest: Option<&str>,
        test_evidence_digest: Option<&str>,
    ) -> bool {
        self.entries.values().any(|entry| {
            entry.input_digest
                != input_digest(snapshot, graph, context_digest, test_evidence_digest)
        })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, path)?;
        Ok(())
    }
}

pub fn path(snapshot: &Snapshot) -> PathBuf {
    snapshot.storage_dir.join("explanations.json")
}

fn validate_annotation(snapshot: &Snapshot, annotation: &ExplanationAnnotation) -> Result<()> {
    if annotation.start_line == 0
        || annotation.end_line < annotation.start_line
        || annotation.label.len() > 512
        || annotation
            .label
            .chars()
            .any(|character| character.is_control() && character != '\n')
    {
        return Err(invalid(
            "invalid_annotation",
            "annotation range or label is invalid",
        ));
    }
    let side = match annotation.side {
        SourceSide::Left => "before",
        SourceSide::Right => "after",
    };
    let source = snapshot.storage_dir.join(side).join(&annotation.path);
    let bytes = fs::read(&source).map_err(|_| {
        invalid(
            "invalid_annotation_path",
            format!("captured source {} is unavailable", annotation.path),
        )
    })?;
    let lines = bytes.split(|byte| *byte == b'\n').count() as u32;
    if annotation.end_line > lines {
        return Err(invalid(
            "invalid_annotation_range",
            format!("annotation exceeds {} captured lines", lines),
        ));
    }
    Ok(())
}

fn validate_text(item: &str, value: &str, limit: usize) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > limit
        || value.chars().any(|character| character == '\0')
    {
        return Err(invalid(
            "invalid_explanation_text",
            format!("text for {item} must contain 1..{limit} safe UTF-8 bytes"),
        ));
    }
    Ok(())
}

fn input_digest(
    snapshot: &Snapshot,
    graph: &ChangeGraph,
    context_digest: Option<&str>,
    test_evidence_digest: Option<&str>,
) -> String {
    let mut digest = Sha256::new();
    digest.update(snapshot.id.as_str());
    digest.update(graph.revision.as_str());
    digest.update(context_digest.unwrap_or(""));
    digest.update(test_evidence_digest.unwrap_or(""));
    hex::encode(digest.finalize())
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
    use crate::graph::ChangeGraph;
    use crate::snapshot::{GitPath, SnapshotInput};

    #[test]
    fn rejects_atomic_batch_with_invalid_annotation_and_preserves_manual_notes() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("after")).unwrap();
        fs::write(root.path().join("after/a.ts"), "one\ntwo\n").unwrap();
        let snapshot = Snapshot {
            id: crate::model::SnapshotId::new(),
            repository: PathBuf::from("."),
            input: SnapshotInput::Uncommitted,
            original_base: String::new(),
            original_head: String::new(),
            comparison_base: String::new(),
            before_commit: String::new(),
            after_commit: String::new(),
            captured_at: Utc::now(),
            source_fingerprint: String::new(),
            files: vec![crate::snapshot::FileChange {
                status: "M".into(),
                old_path: None,
                new_path: Some(GitPath::from_bytes(b"a.ts".to_vec())),
                old_mode: String::new(),
                new_mode: String::new(),
                old_object: String::new(),
                new_object: String::new(),
                before_blob: None,
                after_blob: None,
                binary: false,
                submodule: false,
                hunks: vec![crate::snapshot::Hunk {
                    id: "h_one".into(),
                    old_start: 0,
                    old_count: 0,
                    new_start: 1,
                    new_count: 2,
                    header: "@@ -0,0 +1,2 @@".into(),
                    patch: "@@ -0,0 +1,2 @@\n+one\n+two\n".into(),
                }],
            }],
            storage_dir: root.path().into(),
        };
        let graph = ChangeGraph::from_snapshot(&snapshot);
        let mut store = ExplanationStore::new(&snapshot, &graph, None, None);
        store.manual_notes.insert("h_one".into(), "keep".into());
        let update = ExplanationUpdate {
            item_id: "h_one".into(),
            text: "Meaning".into(),
            evidence_ids: vec!["h_one".into()],
            annotations: vec![ExplanationAnnotation {
                side: SourceSide::Right,
                path: "a.ts".into(),
                start_line: 1,
                end_line: 99,
                label: "bad".into(),
            }],
        };
        assert!(
            store
                .apply_batch(
                    &snapshot,
                    &graph,
                    &ContextBundle::default(),
                    &BTreeSet::new(),
                    0,
                    graph.revision.as_str(),
                    vec![update]
                )
                .is_err()
        );
        assert_eq!(store.revision, 0);
        assert_eq!(store.manual_notes["h_one"], "keep");
        assert!(store.entries.is_empty());
    }
}

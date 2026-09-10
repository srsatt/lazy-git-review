use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::graph::{ChangeGraph, NodeKind};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Authority {
    Model,
    Manual,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Assessment {
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub score: u8,
    #[serde(default)]
    pub tags: Vec<String>,
    pub rationale: String,
    pub confidence: f32,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default = "default_authority")]
    pub authority: Authority,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleSource {
    Model,
    Manual,
    RationaleFallback,
    StructuralFallback,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HunkLabel {
    pub title: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default = "default_authority")]
    pub authority: Authority,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HunkLabelUpdate {
    pub node_id: String,
    pub title: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default = "default_authority")]
    pub authority: Authority,
}

fn default_authority() -> Authority {
    Authority::Model
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RankingMetrics {
    pub query_count: u64,
    pub returned_bytes: u64,
    pub requested_source_bytes: u64,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RankingState {
    pub graph_revision: String,
    pub context_digest: Option<String>,
    pub assessments: BTreeMap<String, Assessment>,
    #[serde(default)]
    pub labels: BTreeMap<String, HunkLabel>,
    pub finalized: bool,
    pub finalized_at: Option<DateTime<Utc>>,
    pub metrics: RankingMetrics,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QueueItem {
    pub node_id: String,
    pub kind: NodeKind,
    pub name: String,
    pub title: String,
    pub title_source: TitleSource,
    pub path: Option<String>,
    pub score: Option<u8>,
    pub tags: Vec<String>,
    pub rationale: Option<String>,
    pub confidence: Option<f32>,
    pub inherited_from: Option<String>,
    pub assessed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Queue {
    pub graph_revision: String,
    pub stale: bool,
    pub fully_ranked: bool,
    pub finalized: bool,
    pub context_digest: Option<String>,
    pub metrics: RankingMetrics,
    pub assessed_changes: usize,
    pub total_changes: usize,
    pub items: Vec<QueueItem>,
}

impl RankingState {
    pub fn new(graph: &ChangeGraph, context_digest: Option<String>) -> Self {
        Self {
            graph_revision: graph.revision.to_string(),
            context_digest,
            assessments: BTreeMap::new(),
            labels: BTreeMap::new(),
            finalized: false,
            finalized_at: None,
            metrics: RankingMetrics::default(),
        }
    }

    pub fn restarted(&self, graph: &ChangeGraph, context_digest: Option<String>) -> Self {
        let mut next = Self::new(graph, context_digest);
        next.assessments = self
            .assessments
            .iter()
            .filter(|(node_id, assessment)| {
                assessment.authority == Authority::Manual && graph.nodes.contains_key(*node_id)
            })
            .map(|(node_id, assessment)| (node_id.clone(), assessment.clone()))
            .collect();
        next.labels = self
            .labels
            .iter()
            .filter(|(node_id, _)| graph.nodes.contains_key(*node_id))
            .map(|(node_id, label)| (node_id.clone(), label.clone()))
            .collect();
        next
    }

    pub fn apply_batch(
        &mut self,
        graph: &ChangeGraph,
        expected_revision: &str,
        updates: Vec<Assessment>,
    ) -> Result<()> {
        if graph.revision.as_str() != expected_revision || self.graph_revision != expected_revision
        {
            return Err(invalid(
                "stale_graph_revision",
                format!(
                    "score batch targets {expected_revision}, current graph is {}",
                    graph.revision
                ),
            ));
        }
        let edge_ids: BTreeSet<&str> = graph.edges.iter().map(|edge| edge.id.as_str()).collect();
        for update in &updates {
            let Some(node) = graph.nodes.get(&update.node_id) else {
                return Err(invalid(
                    "unknown_score_node",
                    format!("node {} does not exist", update.node_id),
                ));
            };
            if !matches!(
                node.kind,
                NodeKind::Hunk | NodeKind::ReviewUnit | NodeKind::Symbol | NodeKind::FileChange
            ) {
                return Err(invalid(
                    "invalid_score_node",
                    format!("node {} cannot be ranked", update.node_id),
                ));
            }
            if update.score > 100 {
                return Err(invalid(
                    "invalid_score",
                    format!("score for {} must be between 0 and 100", update.node_id),
                ));
            }
            if update.confidence.is_nan() || !(0.0..=1.0).contains(&update.confidence) {
                return Err(invalid(
                    "invalid_confidence",
                    format!("confidence for {} must be between 0 and 1", update.node_id),
                ));
            }
            if update.rationale.trim().is_empty() || update.rationale.chars().count() > 500 {
                return Err(invalid(
                    "invalid_rationale",
                    format!(
                        "rationale for {} must contain 1..500 characters",
                        update.node_id
                    ),
                ));
            }
            if let Some(title) = &update.title {
                if !matches!(node.kind, NodeKind::Hunk | NodeKind::ReviewUnit) {
                    return Err(invalid(
                        "invalid_title_node",
                        format!("semantic title for {} must target a hunk", update.node_id),
                    ));
                }
                validate_title(&update.node_id, title)?;
            }
            if update.tags.len() > 16
                || update
                    .tags
                    .iter()
                    .any(|tag| tag.is_empty() || tag.len() > 48)
            {
                return Err(invalid(
                    "invalid_tags",
                    format!("tags for {} exceed count or length limits", update.node_id),
                ));
            }
            for evidence in &update.evidence_ids {
                if !graph.nodes.contains_key(evidence) && !edge_ids.contains(evidence.as_str()) {
                    return Err(invalid(
                        "unknown_evidence",
                        format!("evidence {evidence} does not exist"),
                    ));
                }
            }
        }
        for update in updates {
            if self
                .assessments
                .get(&update.node_id)
                .is_some_and(|current| current.authority == Authority::Manual)
                && update.authority == Authority::Model
            {
                continue;
            }
            if let Some(title) = &update.title {
                self.labels.insert(
                    update.node_id.clone(),
                    HunkLabel {
                        title: title.trim().to_owned(),
                        evidence_ids: update.evidence_ids.clone(),
                        authority: update.authority.clone(),
                    },
                );
            }
            self.assessments.insert(update.node_id.clone(), update);
        }
        self.finalized = false;
        self.finalized_at = None;
        Ok(())
    }

    pub fn apply_label_batch(
        &mut self,
        graph: &ChangeGraph,
        expected_revision: &str,
        updates: Vec<HunkLabelUpdate>,
    ) -> Result<()> {
        if graph.revision.as_str() != expected_revision || self.graph_revision != expected_revision
        {
            return Err(invalid(
                "stale_graph_revision",
                format!(
                    "label batch targets {expected_revision}, current graph is {}",
                    graph.revision
                ),
            ));
        }
        let edge_ids: BTreeSet<&str> = graph.edges.iter().map(|edge| edge.id.as_str()).collect();
        for update in &updates {
            let Some(node) = graph.nodes.get(&update.node_id) else {
                return Err(invalid(
                    "unknown_label_node",
                    format!("node {} does not exist", update.node_id),
                ));
            };
            if !matches!(node.kind, NodeKind::Hunk | NodeKind::ReviewUnit) {
                return Err(invalid(
                    "invalid_label_node",
                    format!("node {} is not a hunk", update.node_id),
                ));
            }
            validate_title(&update.node_id, &update.title)?;
            for evidence in &update.evidence_ids {
                if !graph.nodes.contains_key(evidence) && !edge_ids.contains(evidence.as_str()) {
                    return Err(invalid(
                        "unknown_evidence",
                        format!("evidence {evidence} does not exist"),
                    ));
                }
            }
        }
        for update in updates {
            if self
                .labels
                .get(&update.node_id)
                .is_some_and(|current| current.authority == Authority::Manual)
                && update.authority == Authority::Model
            {
                continue;
            }
            self.labels.insert(
                update.node_id,
                HunkLabel {
                    title: update.title.trim().to_owned(),
                    evidence_ids: update.evidence_ids,
                    authority: update.authority,
                },
            );
        }
        Ok(())
    }

    pub fn finalize(&mut self, graph: &ChangeGraph) -> Result<Queue> {
        if self.graph_revision != graph.revision.as_str() {
            return Err(invalid(
                "stale_graph_revision",
                "ranking belongs to an older graph revision".into(),
            ));
        }
        self.finalized = true;
        self.finalized_at = Some(Utc::now());
        Ok(self.queue(graph, self.context_digest.as_deref()))
    }

    pub fn queue(&self, graph: &ChangeGraph, current_context_digest: Option<&str>) -> Queue {
        let stale = self.graph_revision != graph.revision.as_str()
            || self.context_digest.as_deref() != current_context_digest;
        let active_leaf_ids = crate::review_units::active_leaf_ids(graph);
        let mut items: Vec<QueueItem> = graph
            .nodes
            .values()
            .filter(|node| active_leaf_ids.contains(&node.id))
            .map(|node| {
                let direct = self.assessments.get(&node.id);
                let parent = (node.kind == NodeKind::ReviewUnit)
                    .then(|| node.hunk_ids.first())
                    .flatten()
                    .and_then(|parent| self.assessments.get_key_value(parent))
                    .map(|(id, assessment)| (id.as_str(), assessment));
                let inherited = if direct.is_none() {
                    parent.or_else(|| {
                        graph
                            .nodes
                            .values()
                            .filter(|candidate| {
                                candidate.kind == NodeKind::Symbol
                                    && (candidate.hunk_ids.contains(&node.id)
                                        || node
                                            .hunk_ids
                                            .iter()
                                            .any(|parent| candidate.hunk_ids.contains(parent)))
                            })
                            .filter_map(|candidate| {
                                self.assessments
                                    .get(&candidate.id)
                                    .map(|assessment| (candidate.id.as_str(), assessment))
                            })
                            .max_by_key(|(_, assessment)| assessment.score)
                    })
                } else {
                    None
                };
                let assessment = direct.or_else(|| inherited.map(|(_, assessment)| assessment));
                let (title, title_source) = self.title_for(node, direct);
                QueueItem {
                    node_id: node.id.clone(),
                    kind: node.kind.clone(),
                    name: node.name.clone(),
                    title,
                    title_source,
                    path: node
                        .locations
                        .first()
                        .map(|location| location.path.display.clone()),
                    score: assessment.map(|assessment| assessment.score),
                    tags: assessment
                        .map(|assessment| assessment.tags.clone())
                        .unwrap_or_default(),
                    rationale: assessment.map(|assessment| assessment.rationale.clone()),
                    confidence: assessment.map(|assessment| assessment.confidence),
                    inherited_from: inherited.map(|(id, _)| id.to_owned()),
                    assessed: assessment.is_some(),
                }
            })
            .collect();
        items.sort_by(|a, b| {
            b.score
                .unwrap_or(0)
                .cmp(&a.score.unwrap_or(0))
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.node_id.cmp(&b.node_id))
        });
        let total_changes = items.len();
        let assessed_changes = items.iter().filter(|item| item.assessed).count();
        Queue {
            graph_revision: graph.revision.to_string(),
            stale,
            fully_ranked: assessed_changes == total_changes,
            finalized: self.finalized,
            context_digest: self.context_digest.clone(),
            metrics: self.metrics.clone(),
            assessed_changes,
            total_changes,
            items,
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let temporary = temporary_path(path);
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, path)?;
        Ok(())
    }

    pub fn load_or_new(
        path: &Path,
        graph: &ChangeGraph,
        context_digest: Option<String>,
    ) -> Result<Self> {
        if path.exists() {
            Ok(serde_json::from_slice(&fs::read(path)?)?)
        } else {
            Ok(Self::new(graph, context_digest))
        }
    }
}

impl RankingState {
    fn title_for(
        &self,
        node: &crate::graph::GraphNode,
        direct: Option<&Assessment>,
    ) -> (String, TitleSource) {
        if let Some(label) = self.labels.get(&node.id) {
            return (
                label.title.clone(),
                match label.authority {
                    Authority::Model => TitleSource::Model,
                    Authority::Manual => TitleSource::Manual,
                },
            );
        }
        if let Some(assessment) = direct
            && let Some(title) = assessment.title.as_deref()
        {
            return (
                title.trim().to_owned(),
                match assessment.authority {
                    Authority::Model => TitleSource::Model,
                    Authority::Manual => TitleSource::Manual,
                },
            );
        }
        if let Some(rationale) = direct.map(|assessment| assessment.rationale.trim()) {
            return (
                shorten_single_line(rationale, 120),
                TitleSource::RationaleFallback,
            );
        }
        let path = node
            .preferred_review_location()
            .map(|location| location.path.display.as_str())
            .unwrap_or("change");
        let basename = Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path);
        let title = node.preferred_review_location().map_or_else(
            || format!("Change {basename}"),
            |location| {
                let start = location.range.start.line + 1;
                let end = location.range.end.line.max(location.range.start.line + 1);
                if end <= start {
                    format!("Change {basename}:{start}")
                } else {
                    format!("Change {basename}:{start}–{end}")
                }
            },
        );
        (title, TitleSource::StructuralFallback)
    }
}

fn validate_title(node_id: &str, title: &str) -> Result<()> {
    let trimmed = title.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 120 || trimmed.chars().any(char::is_control)
    {
        return Err(invalid(
            "invalid_title",
            format!("title for {node_id} must be one line containing 1..120 characters"),
        ));
    }
    Ok(())
}

fn shorten_single_line(value: &str, max: usize) -> String {
    let one_line = value.lines().next().unwrap_or("").trim();
    if one_line.chars().count() <= max {
        return one_line.to_owned();
    }
    let mut result: String = one_line.chars().take(max.saturating_sub(1)).collect();
    result.push('…');
    result
}

pub fn ranking_path(graph_path: &Path) -> PathBuf {
    graph_path.with_file_name("ranking.json")
}

pub struct RankingFileLock {
    file: File,
}

impl RankingFileLock {
    pub fn acquire(ranking_path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(ranking_path.with_file_name("ranking.lock"))?;
        file.lock_exclusive()?;
        Ok(Self { file })
    }
}

impl Drop for RankingFileLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    PathBuf::from(name)
}

fn invalid(code: &'static str, message: String) -> AppError {
    AppError::InvalidInput { code, message }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git;
    use crate::snapshot::{SnapshotInput, capture};
    use tempfile::TempDir;

    fn graph() -> ChangeGraph {
        let repo = TempDir::new().unwrap();
        git::run(repo.path(), &[git::os("init"), git::os("-q")]).unwrap();
        fs::write(repo.path().join("a.ts"), "let x=1;\n").unwrap();
        git::run(repo.path(), &[git::os("add"), git::os(".")]).unwrap();
        git::run(
            repo.path(),
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
        fs::write(repo.path().join("a.ts"), "let x=2;\n").unwrap();
        let data = TempDir::new().unwrap();
        let snapshot = capture(
            repo.path(),
            SnapshotInput::Unstaged {
                include_untracked: false,
            },
            data.path(),
        )
        .unwrap();
        ChangeGraph::from_snapshot(&snapshot)
    }

    #[test]
    fn rejects_entire_invalid_batch_and_orders_queue() {
        let graph = graph();
        let hunk = graph
            .nodes
            .values()
            .find(|node| node.kind == NodeKind::Hunk)
            .unwrap();
        let mut state = RankingState::new(&graph, None);
        let valid = Assessment {
            node_id: hunk.id.clone(),
            title: Some("Change assigned value".into()),
            score: 90,
            tags: vec!["security".into()],
            rationale: "authentication boundary".into(),
            confidence: 0.9,
            evidence_ids: vec![hunk.id.clone()],
            authority: Authority::Model,
        };
        let mut invalid = valid.clone();
        invalid.node_id = "missing".into();
        assert!(
            state
                .apply_batch(
                    &graph,
                    graph.revision.as_str(),
                    vec![valid.clone(), invalid]
                )
                .is_err()
        );
        assert!(state.assessments.is_empty());
        state
            .apply_batch(&graph, graph.revision.as_str(), vec![valid])
            .unwrap();
        assert_eq!(state.queue(&graph, None).items[0].score, Some(90));
    }

    #[test]
    fn old_state_and_untitled_scores_keep_working() {
        let graph = graph();
        let hunk = graph
            .nodes
            .values()
            .find(|node| node.kind == NodeKind::Hunk)
            .unwrap();
        let old = format!(
            r#"{{"graph_revision":"{}","context_digest":null,"assessments":{{}},"finalized":true,"finalized_at":null,"metrics":{{"query_count":0,"returned_bytes":0,"requested_source_bytes":0,"elapsed_ms":0}}}}"#,
            graph.revision
        );
        let loaded: RankingState = serde_json::from_str(&old).unwrap();
        assert!(loaded.labels.is_empty());

        let mut state = RankingState::new(&graph, None);
        state
            .apply_batch(
                &graph,
                graph.revision.as_str(),
                vec![Assessment {
                    node_id: hunk.id.clone(),
                    title: None,
                    score: 50,
                    tags: Vec::new(),
                    rationale: "Adjust assigned value".into(),
                    confidence: 1.0,
                    evidence_ids: Vec::new(),
                    authority: Authority::Model,
                }],
            )
            .unwrap();
        let item = &state.queue(&graph, None).items[0];
        assert_eq!(item.title, "Adjust assigned value");
        assert_eq!(item.title_source, TitleSource::RationaleFallback);
    }

    #[test]
    fn labels_do_not_assess_or_unfinalize_and_manual_labels_win() {
        let graph = graph();
        let hunk = graph
            .nodes
            .values()
            .find(|node| node.kind == NodeKind::Hunk)
            .unwrap();
        let mut state = RankingState::new(&graph, None);
        state.finalized = true;
        state
            .apply_label_batch(
                &graph,
                graph.revision.as_str(),
                vec![HunkLabelUpdate {
                    node_id: hunk.id.clone(),
                    title: "Change assigned value manually".into(),
                    evidence_ids: Vec::new(),
                    authority: Authority::Manual,
                }],
            )
            .unwrap();
        state
            .apply_label_batch(
                &graph,
                graph.revision.as_str(),
                vec![HunkLabelUpdate {
                    node_id: hunk.id.clone(),
                    title: "Overwrite from model".into(),
                    evidence_ids: Vec::new(),
                    authority: Authority::Model,
                }],
            )
            .unwrap();
        let queue = state.queue(&graph, None);
        assert!(!queue.items[0].assessed);
        assert!(queue.finalized);
        assert_eq!(queue.items[0].title, "Change assigned value manually");
        assert_eq!(queue.items[0].title_source, TitleSource::Manual);
    }

    #[test]
    fn invalid_title_rejects_entire_label_batch() {
        let graph = graph();
        let hunk = graph
            .nodes
            .values()
            .find(|node| node.kind == NodeKind::Hunk)
            .unwrap();
        let mut state = RankingState::new(&graph, None);
        let result = state.apply_label_batch(
            &graph,
            graph.revision.as_str(),
            vec![
                HunkLabelUpdate {
                    node_id: hunk.id.clone(),
                    title: "Useful title".into(),
                    evidence_ids: Vec::new(),
                    authority: Authority::Model,
                },
                HunkLabelUpdate {
                    node_id: hunk.id.clone(),
                    title: "unsafe\nlabel".into(),
                    evidence_ids: Vec::new(),
                    authority: Authority::Model,
                },
            ],
        );
        assert!(result.is_err());
        assert!(state.labels.is_empty());
    }
}

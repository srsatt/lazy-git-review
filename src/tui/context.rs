use crate::comments::{DraftStore, comments_path};
use crate::context::{CaptureStatus, ContextBundle};
use crate::explanations::{ExplanationStore, path as explanations_path};
use crate::graph::{ChangeGraph, GraphNode, SourceSide};
use crate::snapshot::Snapshot;
use crate::test_evidence::{AttributionGranularity, TestEvidenceStore};

const MAX_CONTEXT_LINES: usize = 240;
const MAX_EXCERPT_LINES: usize = 8;

pub(super) struct ReviewContext {
    bundle: ContextBundle,
    explanations: ExplanationStore,
    drafts: DraftStore,
    tests: TestEvidenceStore,
    explanations_stale: bool,
    unavailable: Vec<String>,
}

impl ReviewContext {
    pub(super) fn load(snapshot: &Snapshot, graph: &ChangeGraph) -> Self {
        let mut unavailable = Vec::new();
        let bundle = ContextBundle::load(&snapshot.storage_dir.join("context.json"))
            .unwrap_or_else(|error| {
                unavailable.push(format!("Context unavailable: {error}"));
                ContextBundle::default()
            });
        let tests = TestEvidenceStore::load(snapshot).unwrap_or_else(|error| {
            unavailable.push(format!("Test evidence unavailable: {error}"));
            TestEvidenceStore {
                schema_version: crate::test_evidence::TEST_EVIDENCE_SCHEMA_VERSION,
                snapshot_id: snapshot.id.to_string(),
                revision: 0,
                digest: String::new(),
                runs: Vec::new(),
            }
        });
        let context_digest = (!bundle.digest.is_empty()).then(|| bundle.digest.clone());
        let test_digest = (!tests.digest.is_empty()).then(|| tests.digest.clone());
        let explanations = ExplanationStore::load_or_new(
            &explanations_path(snapshot),
            snapshot,
            graph,
            context_digest.clone(),
            test_digest.clone(),
        )
        .unwrap_or_else(|error| {
            unavailable.push(format!("Explanations unavailable: {error}"));
            ExplanationStore::new(snapshot, graph, context_digest.clone(), test_digest.clone())
        });
        let explanations_stale = explanations.is_stale(
            snapshot,
            graph,
            context_digest.as_deref(),
            test_digest.as_deref(),
        );
        let drafts = DraftStore::load(&comments_path(snapshot)).unwrap_or_else(|error| {
            unavailable.push(format!("Draft comments unavailable: {error}"));
            DraftStore::default()
        });
        Self {
            bundle,
            explanations,
            drafts,
            tests,
            explanations_stale,
            unavailable,
        }
    }

    pub(super) fn item_lines(&self, id: &str, node: Option<&GraphNode>) -> Vec<String> {
        let Some(node) = node else {
            return vec!["Selected review item is unavailable.".into()];
        };
        let mut lines = Vec::new();
        let explanation = self.explanations.entries.get(id).or_else(|| {
            node.hunk_ids
                .iter()
                .find_map(|parent| self.explanations.entries.get(parent))
        });
        if let Some(explanation) = explanation {
            section(
                &mut lines,
                if self.explanations_stale {
                    "What and why · model interpretation · stale"
                } else {
                    "What and why · model interpretation"
                },
            );
            lines.extend(explanation.text.lines().map(str::to_owned));
            if !explanation.evidence_ids.is_empty() {
                lines.push(format!("Evidence: {}", explanation.evidence_ids.join(", ")));
            }
        }
        if let Some(note) = self.explanations.manual_notes.get(id).or_else(|| {
            node.hunk_ids
                .iter()
                .find_map(|parent| self.explanations.manual_notes.get(parent))
        }) {
            section(&mut lines, "Manual note");
            lines.extend(note.lines().map(str::to_owned));
        }
        let comments: Vec<_> = self
            .drafts
            .drafts
            .values()
            .filter(|draft| {
                draft.anchor.as_ref().is_some_and(|anchor| {
                    node.locations.iter().any(|location| {
                        anchor.snapshot_id == self.explanations.snapshot_id
                            && anchor.side == location.side
                            && anchor.path.bytes_base64 == location.path.bytes_base64
                            && anchor.start_line.saturating_sub(1) < location.range.end.line
                            && location.range.start.line < anchor.end_line
                    })
                })
            })
            .collect();
        if !comments.is_empty() {
            section(&mut lines, "Draft comments");
            for comment in comments {
                lines.push(format!(
                    "{}{}",
                    if comment.stale { "[stale] " } else { "" },
                    comment.body
                ));
            }
        }
        let tests = self.tests_for(node);
        if !tests.is_empty() {
            section(
                &mut lines,
                "Tests · execution evidence, not assertion proof",
            );
            lines.extend(tests);
        }
        let sources: Vec<_> = self
            .bundle
            .entries
            .iter()
            .filter(|entry| {
                entry.anchor.as_ref().is_some_and(|anchor| {
                    node.locations.iter().any(|location| {
                        anchor.snapshot_id == self.explanations.snapshot_id
                            && anchor.side == location.side
                            && anchor.path.bytes_base64 == location.path.bytes_base64
                            && anchor.start_line.saturating_sub(1) < location.range.end.line
                            && location.range.start.line < anchor.end_line
                    })
                })
            })
            .collect();
        if !sources.is_empty() {
            section(&mut lines, "Sources");
            for entry in sources {
                lines.push(source_heading(entry));
                lines.extend(
                    entry
                        .markdown
                        .lines()
                        .take(MAX_EXCERPT_LINES)
                        .map(str::to_owned),
                );
                if entry.markdown.lines().count() > MAX_EXCERPT_LINES {
                    lines.push("…".into());
                }
            }
        }
        lines.extend(self.unavailable.iter().cloned());
        if lines.is_empty() {
            lines.push("No context for this item.".into());
            lines.push("Run `lgr explain SESSION` or press I for session context.".into());
        }
        lines.truncate(MAX_CONTEXT_LINES);
        lines
    }

    pub(super) fn session_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        section(&mut lines, "Session context");
        for entry in &self.bundle.entries {
            lines.push(source_heading(entry));
            lines.extend(
                entry
                    .markdown
                    .lines()
                    .take(MAX_EXCERPT_LINES)
                    .map(str::to_owned),
            );
            if entry.markdown.lines().count() > MAX_EXCERPT_LINES {
                lines.push("…".into());
            }
            lines.push(String::new());
            if lines.len() >= MAX_CONTEXT_LINES {
                break;
            }
        }
        if self.bundle.entries.is_empty() {
            lines.push("No captured session context. Add Markdown with `lgr context add`.".into());
        }
        lines.extend(self.unavailable.iter().cloned());
        lines.truncate(MAX_CONTEXT_LINES);
        lines
    }

    pub(super) fn annotation_for(
        &self,
        id: &str,
        node: Option<&GraphNode>,
        side: SourceSide,
        line: u32,
    ) -> Option<&str> {
        let node = node?;
        let explanation = self.explanations.entries.get(id).or_else(|| {
            node.hunk_ids
                .iter()
                .find_map(|parent| self.explanations.entries.get(parent))
        })?;
        explanation
            .annotations
            .iter()
            .find(|annotation| {
                annotation.side == side
                    && line >= annotation.start_line
                    && line <= annotation.end_line
                    && node.locations.iter().any(|location| {
                        location.side == side && location.path.display == annotation.path
                    })
            })
            .map(|annotation| annotation.label.as_str())
    }

    fn tests_for(&self, node: &GraphNode) -> Vec<String> {
        let mut result = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for run in &self.tests.runs {
            for evidence in &run.evidence {
                if evidence.test.granularity == AttributionGranularity::Suite {
                    continue;
                }
                let intersects = evidence.ranges.iter().any(|range| {
                    range.compatible
                        && range.hits > 0
                        && node.locations.iter().any(|location| {
                            location.side == range.side
                                && location.path.display == range.path
                                && location.range.start.line < range.end_line
                                && range.start_line.saturating_sub(1) < location.range.end.line
                        })
                });
                if intersects && seen.insert(evidence.test.id.as_str()) {
                    result.push(format!(
                        "{:?} · {} · {:?}",
                        evidence.status, evidence.test.name, evidence.test.granularity
                    ));
                }
            }
        }
        result
    }
}

fn section(lines: &mut Vec<String>, title: &str) {
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.push(format!("── {title} ──"));
}

fn source_heading(entry: &crate::context::ContextEntry) -> String {
    let author = entry
        .author
        .as_deref()
        .map_or(String::new(), |author| format!(" · {author}"));
    let status = match entry.status {
        CaptureStatus::Complete => String::new(),
        ref status => format!(" · {:?}", status).to_ascii_lowercase(),
    };
    let location = entry.external_url.as_deref().unwrap_or(&entry.origin);
    format!(
        "[{}] {location}{author}{status}",
        format!("{:?}", entry.kind).to_ascii_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::fs;

    use crate::comments::CommentAnchor;
    use crate::context::{ContextAnchor, ContextMetadata, ContextSourceKind};
    use crate::explanations::{Explanation, ExplanationAnnotation};
    use crate::graph::SourceSide;
    use crate::snapshot::{FileChange, GitPath, Hunk, SnapshotInput};
    use crate::test_evidence::{
        ExecutedRange, RunStatus, TEST_EVIDENCE_SCHEMA_VERSION, TestEvidence, TestIdentity, TestRun,
    };

    #[test]
    fn populated_item_context_distinguishes_model_manual_draft_test_and_source() {
        let root = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("after/src")).unwrap();
        fs::write(root.path().join("after/src/a.ts"), "export const a = 1;\n").unwrap();
        let path = GitPath::from_bytes(b"src/a.ts".to_vec());
        let snapshot = crate::snapshot::Snapshot {
            id: crate::model::SnapshotId::new(),
            repository: root.path().into(),
            input: SnapshotInput::Uncommitted,
            original_base: "base".into(),
            original_head: "head".into(),
            comparison_base: "base".into(),
            before_commit: "base".into(),
            after_commit: "head".into(),
            captured_at: Utc::now(),
            source_fingerprint: "captured".into(),
            files: vec![FileChange {
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
                hunks: vec![Hunk {
                    id: "h_one".into(),
                    old_start: 0,
                    old_count: 0,
                    new_start: 1,
                    new_count: 1,
                    header: "@@ -0,0 +1 @@".into(),
                    patch: "@@ -0,0 +1 @@\n+export const a = 1;\n".into(),
                }],
            }],
            storage_dir: root.path().into(),
        };
        let graph = ChangeGraph::from_snapshot(&snapshot);
        let node = &graph.nodes["h_one"];

        let mut bundle = ContextBundle::default();
        bundle.add_markdown_with(
            "PR thread",
            "Requirement: preserve the fallback.".into(),
            1024,
            ContextMetadata {
                source_key: "github:thread:1".into(),
                kind: ContextSourceKind::ReviewThread,
                external_url: Some("https://example.test/thread/1".into()),
                author: Some("reviewer".into()),
                anchor: Some(ContextAnchor {
                    snapshot_id: snapshot.id.to_string(),
                    side: SourceSide::Right,
                    path: path.clone(),
                    start_line: 1,
                    end_line: 1,
                }),
                ..ContextMetadata::default()
            },
        );

        let mut explanations = ExplanationStore::new(
            &snapshot,
            &graph,
            Some(bundle.digest.clone()),
            Some("old-tests".into()),
        );
        explanations.entries.insert(
            "h_one".into(),
            Explanation {
                item_id: "h_one".into(),
                text: "Adds the captured fallback.".into(),
                annotations: vec![ExplanationAnnotation {
                    side: SourceSide::Right,
                    path: "src/a.ts".into(),
                    start_line: 1,
                    end_line: 1,
                    label: "new fallback".into(),
                }],
                evidence_ids: vec![bundle.entries[0].id.to_string()],
                authority: "model_interpretation".into(),
                created_at: Utc::now(),
                input_digest: "stale-input".into(),
            },
        );
        explanations
            .manual_notes
            .insert("h_one".into(), "Check the empty input path.".into());

        let mut drafts = DraftStore::default();
        drafts
            .add(
                "Could this return early?".into(),
                Some(CommentAnchor {
                    snapshot_id: snapshot.id.to_string(),
                    side: SourceSide::Right,
                    path: path.clone(),
                    start_line: 1,
                    end_line: 1,
                    source_fingerprint: "captured".into(),
                }),
            )
            .unwrap();

        let test = TestEvidence {
            id: "tev_one".into(),
            run_id: "run_one".into(),
            test: TestIdentity {
                id: "test-file".into(),
                name: "tests/a.test.ts".into(),
                path: Some("tests/a.test.ts".into()),
                line: Some(1),
                granularity: AttributionGranularity::File,
            },
            status: RunStatus::Failed,
            ranges: vec![ExecutedRange {
                path: "src/a.ts".into(),
                side: SourceSide::Right,
                start_line: 1,
                end_line: 1,
                hits: 1,
                source_hash: Some("hash".into()),
                compatible: true,
            }],
            producer: "jest".into(),
        };
        let tests = TestEvidenceStore {
            schema_version: TEST_EVIDENCE_SCHEMA_VERSION,
            snapshot_id: snapshot.id.to_string(),
            revision: 1,
            digest: "current-tests".into(),
            runs: vec![TestRun {
                id: "run_one".into(),
                snapshot_id: snapshot.id.to_string(),
                side: SourceSide::Right,
                profile: "jest-file".into(),
                runner_version: None,
                command: Vec::new(),
                selection: vec!["tests/a.test.ts".into()],
                cache_key: "key".into(),
                status: RunStatus::Failed,
                completed: true,
                started_at: Utc::now(),
                completed_at: Some(Utc::now()),
                stdout: String::new(),
                stderr: String::new(),
                output_truncated: false,
                report_digest: Some("report".into()),
                evidence: vec![test],
                diagnostics: Vec::new(),
            }],
        };
        let context = ReviewContext {
            bundle,
            explanations,
            drafts,
            tests,
            explanations_stale: true,
            unavailable: Vec::new(),
        };

        let lines = context.item_lines("h_one", Some(node)).join("\n");
        assert!(lines.contains("model interpretation · stale"));
        assert!(lines.contains("Manual note"));
        assert!(lines.contains("Draft comments"));
        assert!(lines.contains("Failed · tests/a.test.ts · File"));
        assert!(lines.contains("https://example.test/thread/1 · reviewer"));
        assert_eq!(
            context.annotation_for("h_one", Some(node), SourceSide::Right, 1),
            Some("new fallback")
        );
        assert!(context.session_lines().join("\n").contains("Requirement"));
    }
}

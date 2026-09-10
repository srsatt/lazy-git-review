use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, Result};
use crate::graph::SourceSide;
use crate::model::CommentId;
use crate::snapshot::{GitPath, Snapshot};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CommentAnchor {
    pub snapshot_id: String,
    pub side: SourceSide,
    pub path: GitPath,
    pub start_line: u32,
    pub end_line: u32,
    pub source_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DraftComment {
    pub id: CommentId,
    pub body: String,
    pub body_revision: u64,
    pub anchor: Option<CommentAnchor>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub stale: bool,
    pub orphaned_reason: Option<String>,
    pub plugin_id: Option<String>,
    pub remote_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CommentConflict {
    pub comment_id: String,
    pub local_body: String,
    pub imported_body: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct DraftStore {
    pub drafts: BTreeMap<String, DraftComment>,
    pub conflicts: Vec<CommentConflict>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ExportMetadata {
    schema_version: u32,
    snapshot_id: String,
    comments: BTreeMap<String, ExportedComment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ExportedComment {
    body_digest: String,
    body_revision: u64,
    anchor: Option<CommentAnchor>,
}

impl DraftStore {
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            Ok(serde_json::from_slice(&fs::read(path)?)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn add(&mut self, body: String, anchor: Option<CommentAnchor>) -> Result<CommentId> {
        validate_body(&body)?;
        let id = CommentId::new();
        let now = Utc::now();
        self.drafts.insert(
            id.to_string(),
            DraftComment {
                id: id.clone(),
                body,
                body_revision: 1,
                anchor,
                created_at: now,
                updated_at: now,
                stale: false,
                orphaned_reason: None,
                plugin_id: None,
                remote_id: None,
            },
        );
        Ok(id)
    }

    pub fn set_plugin_id(&mut self, id: &str, plugin_id: String) -> Result<()> {
        let draft = self
            .drafts
            .get_mut(id)
            .ok_or_else(|| invalid("comment_not_found", format!("comment {id} does not exist")))?;
        draft.plugin_id = Some(plugin_id);
        Ok(())
    }

    pub fn set_remote_id(&mut self, id: &str, remote_id: String) -> Result<()> {
        let draft = self
            .drafts
            .get_mut(id)
            .ok_or_else(|| invalid("comment_not_found", format!("comment {id} does not exist")))?;
        draft.remote_id = Some(remote_id);
        Ok(())
    }

    pub fn update(&mut self, id: &str, body: String, expected_revision: u64) -> Result<()> {
        validate_body(&body)?;
        let draft = self
            .drafts
            .get_mut(id)
            .ok_or_else(|| invalid("comment_not_found", format!("comment {id} does not exist")))?;
        if draft.body_revision != expected_revision {
            return Err(invalid(
                "comment_revision_conflict",
                format!(
                    "comment {id} expected revision {expected_revision}, current {}",
                    draft.body_revision
                ),
            ));
        }
        draft.body = body;
        draft.body_revision += 1;
        draft.updated_at = Utc::now();
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        self.drafts
            .remove(id)
            .ok_or_else(|| invalid("comment_not_found", format!("comment {id} does not exist")))?;
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, path)?;
        Ok(())
    }

    pub fn export_markdown(&self, snapshot: &Snapshot, output: &Path) -> Result<PathBuf> {
        let mut markdown = String::from("# Review\n\n");
        let mut metadata = ExportMetadata {
            schema_version: 1,
            snapshot_id: snapshot.id.to_string(),
            comments: BTreeMap::new(),
        };
        for draft in self.drafts.values() {
            if let Some(anchor) = &draft.anchor {
                markdown.push_str(&format!(
                    "## {} — {}:{}-{} [{:?}]\n\n",
                    draft.id, anchor.path.display, anchor.start_line, anchor.end_line, anchor.side
                ));
                if let Some(source) = source_excerpt(snapshot, anchor)? {
                    markdown.push_str("```text\n");
                    markdown.push_str(&source);
                    if !source.ends_with('\n') {
                        markdown.push('\n');
                    }
                    markdown.push_str("```\n\n");
                }
            } else {
                markdown.push_str(&format!("## {} — General feedback\n\n", draft.id));
            }
            markdown.push_str(&draft.body);
            markdown.push_str("\n\n");
            metadata.comments.insert(
                draft.id.to_string(),
                ExportedComment {
                    body_digest: body_digest(&draft.body),
                    body_revision: draft.body_revision,
                    anchor: draft.anchor.clone(),
                },
            );
        }
        fs::write(output, markdown)?;
        let sidecar = sidecar_path(output);
        fs::write(&sidecar, serde_json::to_vec_pretty(&metadata)?)?;
        Ok(sidecar)
    }

    pub fn import_markdown(&mut self, snapshot: &Snapshot, input: &Path) -> Result<()> {
        let markdown = fs::read_to_string(input)?;
        let sidecar = sidecar_path(input);
        if !sidecar.exists() {
            return Err(invalid(
                "comment_metadata_missing",
                format!("anchor metadata {} is missing", sidecar.display()),
            ));
        }
        let metadata: ExportMetadata = serde_json::from_slice(&fs::read(&sidecar)?)?;
        if metadata.schema_version != 1 || metadata.snapshot_id != snapshot.id.as_str() {
            return Err(invalid(
                "comment_metadata_mismatch",
                "anchor metadata has an unsupported schema or belongs to another snapshot",
            ));
        }
        let sections = parse_sections(&markdown)?;
        let mut seen = BTreeSet::new();
        for (raw_id, imported_body) in sections {
            if let Ok(id) = CommentId::parse(raw_id.clone()) {
                seen.insert(raw_id.clone());
                let exported = metadata.comments.get(&raw_id).ok_or_else(|| {
                    invalid(
                        "comment_metadata_missing_entry",
                        format!("metadata has no anchor for {raw_id}"),
                    )
                })?;
                if let Some(current) = self.drafts.get_mut(&raw_id) {
                    if imported_body == current.body {
                        continue;
                    }
                    let local_changed = body_digest(&current.body) != exported.body_digest;
                    if local_changed {
                        self.conflicts.push(CommentConflict {
                            comment_id: raw_id,
                            local_body: current.body.clone(),
                            imported_body,
                            reason: "both local draft and Markdown changed since export".into(),
                        });
                    } else {
                        validate_body(&imported_body)?;
                        current.body = imported_body;
                        current.body_revision += 1;
                        current.updated_at = Utc::now();
                    }
                } else {
                    validate_body(&imported_body)?;
                    let now = Utc::now();
                    self.drafts.insert(
                        raw_id,
                        DraftComment {
                            id,
                            body: imported_body,
                            body_revision: exported.body_revision + 1,
                            anchor: exported.anchor.clone(),
                            created_at: now,
                            updated_at: now,
                            stale: false,
                            orphaned_reason: None,
                            plugin_id: None,
                            remote_id: None,
                        },
                    );
                }
            } else {
                self.add(imported_body, None)?;
            }
        }
        // Omitted sections intentionally remain; deletion is a separate explicit action.
        let _omitted: Vec<_> = metadata
            .comments
            .keys()
            .filter(|id| !seen.contains(*id))
            .collect();
        Ok(())
    }

    pub fn carry_to_snapshot(&self, next: &Snapshot) -> Result<Self> {
        let mut carried = self.clone();
        for draft in carried.drafts.values_mut() {
            let Some(anchor) = draft.anchor.as_mut() else {
                continue;
            };
            let side = match anchor.side {
                SourceSide::Left => "before",
                SourceSide::Right => "after",
            };
            let path = next.storage_dir.join(side).join(anchor.path.to_path_buf()?);
            let matches = fs::read(&path)
                .map(|source| hex::encode(Sha256::digest(source)) == anchor.source_fingerprint)
                .unwrap_or(false);
            anchor.snapshot_id = next.id.to_string();
            draft.stale = !matches;
            draft.orphaned_reason = (!matches).then(|| {
                "captured source no longer matches this comment anchor after refresh".into()
            });
        }
        Ok(carried)
    }
}

pub fn comments_path(snapshot: &Snapshot) -> PathBuf {
    snapshot.storage_dir.join("comments.json")
}

fn source_excerpt(snapshot: &Snapshot, anchor: &CommentAnchor) -> Result<Option<String>> {
    let side = match anchor.side {
        SourceSide::Left => "before",
        SourceSide::Right => "after",
    };
    let path = snapshot
        .storage_dir
        .join(side)
        .join(anchor.path.to_path_buf()?);
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)?;
    let lines: Vec<_> = text.lines().collect();
    let start = anchor.start_line.saturating_sub(1) as usize;
    let end = (anchor.end_line as usize).min(lines.len());
    Ok(Some(lines.get(start..end).unwrap_or_default().join("\n")))
}

fn parse_sections(markdown: &str) -> Result<Vec<(String, String)>> {
    let header = Regex::new(r"(?m)^##\s+([^\s]+)(?:\s+—.*)?$").expect("valid regex");
    let matches: Vec<_> = header.find_iter(markdown).collect();
    if matches.is_empty() {
        return Ok(if markdown.trim().is_empty() {
            Vec::new()
        } else {
            vec![("general".into(), markdown.trim().into())]
        });
    }
    let mut sections = Vec::new();
    for (index, matched) in matches.iter().enumerate() {
        let line = matched.as_str();
        let id = line
            .trim_start_matches("##")
            .split_whitespace()
            .next()
            .unwrap_or("general")
            .to_owned();
        let end = matches
            .get(index + 1)
            .map(|next| next.start())
            .unwrap_or(markdown.len());
        let raw_body = markdown[matched.end()..end].trim();
        let body = strip_source_block(raw_body);
        sections.push((id, body));
    }
    Ok(sections)
}

fn strip_source_block(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("```text\n")
        && let Some(end) = rest.find("\n```\n")
    {
        return rest[end + 5..].trim().to_owned();
    }
    value.trim().to_owned()
}

fn sidecar_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".anchors.json");
    PathBuf::from(name)
}

fn body_digest(body: &str) -> String {
    hex::encode(Sha256::digest(body.as_bytes()))
}

fn validate_body(body: &str) -> Result<()> {
    if body.trim().is_empty() || body.len() > 262_144 {
        return Err(invalid(
            "invalid_comment_body",
            "comment body must contain 1..262144 bytes",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git;
    use crate::graph::SourceSide;
    use crate::snapshot::{SnapshotInput, capture};
    use tempfile::TempDir;

    fn fixture() -> (TempDir, Snapshot) {
        let repo = TempDir::new().unwrap();
        git::run(repo.path(), &[git::os("init"), git::os("-q")]).unwrap();
        fs::write(repo.path().join("old.ts"), "one\ntwo\n").unwrap();
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
        let base = git::text(repo.path(), &[git::os("rev-parse"), git::os("HEAD")]).unwrap();
        git::run(
            repo.path(),
            &[git::os("mv"), git::os("old.ts"), git::os("new.ts")],
        )
        .unwrap();
        fs::write(repo.path().join("new.ts"), "one\nchanged\n").unwrap();
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
                git::os("head"),
            ],
        )
        .unwrap();
        let head = git::text(repo.path(), &[git::os("rev-parse"), git::os("HEAD")]).unwrap();
        let data = TempDir::new().unwrap();
        let snapshot = capture(
            repo.path(),
            SnapshotInput::Revisions { base, head },
            data.path(),
        )
        .unwrap();
        (data, snapshot)
    }

    #[test]
    fn readable_export_import_is_idempotent_and_detects_conflicts() {
        let (_data, snapshot) = fixture();
        let old_path = snapshot
            .files
            .iter()
            .find_map(|file| file.old_path.clone())
            .unwrap();
        let mut store = DraftStore::default();
        let id = store
            .add(
                "Original feedback".into(),
                Some(CommentAnchor {
                    snapshot_id: snapshot.id.to_string(),
                    side: SourceSide::Left,
                    path: old_path,
                    start_line: 1,
                    end_line: 2,
                    source_fingerprint: "fingerprint".into(),
                }),
            )
            .unwrap();
        store.add("General feedback".into(), None).unwrap();
        let output = snapshot.storage_dir.join("review.md");
        store.export_markdown(&snapshot, &output).unwrap();
        let exported = fs::read_to_string(&output).unwrap();
        assert!(exported.contains("old.ts:1-2 [Left]"));
        assert!(exported.contains("```text\none\ntwo\n```"));
        store.import_markdown(&snapshot, &output).unwrap();
        assert_eq!(store.drafts[&id.to_string()].body_revision, 1);

        store
            .update(&id.to_string(), "Local edit".into(), 1)
            .unwrap();
        fs::write(
            &output,
            exported.replace("Original feedback", "Markdown edit"),
        )
        .unwrap();
        store.import_markdown(&snapshot, &output).unwrap();
        assert_eq!(store.conflicts.len(), 1);
        assert_eq!(store.conflicts[0].local_body, "Local edit");
        assert_eq!(store.conflicts[0].imported_body, "Markdown edit");
    }

    #[test]
    fn missing_metadata_never_guesses_anchor_and_delete_is_explicit() {
        let (_data, snapshot) = fixture();
        let output = snapshot.storage_dir.join("review.md");
        fs::write(
            &output,
            "# Review\n\n## cmt_unknown — General feedback\n\nText\n",
        )
        .unwrap();
        let mut store = DraftStore::default();
        assert!(store.import_markdown(&snapshot, &output).is_err());
        let id = store.add("Delete me".into(), None).unwrap();
        store.delete(id.as_str()).unwrap();
        assert!(store.drafts.is_empty());
    }
}

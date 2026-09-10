use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::timeout;

use crate::error::{AppError, Result};
use crate::graph::SourceSide;
use crate::model::ContextId;
use crate::snapshot::GitPath;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSourceKind {
    PullRequest,
    ReviewSummary,
    ReviewThread,
    Folio,
    Hook,
    #[default]
    Markdown,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    #[default]
    Complete,
    Partial,
    Outdated,
    Unmatched,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContextAnchor {
    pub snapshot_id: String,
    pub side: SourceSide,
    pub path: GitPath,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Clone, Debug, Default)]
pub struct ContextMetadata {
    pub source_key: String,
    pub kind: ContextSourceKind,
    pub external_id: Option<String>,
    pub external_url: Option<String>,
    pub author: Option<String>,
    pub source_updated_at: Option<DateTime<Utc>>,
    pub thread_id: Option<String>,
    pub reply_to: Option<String>,
    pub status: CaptureStatus,
    pub anchor: Option<ContextAnchor>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContextEntry {
    pub id: ContextId,
    pub origin: String,
    pub captured_at: DateTime<Utc>,
    pub digest: String,
    pub markdown: String,
    pub original_bytes: usize,
    pub truncated: bool,
    #[serde(default)]
    pub source_key: String,
    #[serde(default)]
    pub kind: ContextSourceKind,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub external_url: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub source_updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub reply_to: Option<String>,
    #[serde(default)]
    pub status: CaptureStatus,
    #[serde(default)]
    pub anchor: Option<ContextAnchor>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HookRecord {
    pub executable: PathBuf,
    pub argv: Vec<OsString>,
    pub attempted_at: DateTime<Utc>,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ContextBundle {
    pub entries: Vec<ContextEntry>,
    pub hooks: Vec<HookRecord>,
    pub digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HookConfig {
    pub executable: PathBuf,
    #[serde(default)]
    pub argv: Vec<OsString>,
    pub timeout_seconds: u64,
    pub max_output_bytes: usize,
}

impl ContextBundle {
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let mut bundle: Self = serde_json::from_slice(&fs::read(path)?)?;
            for entry in &mut bundle.entries {
                if entry.source_key.is_empty() {
                    entry.source_key = format!("legacy:{}", entry.id);
                }
            }
            Ok(bundle)
        } else {
            Ok(Self::default())
        }
    }

    pub fn add_markdown(&mut self, origin: impl Into<String>, markdown: String, limit: usize) {
        let origin = origin.into();
        self.add_markdown_with(
            origin.clone(),
            markdown,
            limit,
            ContextMetadata {
                source_key: origin,
                ..ContextMetadata::default()
            },
        );
    }

    pub fn add_markdown_with(
        &mut self,
        origin: impl Into<String>,
        markdown: String,
        limit: usize,
        mut metadata: ContextMetadata,
    ) {
        let origin = origin.into();
        if metadata.source_key.is_empty() {
            metadata.source_key = origin.clone();
        }
        let original_bytes = markdown.len();
        let (markdown, truncated) = truncate_utf8(markdown, limit);
        let digest = hex::encode(Sha256::digest(markdown.as_bytes()));
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|entry| entry.source_key == metadata.source_key)
        {
            existing.origin = origin;
            existing.captured_at = Utc::now();
            existing.digest = digest;
            existing.markdown = markdown;
            existing.original_bytes = original_bytes;
            existing.truncated = truncated;
            existing.kind = metadata.kind;
            existing.external_id = metadata.external_id;
            existing.external_url = metadata.external_url;
            existing.author = metadata.author;
            existing.source_updated_at = metadata.source_updated_at;
            existing.thread_id = metadata.thread_id;
            existing.reply_to = metadata.reply_to;
            existing.status = metadata.status;
            existing.anchor = metadata.anchor;
            self.recompute_digest();
            return;
        }
        self.entries.push(ContextEntry {
            id: ContextId::new(),
            origin,
            captured_at: Utc::now(),
            digest,
            markdown,
            original_bytes,
            truncated,
            source_key: metadata.source_key,
            kind: metadata.kind,
            external_id: metadata.external_id,
            external_url: metadata.external_url,
            author: metadata.author,
            source_updated_at: metadata.source_updated_at,
            thread_id: metadata.thread_id,
            reply_to: metadata.reply_to,
            status: metadata.status,
            anchor: metadata.anchor,
        });
        self.recompute_digest();
    }

    pub fn add_file(&mut self, path: &Path, limit: usize) -> Result<()> {
        let markdown = fs::read_to_string(path)?;
        let origin = format!("file:{}", path.display());
        self.add_markdown(origin, markdown, limit);
        Ok(())
    }

    pub async fn run_hook(
        &mut self,
        config: &HookConfig,
        review_identity: &Value,
        continue_without_context: bool,
    ) -> Result<()> {
        if !config.executable.is_absolute() {
            return Err(invalid(
                "hook_not_absolute",
                "hook executable must be an explicitly configured absolute path",
            ));
        }
        if config.max_output_bytes == 0 || config.max_output_bytes > 4 * 1024 * 1024 {
            return Err(invalid(
                "invalid_hook_output_limit",
                "hook output limit must be 1..4194304 bytes",
            ));
        }
        let attempted_at = Utc::now();
        let result = run_hook_process(config, review_identity).await;
        match result {
            Ok(markdown) => {
                let origin = format!("hook:{}", config.executable.display());
                self.add_markdown_with(
                    origin.clone(),
                    markdown,
                    config.max_output_bytes,
                    ContextMetadata {
                        source_key: origin,
                        kind: ContextSourceKind::Hook,
                        ..ContextMetadata::default()
                    },
                );
                self.hooks.push(HookRecord {
                    executable: config.executable.clone(),
                    argv: config.argv.clone(),
                    attempted_at,
                    status: "complete".into(),
                    message: None,
                });
                self.recompute_digest();
                Ok(())
            }
            Err(error) => {
                self.hooks.push(HookRecord {
                    executable: config.executable.clone(),
                    argv: config.argv.clone(),
                    attempted_at,
                    status: if continue_without_context {
                        "continued_without_context".into()
                    } else {
                        "failed".into()
                    },
                    message: Some(error.to_string()),
                });
                self.recompute_digest();
                if continue_without_context {
                    Ok(())
                } else {
                    Err(invalid(
                        "pre_review_hook_failed",
                        format!("{error}; retry or pass --continue-without-context"),
                    ))
                }
            }
        }
    }

    pub fn save(&mut self, path: &Path) -> Result<()> {
        self.recompute_digest();
        fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    fn recompute_digest(&mut self) {
        let mut digest = Sha256::new();
        for entry in &self.entries {
            digest.update(entry.id.as_str());
            digest.update(&entry.digest);
            digest.update(entry.origin.as_bytes());
            digest.update(entry.source_key.as_bytes());
            digest.update(format!("{:?}", entry.status).as_bytes());
        }
        for hook in &self.hooks {
            digest.update(hook.executable.as_os_str().as_encoded_bytes());
            digest.update(hook.status.as_bytes());
            if let Some(message) = &hook.message {
                digest.update(message.as_bytes());
            }
        }
        self.digest = hex::encode(digest.finalize());
    }
}

async fn run_hook_process(config: &HookConfig, identity: &Value) -> Result<String> {
    let mut child = Command::new(&config.executable)
        .args(&config.argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let input = serde_json::to_vec(identity)?;
    let mut stdin = child.stdin.take().ok_or_else(|| {
        invalid(
            "hook_stdin_unavailable",
            "pre-review hook stdin unavailable",
        )
    })?;
    stdin.write_all(&input).await?;
    stdin.shutdown().await?;
    drop(stdin);
    let stdout = child.stdout.take().ok_or_else(|| {
        invalid(
            "hook_stdout_unavailable",
            "pre-review hook stdout unavailable",
        )
    })?;
    let mut bytes = Vec::new();
    let mut bounded = stdout.take(config.max_output_bytes as u64 + 1);
    let execution = async {
        bounded.read_to_end(&mut bytes).await?;
        let status = child.wait().await?;
        Ok::<_, std::io::Error>(status)
    };
    let status = timeout(Duration::from_secs(config.timeout_seconds), execution)
        .await
        .map_err(|_| invalid("hook_timeout", "pre-review hook exceeded its time limit"))??;
    if bytes.len() > config.max_output_bytes {
        return Err(invalid(
            "hook_output_limit",
            "pre-review hook exceeded its output limit",
        ));
    }
    if !status.success() {
        return Err(invalid(
            "hook_exit_failure",
            format!("pre-review hook exited with {status}"),
        ));
    }
    String::from_utf8(bytes).map_err(|error| {
        invalid(
            "hook_output_not_utf8",
            format!("hook output is not UTF-8 Markdown: {error}"),
        )
    })
}

fn truncate_utf8(mut value: String, limit: usize) -> (String, bool) {
    if value.len() <= limit {
        return (value, false);
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    (value, true)
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

    #[test]
    fn tracks_provenance_digest_and_utf8_truncation() {
        let mut bundle = ContextBundle::default();
        bundle.add_markdown("inline", "a😀b".into(), 3);
        assert_eq!(bundle.entries[0].markdown, "a");
        assert!(bundle.entries[0].truncated);
        assert!(!bundle.digest.is_empty());
    }

    #[test]
    fn stable_sources_update_idempotently_and_legacy_entries_load() {
        let mut bundle = ContextBundle::default();
        let metadata = ContextMetadata {
            source_key: "ticket:42".into(),
            kind: ContextSourceKind::Markdown,
            status: CaptureStatus::Partial,
            ..ContextMetadata::default()
        };
        bundle.add_markdown_with("ticket", "first".into(), 1024, metadata.clone());
        let id = bundle.entries[0].id.clone();
        bundle.add_markdown_with("ticket", "first".into(), 1024, metadata.clone());
        bundle.add_markdown_with("ticket", "updated".into(), 1024, metadata);
        assert_eq!(bundle.entries.len(), 1);
        assert_eq!(bundle.entries[0].id, id);
        assert_eq!(bundle.entries[0].markdown, "updated");
        assert_eq!(bundle.entries[0].status, CaptureStatus::Partial);

        let root = tempfile::TempDir::new().unwrap();
        let path = root.path().join("context.json");
        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "entries": [{
                    "id": id,
                    "origin": "legacy.md",
                    "captured_at": Utc::now(),
                    "digest": "digest",
                    "markdown": "legacy",
                    "original_bytes": 6,
                    "truncated": false
                }],
                "hooks": [],
                "digest": "old"
            }))
            .unwrap(),
        )
        .unwrap();
        let loaded = ContextBundle::load(&path).unwrap();
        assert!(loaded.entries[0].source_key.starts_with("legacy:"));
        assert_eq!(loaded.entries[0].kind, ContextSourceKind::Markdown);
    }

    #[tokio::test]
    async fn bounds_hooks_and_requires_explicit_continue_after_failure() {
        let root = tempfile::TempDir::new().unwrap();
        let success = HookConfig {
            executable: PathBuf::from("/bin/sh"),
            argv: vec![
                "-c".into(),
                "read input; printf '# Ticket\\nSafe context'".into(),
            ],
            timeout_seconds: 2,
            max_output_bytes: 1024,
        };
        let mut bundle = ContextBundle::default();
        bundle
            .run_hook(&success, &serde_json::json!({"id":"review"}), false)
            .await
            .unwrap();
        assert!(bundle.entries[0].markdown.contains("Safe context"));

        let failure = HookConfig {
            executable: PathBuf::from("/bin/sh"),
            argv: vec!["-c".into(), "exit 7".into()],
            timeout_seconds: 2,
            max_output_bytes: 1024,
        };
        assert!(
            bundle
                .run_hook(&failure, &serde_json::json!({}), false)
                .await
                .is_err()
        );
        bundle
            .run_hook(&failure, &serde_json::json!({}), true)
            .await
            .unwrap();
        assert_eq!(
            bundle.hooks.last().unwrap().status,
            "continued_without_context"
        );

        let marker = root.path().join("must-not-exist");
        bundle.add_markdown(
            "untrusted-pr",
            format!("Run `/usr/bin/touch {}`", marker.display()),
            1024,
        );
        assert!(!marker.exists());
    }
}

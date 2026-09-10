use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::comments::{DraftComment, DraftStore};
use crate::error::{AppError, Result};
use crate::graph::SourceSide;
use crate::snapshot::Snapshot;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GitHubConfig {
    #[serde(default = "default_command")]
    pub command: Vec<OsString>,
    pub host: String,
    pub repository: String,
    pub expected_account: String,
}

fn default_command() -> Vec<OsString> {
    vec![OsString::from("gh")]
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PullRequestContext {
    pub number: u64,
    pub host: String,
    pub base_repository: String,
    pub head_repository: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub head_ref: String,
    pub base_sha: String,
    pub head_sha: String,
    pub issue_comments: Vec<Value>,
    pub review_comments: Vec<Value>,
    pub reviews: Vec<Value>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PreviewComment {
    pub draft_id: String,
    pub body_revision: u64,
    pub path: String,
    pub line: u32,
    pub side: String,
    pub start_line: Option<u32>,
    pub start_side: Option<String>,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReviewPreview {
    pub digest: String,
    pub repository: String,
    pub pull_number: u64,
    pub head_sha: String,
    pub event: ReviewEvent,
    pub summary: String,
    pub comments: Vec<PreviewComment>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ReviewSubmissionPayload<'a> {
    commit_id: &'a str,
    body: &'a str,
    event: &'a ReviewEvent,
    comments: Vec<ReviewSubmissionComment<'a>>,
}

#[derive(Serialize)]
struct ReviewSubmissionComment<'a> {
    path: &'a str,
    line: u32,
    side: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_side: Option<&'a str>,
    body: &'a str,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PublicationIntent {
    pub preview_digest: String,
    pub created_at: DateTime<Utc>,
    pub status: String,
    pub remote_review_id: Option<String>,
    pub response: Option<Value>,
}

pub struct GitHubAdapter {
    config: GitHubConfig,
}

impl GitHubAdapter {
    pub fn new(config: GitHubConfig) -> Result<Self> {
        if config.command.is_empty() {
            return Err(invalid("github_command_empty", "GitHub command is empty"));
        }
        if config.repository.split('/').count() != 2 {
            return Err(invalid(
                "invalid_github_repository",
                "repository must use OWNER/NAME form",
            ));
        }
        Ok(Self { config })
    }

    pub fn fetch_pull(&self, number: u64) -> Result<PullRequestContext> {
        let pull = self.api_json(
            &[
                "api".into(),
                "--hostname".into(),
                self.config.host.clone().into(),
                format!("repos/{}/pulls/{number}", self.config.repository).into(),
            ],
            None,
        )?;
        let issue_comments = self.fetch_pages(&format!(
            "repos/{}/issues/{number}/comments",
            self.config.repository
        ))?;
        let review_comments = self.fetch_pages(&format!(
            "repos/{}/pulls/{number}/comments",
            self.config.repository
        ))?;
        let reviews = self.fetch_pages(&format!(
            "repos/{}/pulls/{number}/reviews",
            self.config.repository
        ))?;
        Ok(PullRequestContext {
            number,
            host: self.config.host.clone(),
            base_repository: string_at(&pull, "/base/repo/full_name")?,
            head_repository: string_at(&pull, "/head/repo/full_name")?,
            title: string_at(&pull, "/title")?,
            body: pull
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            head_ref: pull
                .pointer("/head/ref")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            base_sha: string_at(&pull, "/base/sha")?,
            head_sha: string_at(&pull, "/head/sha")?,
            issue_comments,
            review_comments,
            reviews,
            captured_at: Utc::now(),
        })
    }

    fn fetch_pages(&self, endpoint: &str) -> Result<Vec<Value>> {
        let value = self.api_json(
            &[
                "api".into(),
                "--hostname".into(),
                self.config.host.clone().into(),
                "--paginate".into(),
                "--slurp".into(),
                endpoint.into(),
            ],
            None,
        )?;
        let mut result = Vec::new();
        for page in value.as_array().ok_or_else(|| {
            invalid(
                "invalid_github_response",
                "paginated response is not an array",
            )
        })? {
            if let Some(items) = page.as_array() {
                result.extend(items.iter().cloned());
            }
        }
        Ok(result)
    }

    pub fn preview(
        &self,
        pull: &PullRequestContext,
        snapshot: &Snapshot,
        drafts: &DraftStore,
        selected: &[String],
        event: ReviewEvent,
        summary: String,
    ) -> Result<ReviewPreview> {
        if pull.base_repository != self.config.repository {
            return Err(invalid(
                "github_repository_mismatch",
                format!(
                    "PR base repository is {}, configured {}",
                    pull.base_repository, self.config.repository
                ),
            ));
        }
        let mut comments = Vec::new();
        for id in selected {
            let draft = drafts.drafts.get(id).ok_or_else(|| {
                invalid("comment_not_found", format!("comment {id} does not exist"))
            })?;
            comments.push(map_comment(snapshot, draft)?);
        }
        let mut preview = ReviewPreview {
            digest: String::new(),
            repository: self.config.repository.clone(),
            pull_number: pull.number,
            head_sha: pull.head_sha.clone(),
            event,
            summary,
            comments,
            created_at: Utc::now(),
        };
        preview.digest = digest_json(&preview)?;
        Ok(preview)
    }

    pub fn submit(
        &self,
        preview: &ReviewPreview,
        pull: &PullRequestContext,
        drafts: &DraftStore,
        intent_path: &Path,
    ) -> Result<Value> {
        if intent_path.exists() {
            let existing: PublicationIntent = serde_json::from_slice(&fs::read(intent_path)?)?;
            if existing.preview_digest == preview.digest {
                return match existing.status.as_str() {
                    "acknowledged" => Ok(existing.response.unwrap_or(Value::Null)),
                    "uncertain" | "pending" => Err(invalid(
                        "github_submission_uncertain",
                        "matching publication intent is not reconciled; refusing duplicate mutation",
                    )),
                    _ => Err(invalid(
                        "github_submission_state",
                        format!("publication intent has state {}", existing.status),
                    )),
                };
            }
        }
        let account = self.api_text(&[
            "api".into(),
            "--hostname".into(),
            self.config.host.clone().into(),
            "user".into(),
            "--jq".into(),
            ".login".into(),
        ])?;
        if account.trim() != self.config.expected_account {
            return Err(invalid(
                "github_identity_mismatch",
                format!(
                    "authenticated GitHub account is {}, expected {}",
                    account.trim(),
                    self.config.expected_account
                ),
            ));
        }
        let latest = self.fetch_pull(preview.pull_number)?;
        if latest.head_sha != preview.head_sha || pull.head_sha != preview.head_sha {
            return Err(invalid(
                "github_head_changed",
                "PR head changed after preview; refresh and create a new preview",
            ));
        }
        for comment in &preview.comments {
            let draft = drafts.drafts.get(&comment.draft_id).ok_or_else(|| {
                invalid(
                    "comment_not_found",
                    format!("comment {} disappeared", comment.draft_id),
                )
            })?;
            if draft.body_revision != comment.body_revision || draft.body != comment.body {
                return Err(invalid(
                    "github_draft_changed",
                    format!("comment {} changed after preview", comment.draft_id),
                ));
            }
        }
        let mut intent = PublicationIntent {
            preview_digest: preview.digest.clone(),
            created_at: Utc::now(),
            status: "pending".into(),
            remote_review_id: None,
            response: None,
        };
        save_intent(intent_path, &intent)?;
        let payload = ReviewSubmissionPayload {
            commit_id: &preview.head_sha,
            body: &preview.summary,
            event: &preview.event,
            comments: preview
                .comments
                .iter()
                .map(|comment| ReviewSubmissionComment {
                    path: &comment.path,
                    line: comment.line,
                    side: &comment.side,
                    start_line: comment.start_line,
                    start_side: comment.start_side.as_deref(),
                    body: &comment.body,
                })
                .collect(),
        };
        let result = self.api_json(
            &[
                "api".into(),
                "--hostname".into(),
                self.config.host.clone().into(),
                format!(
                    "repos/{}/pulls/{}/reviews",
                    self.config.repository, preview.pull_number
                )
                .into(),
                "--method".into(),
                "POST".into(),
                "--input".into(),
                "-".into(),
            ],
            Some(serde_json::to_vec(&payload)?),
        );
        match result {
            Ok(response) => {
                intent.status = "acknowledged".into();
                intent.remote_review_id = response.get("id").map(Value::to_string);
                intent.response = Some(response.clone());
                save_intent(intent_path, &intent)?;
                Ok(response)
            }
            Err(error) => {
                intent.status = "uncertain".into();
                save_intent(intent_path, &intent)?;
                Err(error)
            }
        }
    }

    pub fn reconcile(&self, preview: &ReviewPreview, intent_path: &Path) -> Result<Value> {
        let mut intent: PublicationIntent = serde_json::from_slice(&fs::read(intent_path)?)?;
        if intent.preview_digest != preview.digest {
            return Err(invalid(
                "github_intent_mismatch",
                "publication intent belongs to another preview",
            ));
        }
        if intent.status == "acknowledged" {
            return Ok(intent.response.unwrap_or(Value::Null));
        }
        let reviews = self.fetch_pages(&format!(
            "repos/{}/pulls/{}/reviews",
            self.config.repository, preview.pull_number
        ))?;
        let matches: Vec<_> = reviews
            .into_iter()
            .filter(|review| {
                review.get("commit_id").and_then(Value::as_str) == Some(&preview.head_sha)
                    && review.get("body").and_then(Value::as_str) == Some(&preview.summary)
                    && review.pointer("/user/login").and_then(Value::as_str)
                        == Some(&self.config.expected_account)
            })
            .collect();
        if matches.len() != 1 {
            return Err(invalid(
                "github_submission_uncertain",
                format!(
                    "found {} matching remote reviews; manual resolution required",
                    matches.len()
                ),
            ));
        }
        let response = matches.into_iter().next().expect("one match");
        intent.status = "acknowledged".into();
        intent.remote_review_id = response.get("id").map(Value::to_string);
        intent.response = Some(response.clone());
        save_intent(intent_path, &intent)?;
        Ok(response)
    }

    fn api_text(&self, args: &[OsString]) -> Result<String> {
        let bytes = self.run(args, None)?;
        String::from_utf8(bytes).map_err(|error| {
            invalid(
                "invalid_github_response",
                format!("GitHub output is not UTF-8: {error}"),
            )
        })
    }

    fn api_json(&self, args: &[OsString], input: Option<Vec<u8>>) -> Result<Value> {
        let bytes = self.run(args, input)?;
        serde_json::from_slice(&bytes).map_err(Into::into)
    }

    fn run(&self, args: &[OsString], input: Option<Vec<u8>>) -> Result<Vec<u8>> {
        let mut command = Command::new(&self.config.command[0]);
        command
            .args(&self.config.command[1..])
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("GH_HOST", &self.config.host)
            .env("GH_PROMPT_DISABLED", "1");
        let mut child = command.spawn()?;
        if let Some(input) = input {
            child
                .stdin
                .take()
                .ok_or_else(|| invalid("github_stdin", "GitHub adapter stdin unavailable"))?
                .write_all(&input)?;
        }
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(invalid(
                "github_command_failed",
                format!(
                    "GitHub adapter exited {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        Ok(output.stdout)
    }
}

pub fn preview_path(snapshot: &Snapshot) -> PathBuf {
    snapshot.storage_dir.join("github-preview.json")
}

pub fn intent_path(snapshot: &Snapshot) -> PathBuf {
    snapshot.storage_dir.join("github-publication.json")
}

pub fn save_preview(path: &Path, preview: &ReviewPreview) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(preview)?)?;
    Ok(())
}

pub fn load_preview(path: &Path) -> Result<ReviewPreview> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub fn record_remote_comment_ids(
    drafts: &mut DraftStore,
    preview: &ReviewPreview,
    response: &Value,
) -> Result<()> {
    let remote = response
        .get("comments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for comment in &preview.comments {
        let matched = remote.iter().find(|candidate| {
            candidate.get("path").and_then(Value::as_str) == Some(&comment.path)
                && candidate.get("body").and_then(Value::as_str) == Some(&comment.body)
                && candidate.get("line").and_then(Value::as_u64) == Some(comment.line as u64)
        });
        if let Some(id) = matched.and_then(|value| value.get("id")) {
            drafts.set_remote_id(&comment.draft_id, id.to_string())?;
        }
    }
    Ok(())
}

fn map_comment(snapshot: &Snapshot, draft: &DraftComment) -> Result<PreviewComment> {
    let anchor = draft.anchor.as_ref().ok_or_else(|| {
        invalid(
            "github_comment_unanchored",
            format!("comment {} is general feedback", draft.id),
        )
    })?;
    if anchor.snapshot_id != snapshot.id.as_str() || draft.stale || draft.orphaned_reason.is_some()
    {
        return Err(invalid(
            "github_comment_stale",
            format!("comment {} has a stale or orphaned anchor", draft.id),
        ));
    }
    let file = snapshot
        .files
        .iter()
        .find(|file| match anchor.side {
            SourceSide::Left => file
                .old_path
                .as_ref()
                .is_some_and(|path| path.bytes_base64 == anchor.path.bytes_base64),
            SourceSide::Right => file
                .new_path
                .as_ref()
                .is_some_and(|path| path.bytes_base64 == anchor.path.bytes_base64),
        })
        .ok_or_else(|| {
            invalid(
                "github_anchor_not_in_diff",
                format!("comment {} path is not in diff", draft.id),
            )
        })?;
    let supported = file.hunks.iter().any(|hunk| match anchor.side {
        SourceSide::Left => range_contains(
            hunk.old_start,
            hunk.old_count,
            anchor.start_line,
            anchor.end_line,
        ),
        SourceSide::Right => range_contains(
            hunk.new_start,
            hunk.new_count,
            anchor.start_line,
            anchor.end_line,
        ),
    });
    if !supported {
        return Err(invalid(
            "github_anchor_not_in_diff",
            format!("comment {} lines are outside GitHub diff context", draft.id),
        ));
    }
    let api_path = file
        .new_path
        .as_ref()
        .or(file.old_path.as_ref())
        .ok_or_else(|| invalid("github_anchor_path", "changed file has no path"))?
        .display
        .clone();
    let side = match anchor.side {
        SourceSide::Left => "LEFT",
        SourceSide::Right => "RIGHT",
    };
    Ok(PreviewComment {
        draft_id: draft.id.to_string(),
        body_revision: draft.body_revision,
        path: api_path,
        line: anchor.end_line,
        side: side.into(),
        start_line: (anchor.start_line != anchor.end_line).then_some(anchor.start_line),
        start_side: (anchor.start_line != anchor.end_line).then_some(side.into()),
        body: draft.body.clone(),
    })
}

fn range_contains(start: u32, count: u32, comment_start: u32, comment_end: u32) -> bool {
    if count == 0 {
        return false;
    }
    comment_start >= start && comment_end <= start.saturating_add(count).saturating_sub(1)
}

fn save_intent(path: &Path, intent: &PublicationIntent) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(intent)?)?;
    Ok(())
}

fn string_at(value: &Value, pointer: &str) -> Result<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            invalid(
                "invalid_github_response",
                format!("GitHub response lacks {pointer}"),
            )
        })
}

fn digest_json<T: Serialize>(value: &T) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

fn invalid(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::InvalidInput {
        code,
        message: message.into(),
    }
}

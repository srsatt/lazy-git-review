use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, Result};
use crate::git;
use crate::model::SnapshotId;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SnapshotInput {
    Branch { base: String, head: String },
    Revisions { base: String, head: String },
    Uncommitted,
    Staged,
    Unstaged { include_untracked: bool },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub repository: PathBuf,
    pub input: SnapshotInput,
    pub original_base: String,
    pub original_head: String,
    pub comparison_base: String,
    pub before_commit: String,
    pub after_commit: String,
    pub captured_at: DateTime<Utc>,
    pub source_fingerprint: String,
    pub files: Vec<FileChange>,
    pub storage_dir: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GitPath {
    pub display: String,
    pub bytes_base64: String,
}

impl GitPath {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            display: String::from_utf8_lossy(&bytes).into_owned(),
            bytes_base64: BASE64.encode(bytes),
        }
    }

    pub fn to_path_buf(&self) -> Result<PathBuf> {
        let bytes = BASE64
            .decode(&self.bytes_base64)
            .map_err(|error| AppError::InvalidInput {
                code: "invalid_git_path",
                message: error.to_string(),
            })?;
        Ok(PathBuf::from(OsString::from_vec(bytes)))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileChange {
    pub status: String,
    pub old_path: Option<GitPath>,
    pub new_path: Option<GitPath>,
    pub old_mode: String,
    pub new_mode: String,
    pub old_object: String,
    pub new_object: String,
    pub before_blob: Option<String>,
    pub after_blob: Option<String>,
    pub binary: bool,
    pub submodule: bool,
    pub hunks: Vec<Hunk>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Hunk {
    pub id: String,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub header: String,
    pub patch: String,
}

pub fn capture(repository: &Path, input: SnapshotInput, root: &Path) -> Result<Snapshot> {
    let repository = fs::canonicalize(repository)?;
    ensure_repository(&repository)?;
    if !conflicted_paths(&repository)?.is_empty() {
        return Err(AppError::InvalidInput {
            code: "unmerged_index",
            message:
                "repository index contains unresolved merge stages; resolve them before capture"
                    .into(),
        });
    }

    for attempt in 0..2 {
        let before_fingerprint = source_fingerprint(&repository, &input)?;
        let result = capture_once(&repository, input.clone(), root);
        let after_fingerprint = source_fingerprint(&repository, &input)?;
        if before_fingerprint == after_fingerprint {
            return result;
        }
        if attempt == 1 {
            return Err(AppError::InvalidInput {
                code: "source_changed_during_capture",
                message:
                    "index or working tree changed repeatedly while snapshot was being captured"
                        .into(),
            });
        }
    }
    unreachable!()
}

fn capture_once(repository: &Path, input: SnapshotInput, root: &Path) -> Result<Snapshot> {
    let id = SnapshotId::new();
    let storage_dir = root.join("snapshots").join(id.as_str());
    let blobs_dir = storage_dir.join("blobs");
    fs::create_dir_all(&blobs_dir)?;
    let head = rev_parse(repository, "HEAD")?;
    let (original_base, original_head, comparison_base, diff_left, diff_right) = match &input {
        SnapshotInput::Branch { base, head } => {
            let base_id = rev_parse(repository, base)?;
            let head_id = rev_parse(repository, head)?;
            let merge_base = git::text(
                repository,
                &[
                    git::os("merge-base"),
                    OsStr::new(&base_id),
                    OsStr::new(&head_id),
                ],
            )?;
            (
                base_id,
                head_id.clone(),
                merge_base.clone(),
                DiffSide::Commit(merge_base),
                DiffSide::Commit(head_id),
            )
        }
        SnapshotInput::Revisions { base, head } => {
            let base_id = rev_parse(repository, base)?;
            let head_id = rev_parse(repository, head)?;
            (
                base_id.clone(),
                head_id.clone(),
                base_id.clone(),
                DiffSide::Commit(base_id),
                DiffSide::Commit(head_id),
            )
        }
        SnapshotInput::Uncommitted => (
            head.clone(),
            "WORKTREE".into(),
            head.clone(),
            DiffSide::Commit(head),
            DiffSide::Worktree,
        ),
        SnapshotInput::Staged => (
            head.clone(),
            "INDEX".into(),
            head.clone(),
            DiffSide::Commit(head),
            DiffSide::Index,
        ),
        SnapshotInput::Unstaged { .. } => (
            "INDEX".into(),
            "WORKTREE".into(),
            "INDEX".into(),
            DiffSide::Index,
            DiffSide::Worktree,
        ),
    };

    let mut files = raw_changes(repository, &diff_left, &diff_right)?;
    if matches!(input, SnapshotInput::Uncommitted)
        || matches!(
            input,
            SnapshotInput::Unstaged {
                include_untracked: true
            }
        )
    {
        add_untracked(repository, &mut files)?;
    }
    for file in &mut files {
        let before = read_side(repository, &diff_left, file.old_path.as_ref())?;
        let after = read_side(repository, &diff_right, file.new_path.as_ref())?;
        file.before_blob = store_blob(&blobs_dir, before.as_deref())?;
        file.after_blob = store_blob(&blobs_dir, after.as_deref())?;
        let patch = file_patch(repository, &diff_left, &diff_right, file)?;
        file.binary = patch.contains("GIT binary patch") || patch.contains("Binary files ");
        file.hunks = parse_hunks(&patch)?;
        stabilize_hunk_ids(file);
    }

    let (before_commit, after_commit) =
        create_visualization_repository(repository, &storage_dir, &diff_left, &diff_right, &files)?;
    let source_fingerprint = source_fingerprint(repository, &input)?;
    let snapshot = Snapshot {
        id,
        repository: repository.to_path_buf(),
        input,
        original_base,
        original_head,
        comparison_base,
        before_commit,
        after_commit,
        captured_at: Utc::now(),
        source_fingerprint,
        files,
        storage_dir: storage_dir.clone(),
    };
    fs::write(
        storage_dir.join("snapshot.json"),
        serde_json::to_vec_pretty(&snapshot)?,
    )?;
    Ok(snapshot)
}

#[derive(Clone)]
enum DiffSide {
    Commit(String),
    Index,
    Worktree,
}

fn raw_changes(repository: &Path, left: &DiffSide, right: &DiffSide) -> Result<Vec<FileChange>> {
    let mut owned = vec![
        OsString::from("diff"),
        OsString::from("--raw"),
        OsString::from("-z"),
        OsString::from("--full-index"),
        OsString::from("--find-renames"),
        OsString::from("--no-ext-diff"),
        OsString::from("--no-textconv"),
    ];
    append_diff_range(&mut owned, left, right);
    let refs: Vec<&OsStr> = owned.iter().map(OsString::as_os_str).collect();
    parse_raw(&git::run(repository, &refs)?.stdout)
}

fn parse_raw(raw: &[u8]) -> Result<Vec<FileChange>> {
    let mut fields = raw.split(|byte| *byte == 0).filter(|f| !f.is_empty());
    let mut files = Vec::new();
    while let Some(meta_and_maybe_path) = fields.next() {
        let (meta, first_path) =
            if let Some(tab) = meta_and_maybe_path.iter().position(|byte| *byte == b'\t') {
                (
                    &meta_and_maybe_path[..tab],
                    meta_and_maybe_path[tab + 1..].to_vec(),
                )
            } else {
                (
                    meta_and_maybe_path,
                    fields
                        .next()
                        .ok_or_else(|| invalid_diff("raw record has no path"))?
                        .to_vec(),
                )
            };
        let parts: Vec<&[u8]> = meta.split(|b| *b == b' ').collect();
        if parts.len() != 5 || !parts[0].starts_with(b":") {
            return Err(invalid_diff("raw record has an invalid metadata shape"));
        }
        let status = String::from_utf8_lossy(parts[4]).into_owned();
        let code = status.as_bytes().first().copied().unwrap_or(b'?');
        let second_path = if matches!(code, b'R' | b'C') {
            Some(
                fields
                    .next()
                    .ok_or_else(|| invalid_diff("rename record has no destination path"))?
                    .to_vec(),
            )
        } else {
            None
        };
        let (old_path, new_path) = match code {
            b'A' => (None, Some(GitPath::from_bytes(first_path))),
            b'D' => (Some(GitPath::from_bytes(first_path)), None),
            b'R' | b'C' => (
                Some(GitPath::from_bytes(first_path)),
                Some(GitPath::from_bytes(second_path.unwrap())),
            ),
            _ => {
                let path = GitPath::from_bytes(first_path);
                (Some(path.clone()), Some(path))
            }
        };
        let old_mode = String::from_utf8_lossy(&parts[0][1..]).into_owned();
        let new_mode = String::from_utf8_lossy(parts[1]).into_owned();
        files.push(FileChange {
            status,
            old_path,
            new_path,
            old_mode: old_mode.clone(),
            new_mode: new_mode.clone(),
            old_object: String::from_utf8_lossy(parts[2]).into_owned(),
            new_object: String::from_utf8_lossy(parts[3]).into_owned(),
            before_blob: None,
            after_blob: None,
            binary: false,
            submodule: old_mode == "160000" || new_mode == "160000",
            hunks: Vec::new(),
        });
    }
    Ok(files)
}

fn add_untracked(repository: &Path, files: &mut Vec<FileChange>) -> Result<()> {
    let output = git::run(
        repository,
        &[
            git::os("ls-files"),
            git::os("--others"),
            git::os("--exclude-standard"),
            git::os("-z"),
        ],
    )?;
    let existing: BTreeSet<Vec<u8>> = files
        .iter()
        .flat_map(|f| f.new_path.iter())
        .filter_map(|p| BASE64.decode(&p.bytes_base64).ok())
        .collect();
    for bytes in output.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        if !existing.contains(bytes) {
            files.push(FileChange {
                status: "A?".into(),
                old_path: None,
                new_path: Some(GitPath::from_bytes(bytes.to_vec())),
                old_mode: "000000".into(),
                new_mode: "100644".into(),
                old_object: "0".repeat(40),
                new_object: "0".repeat(40),
                before_blob: None,
                after_blob: None,
                binary: false,
                submodule: false,
                hunks: Vec::new(),
            });
        }
    }
    files.sort_by(|a, b| {
        a.new_path
            .as_ref()
            .or(a.old_path.as_ref())
            .map(|p| &p.bytes_base64)
            .cmp(
                &b.new_path
                    .as_ref()
                    .or(b.old_path.as_ref())
                    .map(|p| &p.bytes_base64),
            )
    });
    Ok(())
}

fn file_patch(
    repository: &Path,
    left: &DiffSide,
    right: &DiffSide,
    file: &FileChange,
) -> Result<String> {
    if file.status == "A?" {
        let path = file.new_path.as_ref().unwrap().to_path_buf()?;
        let after = fs::read(repository.join(path))?;
        if after.contains(&0) {
            return Ok("GIT binary patch\n".into());
        }
        let lines = String::from_utf8_lossy(&after);
        let count = lines.lines().count();
        return Ok(format!(
            "@@ -0,0 +1,{count} @@\n{}",
            lines.lines().map(|l| format!("+{l}\n")).collect::<String>()
        ));
    }
    let mut owned = vec![
        OsString::from("diff"),
        OsString::from("--binary"),
        OsString::from("--unified=3"),
        OsString::from("--find-renames"),
        OsString::from("--no-ext-diff"),
        OsString::from("--no-textconv"),
    ];
    append_diff_range(&mut owned, left, right);
    owned.push(OsString::from("--"));
    if let Some(path) = &file.old_path {
        owned.push(path.to_path_buf()?.into_os_string());
    }
    if file.new_path.as_ref().map(|path| &path.bytes_base64)
        != file.old_path.as_ref().map(|path| &path.bytes_base64)
        && let Some(path) = &file.new_path
    {
        owned.push(path.to_path_buf()?.into_os_string());
    }
    let refs: Vec<&OsStr> = owned.iter().map(OsString::as_os_str).collect();
    Ok(String::from_utf8_lossy(&git::run(repository, &refs)?.stdout).into_owned())
}

fn parse_hunks(patch: &str) -> Result<Vec<Hunk>> {
    let header =
        Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$").expect("valid regex");
    let lines: Vec<&str> = patch.split_inclusive('\n').collect();
    let mut starts = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if header.is_match(line.trim_end_matches('\n')) {
            starts.push(index);
        }
    }
    let mut hunks = Vec::new();
    for (position, start) in starts.iter().enumerate() {
        let end = starts.get(position + 1).copied().unwrap_or(lines.len());
        let raw_header = lines[*start].trim_end_matches('\n');
        let captures = header
            .captures(raw_header)
            .ok_or_else(|| invalid_diff("invalid hunk header"))?;
        let old_start = captures[1]
            .parse()
            .map_err(|_| invalid_diff("invalid old hunk start"))?;
        let old_count = captures
            .get(2)
            .map(|m| m.as_str().parse())
            .transpose()
            .map_err(|_| invalid_diff("invalid old hunk count"))?
            .unwrap_or(1);
        let new_start = captures[3]
            .parse()
            .map_err(|_| invalid_diff("invalid new hunk start"))?;
        let new_count = captures
            .get(4)
            .map(|m| m.as_str().parse())
            .transpose()
            .map_err(|_| invalid_diff("invalid new hunk count"))?
            .unwrap_or(1);
        let body = lines[*start..end].concat();
        let encoded = hex::encode(Sha256::digest(body.as_bytes()));
        let id = format!("h_{}", &encoded[..20]);
        hunks.push(Hunk {
            id,
            old_start,
            old_count,
            new_start,
            new_count,
            header: raw_header.into(),
            patch: body,
        });
    }
    Ok(hunks)
}

fn stabilize_hunk_ids(file: &mut FileChange) {
    let path = file
        .new_path
        .as_ref()
        .or(file.old_path.as_ref())
        .map(|path| path.bytes_base64.as_str())
        .unwrap_or("file-level");
    for hunk in &mut file.hunks {
        let mut digest = Sha256::new();
        digest.update(path);
        digest.update(hunk.old_start.to_le_bytes());
        digest.update(hunk.new_start.to_le_bytes());
        digest.update(hunk.patch.as_bytes());
        let encoded = hex::encode(digest.finalize());
        hunk.id = format!("h_{}", &encoded[..20]);
    }
}

fn append_diff_range(args: &mut Vec<OsString>, left: &DiffSide, right: &DiffSide) {
    match (left, right) {
        (DiffSide::Commit(a), DiffSide::Commit(b)) => {
            args.push(a.into());
            args.push(b.into());
        }
        (DiffSide::Commit(_), DiffSide::Index) => {
            args.push(OsString::from("--cached"));
        }
        (DiffSide::Commit(commit), DiffSide::Worktree) => {
            args.push(commit.into());
        }
        (DiffSide::Index, DiffSide::Worktree) => {}
        _ => unreachable!("unsupported diff range"),
    }
}

fn read_side(
    repository: &Path,
    side: &DiffSide,
    path: Option<&GitPath>,
) -> Result<Option<Vec<u8>>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let path = path.to_path_buf()?;
    match side {
        DiffSide::Commit(commit) => {
            let spec = commit_path(commit, &path);
            git::run_optional(repository, &[git::os("show"), spec.as_os_str()])
        }
        DiffSide::Index => {
            let spec = commit_path("", &path);
            git::run_optional(repository, &[git::os("show"), spec.as_os_str()])
        }
        DiffSide::Worktree => match fs::read(repository.join(path)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        },
    }
}

fn commit_path(commit: &str, path: &Path) -> OsString {
    let mut bytes = commit.as_bytes().to_vec();
    bytes.push(b':');
    bytes.extend_from_slice(path.as_os_str().as_bytes());
    OsString::from_vec(bytes)
}

fn store_blob(directory: &Path, content: Option<&[u8]>) -> Result<Option<String>> {
    let Some(content) = content else {
        return Ok(None);
    };
    let digest = hex::encode(Sha256::digest(content));
    let path = directory.join(&digest);
    if !path.exists() {
        fs::write(path, content)?;
    }
    Ok(Some(digest))
}

fn create_visualization_repository(
    source: &Path,
    storage: &Path,
    left: &DiffSide,
    right: &DiffSide,
    files: &[FileChange],
) -> Result<(String, String)> {
    let workspace = storage.join("workspace");
    let output = std::process::Command::new("git")
        .arg("clone")
        .arg("--quiet")
        .arg("--no-checkout")
        .arg("--shared")
        .arg(source)
        .arg(&workspace)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        return Err(AppError::Git {
            command: "git clone --no-checkout --shared".into(),
            message: String::from_utf8_lossy(&output.stderr).trim().into(),
        });
    }
    let (before, after) = match (left, right) {
        (DiffSide::Commit(a), DiffSide::Commit(b)) => (a.clone(), b.clone()),
        _ => {
            let base = rev_parse(&workspace, "HEAD")?;
            let before_tree = build_tree_from_side(source, &workspace, left, files)?;
            let after_tree = build_tree_from_side(source, &workspace, right, files)?;
            let before = commit_tree(&workspace, &before_tree, &base, "lgr snapshot before")?;
            let after = commit_tree(&workspace, &after_tree, &before, "lgr snapshot after")?;
            (before, after)
        }
    };
    materialize_commit(
        &workspace,
        &before,
        &storage.join("before"),
        &storage.join("before.index"),
    )?;
    materialize_commit(
        &workspace,
        &after,
        &storage.join("after"),
        &storage.join("after.index"),
    )?;
    Ok((before, after))
}

fn materialize_commit(
    workspace: &Path,
    commit: &str,
    destination: &Path,
    index: &Path,
) -> Result<()> {
    fs::create_dir_all(destination)?;
    let git_dir = workspace.join(".git");
    let output = std::process::Command::new("git")
        .arg(format!("--git-dir={}", git_dir.display()))
        .arg(format!("--work-tree={}", destination.display()))
        .args(["checkout", "--quiet", commit, "--", "."])
        .env("GIT_INDEX_FILE", index)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Git {
            command: "git checkout captured commit into private workspace".into(),
            message: String::from_utf8_lossy(&output.stderr).trim().into(),
        })
    }
}

fn build_tree_from_side(
    source: &Path,
    workspace: &Path,
    side: &DiffSide,
    files: &[FileChange],
) -> Result<String> {
    if let DiffSide::Commit(commit) = side {
        return git::text(
            workspace,
            &[
                git::os("rev-parse"),
                OsStr::new(&format!("{commit}^{{tree}}")),
            ],
        );
    }
    if matches!(side, DiffSide::Index) {
        return git::text(source, &[git::os("write-tree")]);
    }
    let index_tree = git::text(source, &[git::os("write-tree")])?;
    git::run(workspace, &[git::os("read-tree"), OsStr::new(&index_tree)])?;
    for file in files {
        let (path, content, mode) = match side {
            DiffSide::Index => (
                file.new_path.as_ref().or(file.old_path.as_ref()),
                read_side(
                    source,
                    side,
                    file.new_path.as_ref().or(file.old_path.as_ref()),
                )?,
                &file.new_mode,
            ),
            DiffSide::Worktree => (
                file.new_path.as_ref().or(file.old_path.as_ref()),
                read_side(
                    source,
                    side,
                    file.new_path.as_ref().or(file.old_path.as_ref()),
                )?,
                &file.new_mode,
            ),
            DiffSide::Commit(_) => unreachable!(),
        };
        let Some(path) = path else {
            continue;
        };
        let path_buf = path.to_path_buf()?;
        if let Some(content) = content {
            let mut child = std::process::Command::new("git")
                .current_dir(workspace)
                .args(["hash-object", "-w", "--stdin"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()?;
            use std::io::Write;
            child.stdin.as_mut().unwrap().write_all(&content)?;
            let output = child.wait_with_output()?;
            if !output.status.success() {
                return Err(AppError::Git {
                    command: "git hash-object".into(),
                    message: String::from_utf8_lossy(&output.stderr).into(),
                });
            }
            let object = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let mode = if mode == "100755" {
                "100755"
            } else if mode == "120000" {
                "120000"
            } else {
                "100644"
            };
            git::run(
                workspace,
                &[
                    git::os("update-index"),
                    git::os("--add"),
                    git::os("--cacheinfo"),
                    OsStr::new(mode),
                    OsStr::new(&object),
                    path_buf.as_os_str(),
                ],
            )?;
        } else {
            let _ = git::run_optional(
                workspace,
                &[
                    git::os("update-index"),
                    git::os("--force-remove"),
                    git::os("--"),
                    path_buf.as_os_str(),
                ],
            )?;
        }
    }
    git::text(workspace, &[git::os("write-tree")])
}

fn commit_tree(workspace: &Path, tree: &str, parent: &str, message: &str) -> Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(workspace)
        .args(["commit-tree", tree, "-p", parent])
        .env("GIT_AUTHOR_NAME", "lazy-git-review")
        .env("GIT_AUTHOR_EMAIL", "snapshot@localhost")
        .env("GIT_COMMITTER_NAME", "lazy-git-review")
        .env("GIT_COMMITTER_EMAIL", "snapshot@localhost")
        .env("GIT_AUTHOR_DATE", "1970-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "1970-01-01T00:00:00Z")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    use std::io::Write;
    let mut child = output;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(message.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(AppError::Git {
            command: "git commit-tree".into(),
            message: String::from_utf8_lossy(&output.stderr).into(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn source_fingerprint(repository: &Path, input: &SnapshotInput) -> Result<String> {
    let repository = fs::canonicalize(repository)?;
    ensure_repository(&repository)?;
    let mut digest = Sha256::new();
    match input {
        SnapshotInput::Branch { base, head } => {
            digest.update(b"branch\0");
            digest.update(rev_parse(&repository, base)?);
            digest.update([0]);
            digest.update(rev_parse(&repository, head)?);
        }
        SnapshotInput::Revisions { base, head } => {
            digest.update(b"revisions\0");
            digest.update(rev_parse(&repository, base)?);
            digest.update([0]);
            digest.update(rev_parse(&repository, head)?);
        }
        SnapshotInput::Staged => {
            digest.update(b"staged\0");
            digest.update(rev_parse(&repository, "HEAD")?);
            digest.update([0]);
            digest.update(git::text(&repository, &[git::os("write-tree")])?);
        }
        SnapshotInput::Unstaged { include_untracked } => {
            digest.update(b"unstaged\0");
            digest.update(git::text(&repository, &[git::os("write-tree")])?);
            digest.update([0]);
            digest.update(tracked_diff(&repository, None)?);
            if *include_untracked {
                update_untracked_digest(&repository, &mut digest)?;
            }
        }
        SnapshotInput::Uncommitted => {
            digest.update(b"uncommitted\0");
            let head = rev_parse(&repository, "HEAD")?;
            digest.update(&head);
            digest.update([0]);
            digest.update(tracked_diff(&repository, Some(&head))?);
            update_untracked_digest(&repository, &mut digest)?;
        }
    }
    Ok(hex::encode(digest.finalize()))
}

fn tracked_diff(repository: &Path, base: Option<&str>) -> Result<Vec<u8>> {
    let mut args = vec![
        OsString::from("diff"),
        OsString::from("--binary"),
        OsString::from("--full-index"),
        OsString::from("--find-renames"),
        OsString::from("--no-ext-diff"),
        OsString::from("--no-textconv"),
    ];
    if let Some(base) = base {
        args.push(base.into());
    }
    args.push(OsString::from("--"));
    let refs: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    Ok(git::run(repository, &refs)?.stdout)
}

fn update_untracked_digest(repository: &Path, digest: &mut Sha256) -> Result<()> {
    let paths = git::run(
        repository,
        &[
            git::os("ls-files"),
            git::os("--others"),
            git::os("--exclude-standard"),
            git::os("-z"),
        ],
    )?
    .stdout;
    for path in paths
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        digest.update((path.len() as u64).to_le_bytes());
        digest.update(path);
        let path = PathBuf::from(OsString::from_vec(path.to_vec()));
        let content = fs::read(repository.join(path))?;
        digest.update((content.len() as u64).to_le_bytes());
        digest.update(content);
    }
    Ok(())
}

fn ensure_repository(repository: &Path) -> Result<()> {
    if git::text(
        repository,
        &[git::os("rev-parse"), git::os("--is-inside-work-tree")],
    )? != "true"
    {
        return Err(AppError::InvalidInput {
            code: "not_git_repository",
            message: format!("{} is not a Git work tree", repository.display()),
        });
    }
    Ok(())
}

fn conflicted_paths(repository: &Path) -> Result<Vec<Vec<u8>>> {
    let output = git::run(
        repository,
        &[
            git::os("diff"),
            git::os("--name-only"),
            git::os("--diff-filter=U"),
            git::os("-z"),
        ],
    )?;
    Ok(output
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(<[u8]>::to_vec)
        .collect())
}

fn rev_parse(repository: &Path, value: &str) -> Result<String> {
    git::text(
        repository,
        &[
            git::os("rev-parse"),
            git::os("--verify"),
            OsStr::new(&format!("{value}^{{commit}}")),
        ],
    )
}

fn invalid_diff(message: &str) -> AppError {
    AppError::InvalidInput {
        code: "invalid_git_diff",
        message: message.into(),
    }
}

pub fn load(path: &Path) -> Result<Snapshot> {
    let mut snapshot: Snapshot = serde_json::from_slice(&fs::read(path)?)?;
    if let Some(storage_dir) = path.parent() {
        snapshot.storage_dir = storage_dir.to_path_buf();
    }
    Ok(snapshot)
}

pub fn blobs(snapshot: &Snapshot) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut result = BTreeMap::new();
    for digest in snapshot
        .files
        .iter()
        .flat_map(|file| [file.before_blob.as_ref(), file.after_blob.as_ref()])
        .flatten()
    {
        if !result.contains_key(digest) {
            result.insert(
                digest.clone(),
                fs::read(snapshot.storage_dir.join("blobs").join(digest))?,
            );
        }
    }
    Ok(result)
}

pub fn transferable_hunks(old: &Snapshot, new: &Snapshot) -> Vec<(String, String)> {
    fn signatures(snapshot: &Snapshot) -> BTreeMap<String, Vec<String>> {
        let mut result: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for file in &snapshot.files {
            let path = file
                .new_path
                .as_ref()
                .or(file.old_path.as_ref())
                .map(|p| &p.bytes_base64);
            for hunk in &file.hunks {
                let key = format!(
                    "{:?}:{}:{}:{}:{}:{}",
                    path, hunk.old_count, hunk.new_count, hunk.header, hunk.patch, file.status
                );
                result.entry(key).or_default().push(hunk.id.clone());
            }
        }
        result
    }
    let old = signatures(old);
    let new = signatures(new);
    old.into_iter()
        .filter_map(|(signature, old_ids)| {
            let new_ids = new.get(&signature)?;
            (old_ids.len() == 1 && new_ids.len() == 1)
                .then(|| (old_ids[0].clone(), new_ids[0].clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn git_ok(repo: &Path, args: &[&str]) {
        git::run(repo, &args.iter().map(OsStr::new).collect::<Vec<_>>()).unwrap();
    }
    fn write(repo: &Path, path: &str, body: &[u8]) {
        let path = repo.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }
    fn repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        git_ok(dir.path(), &["init", "-q"]);
        write(dir.path(), "same.txt", b"base\n");
        git_ok(dir.path(), &["add", "."]);
        git_ok(
            dir.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-qm",
                "base",
            ],
        );
        dir
    }

    #[test]
    fn direct_revision_capture_keeps_hunks_and_private_commits() {
        let dir = repo();
        let base = rev_parse(dir.path(), "HEAD").unwrap();
        write(dir.path(), "same.txt", b"base\nchanged\n");
        git_ok(dir.path(), &["add", "."]);
        git_ok(
            dir.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-qm",
                "head",
            ],
        );
        let head = rev_parse(dir.path(), "HEAD").unwrap();
        let data = TempDir::new().unwrap();
        let snapshot = capture(
            dir.path(),
            SnapshotInput::Revisions {
                base: base.clone(),
                head: head.clone(),
            },
            data.path(),
        )
        .unwrap();
        assert_eq!(snapshot.before_commit, base);
        assert_eq!(snapshot.after_commit, head);
        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].hunks.len(), 1);
        assert_eq!(blobs(&snapshot).unwrap().len(), 2);
    }

    #[test]
    fn staged_and_unstaged_are_separate_and_source_is_unchanged() {
        let dir = repo();
        write(dir.path(), "same.txt", b"base\nstaged\n");
        git_ok(dir.path(), &["add", "same.txt"]);
        write(dir.path(), "same.txt", b"base\nstaged\nunstaged\n");
        write(dir.path(), "untracked.txt", b"new\n");
        let status_before = git::run(
            dir.path(),
            &[git::os("status"), git::os("--porcelain=v2"), git::os("-z")],
        )
        .unwrap()
        .stdout;
        let data = TempDir::new().unwrap();
        let staged = capture(dir.path(), SnapshotInput::Staged, data.path()).unwrap();
        let unstaged = capture(
            dir.path(),
            SnapshotInput::Unstaged {
                include_untracked: true,
            },
            data.path(),
        )
        .unwrap();
        assert_eq!(staged.files.len(), 1);
        assert_eq!(unstaged.files.len(), 2);
        assert_eq!(
            status_before,
            git::run(
                dir.path(),
                &[git::os("status"), git::os("--porcelain=v2"), git::os("-z")]
            )
            .unwrap()
            .stdout
        );
        assert_eq!(rev_parse(dir.path(), "HEAD").unwrap(), staged.original_base);
        let staged_source = git::run_optional(
            &staged.storage_dir.join("workspace"),
            &[
                git::os("show"),
                commit_path(&staged.after_commit, Path::new("same.txt")).as_os_str(),
            ],
        )
        .unwrap()
        .unwrap();
        assert_eq!(staged_source, b"base\nstaged\n");
    }

    #[test]
    fn uncommitted_combines_staged_unstaged_and_untracked_changes() {
        let dir = repo();
        write(dir.path(), "same.txt", b"base\nstaged\n");
        git_ok(dir.path(), &["add", "same.txt"]);
        write(dir.path(), "same.txt", b"base\nstaged\nunstaged\n");
        write(dir.path(), "untracked.txt", b"new\n");
        let status_before = git::run(
            dir.path(),
            &[git::os("status"), git::os("--porcelain=v2"), git::os("-z")],
        )
        .unwrap()
        .stdout;

        let data = TempDir::new().unwrap();
        let snapshot = capture(dir.path(), SnapshotInput::Uncommitted, data.path()).unwrap();

        assert_eq!(snapshot.files.len(), 2);
        let paths = snapshot
            .files
            .iter()
            .filter_map(|file| file.new_path.as_ref().map(|path| path.display.as_str()))
            .collect::<BTreeSet<_>>();
        assert_eq!(paths, BTreeSet::from(["same.txt", "untracked.txt"]));
        assert_eq!(
            status_before,
            git::run(
                dir.path(),
                &[git::os("status"), git::os("--porcelain=v2"), git::os("-z")]
            )
            .unwrap()
            .stdout
        );
        assert_eq!(
            fs::read(dir.path().join("same.txt")).unwrap(),
            b"base\nstaged\nunstaged\n"
        );
    }

    #[test]
    fn branch_capture_uses_merge_base() {
        let dir = repo();
        git_ok(dir.path(), &["branch", "target"]);
        write(dir.path(), "feature.txt", b"feature\n");
        git_ok(dir.path(), &["add", "."]);
        git_ok(
            dir.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-qm",
                "feature",
            ],
        );
        let feature = rev_parse(dir.path(), "HEAD").unwrap();
        git_ok(dir.path(), &["checkout", "-q", "target"]);
        write(dir.path(), "target.txt", b"target\n");
        git_ok(dir.path(), &["add", "."]);
        git_ok(
            dir.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-qm",
                "target",
            ],
        );
        let data = TempDir::new().unwrap();
        let snapshot = capture(
            dir.path(),
            SnapshotInput::Branch {
                base: "target".into(),
                head: feature,
            },
            data.path(),
        )
        .unwrap();
        assert!(snapshot.files.iter().any(|f| {
            f.new_path
                .as_ref()
                .is_some_and(|p| p.display == "feature.txt")
        }));
        assert!(!snapshot.files.iter().any(|f| {
            f.new_path
                .as_ref()
                .is_some_and(|p| p.display == "target.txt")
        }));
        assert_ne!(snapshot.original_base, snapshot.comparison_base);
    }

    #[test]
    fn inventory_retains_binary_delete_rename_and_mode_only() {
        let dir = repo();
        write(dir.path(), "binary.bin", &[0, 1, 2]);
        write(dir.path(), "delete.txt", b"gone\n");
        write(dir.path(), "rename.txt", b"renamed\n");
        write(dir.path(), "mode.sh", b"#!/bin/sh\n");
        git_ok(dir.path(), &["add", "."]);
        git_ok(
            dir.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-qm",
                "inventory base",
            ],
        );
        let base = rev_parse(dir.path(), "HEAD").unwrap();
        fs::remove_file(dir.path().join("delete.txt")).unwrap();
        fs::rename(
            dir.path().join("rename.txt"),
            dir.path().join("renamed.txt"),
        )
        .unwrap();
        write(dir.path(), "binary.bin", &[0, 2, 3]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                dir.path().join("mode.sh"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        git_ok(dir.path(), &["add", "-A"]);
        git_ok(
            dir.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-qm",
                "inventory head",
            ],
        );
        let head = rev_parse(dir.path(), "HEAD").unwrap();
        let data = TempDir::new().unwrap();
        let snapshot = capture(
            dir.path(),
            SnapshotInput::Revisions { base, head },
            data.path(),
        )
        .unwrap();
        assert!(snapshot.files.iter().any(|f| f.binary));
        assert!(snapshot.files.iter().any(|f| f.status.starts_with('D')));
        assert!(snapshot.files.iter().any(|f| f.status.starts_with('R')));
        assert!(
            snapshot
                .files
                .iter()
                .any(|f| f.old_mode != f.new_mode && f.hunks.is_empty())
        );
    }

    #[test]
    fn raw_parser_keeps_rename_paths_and_special_bytes() {
        let raw = b":100644 100644 1111111 2222222 R100\0old name\0new\tname\0";
        let changes = parse_raw(raw).unwrap();
        assert_eq!(changes[0].status, "R100");
        assert_eq!(
            changes[0].old_path.as_ref().unwrap().to_path_buf().unwrap(),
            PathBuf::from("old name")
        );
        assert_eq!(
            changes[0].new_path.as_ref().unwrap().to_path_buf().unwrap(),
            PathBuf::from("new\tname")
        );
    }

    #[test]
    fn hunk_parser_handles_optional_counts() {
        let hunks = parse_hunks("@@ -1 +1,2 @@ name\n-old\n+new\n+more\n").unwrap();
        assert_eq!(
            (
                hunks[0].old_start,
                hunks[0].old_count,
                hunks[0].new_start,
                hunks[0].new_count
            ),
            (1, 1, 1, 2)
        );
    }

    #[test]
    fn refresh_transfer_requires_unique_unchanged_hunk() {
        let dir = repo();
        let base = rev_parse(dir.path(), "HEAD").unwrap();
        write(dir.path(), "same.txt", b"base\nfirst\n");
        git_ok(dir.path(), &["add", "."]);
        git_ok(
            dir.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-qm",
                "first",
            ],
        );
        let first = rev_parse(dir.path(), "HEAD").unwrap();
        let data = TempDir::new().unwrap();
        let old = capture(
            dir.path(),
            SnapshotInput::Revisions {
                base: base.clone(),
                head: first,
            },
            data.path(),
        )
        .unwrap();
        write(dir.path(), "same.txt", b"base\nsecond\n");
        git_ok(dir.path(), &["add", "."]);
        git_ok(
            dir.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-qm",
                "second",
            ],
        );
        let second = rev_parse(dir.path(), "HEAD").unwrap();
        let new = capture(
            dir.path(),
            SnapshotInput::Revisions { base, head: second },
            data.path(),
        )
        .unwrap();
        assert!(transferable_hunks(&old, &new).is_empty());
    }
}

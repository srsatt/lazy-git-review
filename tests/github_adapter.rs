use std::fs;
use std::path::{Path, PathBuf};

use lazy_git_review::comments::{CommentAnchor, DraftStore};
use lazy_git_review::git;
use lazy_git_review::github::{GitHubAdapter, GitHubConfig, ReviewEvent, intent_path};
use lazy_git_review::graph::SourceSide;
use lazy_git_review::snapshot::{Snapshot, SnapshotInput, capture};
use tempfile::TempDir;

fn fixture() -> (TempDir, TempDir, Snapshot, String, String) {
    let repo = TempDir::new().unwrap();
    git::run(repo.path(), &[git::os("init"), git::os("-q")]).unwrap();
    fs::write(
        repo.path().join("a.ts"),
        "export const value = 1;\nexport const other = 1;\nkeep();\n",
    )
    .unwrap();
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
    fs::write(
        repo.path().join("a.ts"),
        "export const value = 2;\nexport const other = 2;\nkeep();\n",
    )
    .unwrap();
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
        SnapshotInput::Revisions {
            base: base.clone(),
            head: head.clone(),
        },
        data.path(),
    )
    .unwrap();
    (repo, data, snapshot, base, head)
}

fn adapter(state: &Path, mode: &str, base: &str, head: &str) -> GitHubAdapter {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_github_cli.py");
    GitHubAdapter::new(GitHubConfig {
        command: vec![
            "python3".into(),
            script.into_os_string(),
            state.into(),
            mode.into(),
            base.into(),
            head.into(),
        ],
        host: "github.com".into(),
        repository: "example/repository".into(),
        expected_account: "reviewer".into(),
    })
    .unwrap()
}

fn drafts(snapshot: &Snapshot) -> DraftStore {
    let mut drafts = DraftStore::default();
    drafts
        .add(
            "Please verify this behavior".into(),
            Some(CommentAnchor {
                snapshot_id: snapshot.id.to_string(),
                side: SourceSide::Right,
                path: snapshot.files[0].new_path.clone().unwrap(),
                start_line: 1,
                end_line: 2,
                source_fingerprint: "source".into(),
            }),
        )
        .unwrap();
    drafts
        .add(
            "Left-side feedback".into(),
            Some(CommentAnchor {
                snapshot_id: snapshot.id.to_string(),
                side: SourceSide::Left,
                path: snapshot.files[0].old_path.clone().unwrap(),
                start_line: 1,
                end_line: 1,
                source_fingerprint: "source".into(),
            }),
        )
        .unwrap();
    drafts
}

#[test]
fn reads_fork_and_all_pages_then_previews_exact_anchor() {
    let (_repo, _data, snapshot, base, head) = fixture();
    let state = snapshot.storage_dir.join("mutations");
    let adapter = adapter(&state, "normal", &base, &head);
    let pull = adapter.fetch_pull(7).unwrap();
    assert_eq!(pull.head_repository, "fork/example");
    assert_eq!(pull.head_ref, "feature/JT-95384-settings");
    assert_eq!(pull.issue_comments.len(), 2);
    assert_eq!(pull.review_comments[0]["original_position"], 2);
    let drafts = drafts(&snapshot);
    let ids: Vec<_> = drafts.drafts.keys().cloned().collect();
    let preview = adapter
        .preview(
            &pull,
            &snapshot,
            &drafts,
            &ids,
            ReviewEvent::Comment,
            "Summary".into(),
        )
        .unwrap();
    assert!(
        preview
            .comments
            .iter()
            .all(|comment| comment.path == "a.ts")
    );
    let right = preview
        .comments
        .iter()
        .find(|comment| comment.side == "RIGHT")
        .unwrap();
    assert_eq!(right.line, 2);
    assert_eq!(right.start_line, Some(1));
    assert_eq!(right.start_side.as_deref(), Some("RIGHT"));
    assert!(
        preview
            .comments
            .iter()
            .any(|comment| comment.side == "LEFT")
    );
}

#[test]
fn verifies_identity_and_never_blindly_retries_uncertain_submission() {
    let (_repo, _data, snapshot, base, head) = fixture();
    let drafts = drafts(&snapshot);
    let ids: Vec<_> = drafts.drafts.keys().cloned().collect();
    let state = snapshot.storage_dir.join("mutations");
    let normal = adapter(&state, "normal", &base, &head);
    let pull = normal.fetch_pull(7).unwrap();
    let preview = normal
        .preview(
            &pull,
            &snapshot,
            &drafts,
            &ids,
            ReviewEvent::Comment,
            "Summary".into(),
        )
        .unwrap();
    let wrong = adapter(&state, "wrong_identity", &base, &head);
    assert!(
        wrong
            .submit(&preview, &pull, &drafts, &intent_path(&snapshot))
            .is_err()
    );
    assert!(!state.exists());

    let lost_intent = snapshot.storage_dir.join("lost-intent.json");
    let lost = adapter(&state, "lost_response", &base, &head);
    assert!(lost.submit(&preview, &pull, &drafts, &lost_intent).is_err());
    assert_eq!(fs::read_to_string(&state).unwrap(), "1");
    assert!(lost.submit(&preview, &pull, &drafts, &lost_intent).is_err());
    assert_eq!(fs::read_to_string(&state).unwrap(), "1");
    let reconcile = adapter(&state, "reconcile", &base, &head);
    assert_eq!(
        reconcile.reconcile(&preview, &lost_intent).unwrap()["id"],
        9001
    );
    assert!(
        reconcile
            .submit(&preview, &pull, &drafts, &lost_intent)
            .is_ok()
    );
    assert_eq!(fs::read_to_string(&state).unwrap(), "1");
}

#[test]
fn omits_multiline_fields_from_single_line_submission_comments() {
    let (_repo, _data, snapshot, base, head) = fixture();
    let drafts = drafts(&snapshot);
    let ids: Vec<_> = drafts.drafts.keys().cloned().collect();
    let state = snapshot.storage_dir.join("mutations");
    let adapter = adapter(&state, "validate_payload", &base, &head);
    let pull = adapter.fetch_pull(7).unwrap();
    let preview = adapter
        .preview(
            &pull,
            &snapshot,
            &drafts,
            &ids,
            ReviewEvent::Comment,
            "Summary".into(),
        )
        .unwrap();

    let response = adapter
        .submit(
            &preview,
            &pull,
            &drafts,
            &snapshot.storage_dir.join("payload-intent.json"),
        )
        .unwrap();

    assert_eq!(response["id"], 9001);
}

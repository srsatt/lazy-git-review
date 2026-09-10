use std::fs;

use lazy_git_review::comments::{CommentAnchor, DraftStore};
use lazy_git_review::git;
use lazy_git_review::github::{GitHubAdapter, GitHubConfig, PullRequestContext, ReviewEvent};
use lazy_git_review::graph::{ChangeGraph, NodeKind, SourceSide};
use lazy_git_review::ranking::{Assessment, Authority, RankingState};
use lazy_git_review::snapshot::{SnapshotInput, capture};
use tempfile::TempDir;

#[test]
fn snapshot_to_graph_rank_comment_markdown_and_github_preview_preserves_source() {
    let repo = TempDir::new().unwrap();
    git::run(repo.path(), &[git::os("init"), git::os("-q")]).unwrap();
    fs::write(
        repo.path().join("logic.ts"),
        "export function logic() { return 1; }\n",
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
        repo.path().join("logic.ts"),
        "export function logic() { return 2; }\n",
    )
    .unwrap();
    let status_before = git::run(
        repo.path(),
        &[git::os("status"), git::os("--porcelain=v2"), git::os("-z")],
    )
    .unwrap()
    .stdout;
    let index_before = git::text(repo.path(), &[git::os("write-tree")]).unwrap();
    let data = TempDir::new().unwrap();
    let snapshot = capture(
        repo.path(),
        SnapshotInput::Unstaged {
            include_untracked: false,
        },
        data.path(),
    )
    .unwrap();
    let graph = ChangeGraph::from_snapshot(&snapshot);
    let hunk = graph
        .nodes
        .values()
        .find(|node| node.kind == NodeKind::Hunk)
        .unwrap();
    let mut ranking = RankingState::new(&graph, None);
    ranking
        .apply_batch(
            &graph,
            graph.revision.as_str(),
            vec![Assessment {
                node_id: hunk.id.clone(),
                title: Some("Change core behavior".into()),
                score: 90,
                tags: vec!["non-trivial-logic".into()],
                rationale: "Core behavior changed".into(),
                confidence: 0.9,
                evidence_ids: vec![hunk.id.clone()],
                authority: Authority::Model,
            }],
        )
        .unwrap();
    let queue = ranking.finalize(&graph).unwrap();
    assert_eq!(queue.assessed_changes, queue.total_changes);

    let location = &hunk.locations[0];
    let mut drafts = DraftStore::default();
    let comment = drafts
        .add(
            "Review the changed return value".into(),
            Some(CommentAnchor {
                snapshot_id: snapshot.id.to_string(),
                side: SourceSide::Right,
                path: location.path.clone(),
                start_line: 1,
                end_line: 1,
                source_fingerprint: snapshot.source_fingerprint.clone(),
            }),
        )
        .unwrap();
    drafts
        .set_plugin_id(comment.as_str(), "code-review-1".into())
        .unwrap();
    let markdown = data.path().join("review.md");
    drafts.export_markdown(&snapshot, &markdown).unwrap();
    let edited = fs::read_to_string(&markdown).unwrap().replace(
        "Review the changed return value",
        "Check the changed return value",
    );
    fs::write(&markdown, edited).unwrap();
    drafts.import_markdown(&snapshot, &markdown).unwrap();
    assert_eq!(
        drafts.drafts[comment.as_str()].body,
        "Check the changed return value"
    );

    let pull = PullRequestContext {
        number: 1,
        host: "github.com".into(),
        base_repository: "example/repository".into(),
        head_repository: "fork/example".into(),
        title: "Change".into(),
        body: String::new(),
        head_ref: "feature/JT-1-change".into(),
        base_sha: base,
        head_sha: snapshot.after_commit.clone(),
        issue_comments: vec![],
        review_comments: vec![],
        reviews: vec![],
        captured_at: chrono::Utc::now(),
    };
    let adapter = GitHubAdapter::new(GitHubConfig {
        command: vec!["gh".into()],
        host: "github.com".into(),
        repository: "example/repository".into(),
        expected_account: "reviewer".into(),
    })
    .unwrap();
    let preview = adapter
        .preview(
            &pull,
            &snapshot,
            &drafts,
            &[comment.to_string()],
            ReviewEvent::Comment,
            "Summary".into(),
        )
        .unwrap();
    assert_eq!(preview.comments[0].body, "Check the changed return value");
    assert_eq!(
        git::run(
            repo.path(),
            &[git::os("status"), git::os("--porcelain=v2"), git::os("-z")]
        )
        .unwrap()
        .stdout,
        status_before
    );
    assert_eq!(
        git::text(repo.path(), &[git::os("write-tree")]).unwrap(),
        index_before
    );
}

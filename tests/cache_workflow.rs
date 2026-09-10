use std::fs;
use std::path::Path;

use assert_cmd::Command;
use lazy_git_review::context::ContextBundle;
use lazy_git_review::git;
use lazy_git_review::graph::{ChangeGraph, NodeKind};
use lazy_git_review::ranking::{Assessment, Authority, RankingState, ranking_path};
use lazy_git_review::snapshot::{self, Snapshot};
use serde_json::Value;
use tempfile::TempDir;

fn run(home: &TempDir, data: &Path, args: &[&str]) -> Value {
    let output = Command::cargo_bin("lgr")
        .unwrap()
        .env("HOME", home.path())
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "command failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn commit_base(repository: &Path) {
    git::run(repository, &[git::os("init"), git::os("-q")]).unwrap();
    fs::write(repository.join("change.txt"), "base\n").unwrap();
    git::run(repository, &[git::os("add"), git::os(".")]).unwrap();
    git::run(
        repository,
        &[
            git::os("-c"),
            git::os("user.name=T"),
            git::os("-c"),
            git::os("user.email=t@example.test"),
            git::os("commit"),
            git::os("-qm"),
            git::os("base"),
        ],
    )
    .unwrap();
}

#[test]
fn unchanged_launch_reuses_snapshot_graph_and_ranking_but_content_or_context_invalidates() {
    let home = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    let ranker = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mock_ranker.py")
        .to_string_lossy()
        .into_owned();
    commit_base(repository.path());
    fs::write(repository.path().join("change.txt"), "first edit\n").unwrap();

    let repository_arg = repository.path().to_str().unwrap();
    let first = run(
        &home,
        data.path(),
        &[
            "review",
            "create",
            "--repository",
            repository_arg,
            "--reuse",
            "--uncommitted",
        ],
    );
    assert_eq!(first["result"]["cache_hit"], false);
    let session = first["result"]["session"]["id"].as_str().unwrap();

    let second = run(
        &home,
        data.path(),
        &[
            "review",
            "create",
            "--repository",
            repository_arg,
            "--reuse",
            "--uncommitted",
        ],
    );
    assert_eq!(second["result"]["cache_hit"], true);
    assert_eq!(second["result"]["session"]["id"], session);

    let first_graph = run(&home, data.path(), &["graph", "build", session]);
    assert_eq!(first_graph["result"]["cache_hit"], false);
    let second_graph = run(&home, data.path(), &["graph", "build", session]);
    assert_eq!(second_graph["result"]["cache_hit"], true);
    assert_eq!(
        first_graph["result"]["graph_revision"],
        second_graph["result"]["graph_revision"]
    );

    let snapshot_path = first["result"]["snapshot"]["storage_dir"]
        .as_str()
        .map(Path::new)
        .unwrap()
        .join("snapshot.json");
    let snapshot: Snapshot = snapshot::load(&snapshot_path).unwrap();
    let graph_path = snapshot::load(&snapshot_path)
        .map(|value| lazy_git_review::indexer::graph_path(&value))
        .unwrap();
    let graph = ChangeGraph::load(&graph_path).unwrap();
    let mut ranking = RankingState::new(&graph, None);
    let manually_ranked = graph
        .nodes
        .values()
        .find(|node| matches!(node.kind, NodeKind::Hunk | NodeKind::FileChange))
        .unwrap()
        .id
        .clone();
    ranking
        .apply_batch(
            &graph,
            graph.revision.as_str(),
            vec![Assessment {
                node_id: manually_ranked.clone(),
                title: None,
                score: 75,
                tags: vec!["manual".into()],
                rationale: "Reviewer priority".into(),
                confidence: 1.0,
                evidence_ids: vec![manually_ranked.clone()],
                authority: Authority::Manual,
            }],
        )
        .unwrap();
    ranking.finalize(&graph).unwrap();
    ranking.save(&ranking_path(&graph_path)).unwrap();

    let revision = graph.revision.to_string();
    let label_updates = serde_json::json!([{
        "node_id": manually_ranked.clone(),
        "title": "Explain the first edit",
        "evidence_ids": [manually_ranked.clone()],
        "authority": "manual"
    }])
    .to_string();
    let labeled = run(
        &home,
        data.path(),
        &[
            "graph",
            "label",
            session,
            "--graph-revision",
            &revision,
            "--updates",
            &label_updates,
        ],
    );
    assert_eq!(
        labeled["result"]["queue"]["items"][0]["title"],
        "Explain the first edit"
    );
    assert_eq!(labeled["result"]["queue"]["items"][0]["score"], 75);
    assert_eq!(labeled["result"]["ranking"]["finalized"], true);

    let cached_titles = run(&home, data.path(), &["rank", session, "--titles-only"]);
    assert_eq!(cached_titles["result"]["cache_hit"], true);
    let forced_titles = run(
        &home,
        data.path(),
        &["-a", "true", "rank", session, "--titles-only", "--force"],
    );
    assert_eq!(forced_titles["result"]["launched"], true);
    let after_titles: RankingState =
        serde_json::from_slice(&fs::read(ranking_path(&graph_path)).unwrap()).unwrap();
    assert!(after_titles.finalized);
    assert_eq!(after_titles.assessments[&manually_ranked].score, 75);

    let cached_rank = run(&home, data.path(), &["rank", session]);
    assert_eq!(cached_rank["result"]["cache_hit"], true);
    assert_eq!(cached_rank["result"]["launched"], false);

    let sessions = run(
        &home,
        data.path(),
        &["session", "list", "--repository", repository_arg],
    );
    assert_eq!(sessions["result"]["sessions"][0]["id"], session);
    assert_eq!(sessions["result"]["sessions"][0]["stage"], "ready");
    assert_eq!(sessions["result"]["sessions"][0]["files"], 1);
    assert!(
        sessions["result"]["sessions"][0]["changes"]
            .as_u64()
            .unwrap()
            > 0
    );

    let forced_rank = run(
        &home,
        data.path(),
        &["-a", &ranker, "rank", session, "--force"],
    );
    assert_eq!(forced_rank["result"]["cache_hit"], false);
    assert_eq!(forced_rank["result"]["launched"], true);

    let mut context = ContextBundle::default();
    context.add_markdown("test", "new review intent".into(), 1024);
    context
        .save(&snapshot.storage_dir.join("context.json"))
        .unwrap();
    let expected_context_digest = context.digest.clone();
    let stale_rank = run(&home, data.path(), &["-a", &ranker, "rank", session]);
    assert_eq!(stale_rank["result"]["cache_hit"], false);
    assert_eq!(stale_rank["result"]["launched"], true);
    let restarted: RankingState =
        serde_json::from_slice(&fs::read(ranking_path(&graph_path)).unwrap()).unwrap();
    assert_eq!(restarted.context_digest, Some(expected_context_digest));
    assert!(restarted.finalized);
    assert_eq!(
        restarted.assessments[&manually_ranked].authority,
        Authority::Manual
    );
    assert!(snapshot.storage_dir.read_dir().unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("ranking.stale-")
    }));

    fs::write(repository.path().join("change.txt"), "second edit\n").unwrap();
    let changed = run(
        &home,
        data.path(),
        &[
            "review",
            "create",
            "--repository",
            repository_arg,
            "--reuse",
            "--uncommitted",
        ],
    );
    assert_eq!(changed["result"]["cache_hit"], false);
    assert_ne!(changed["result"]["session"]["id"], session);
}

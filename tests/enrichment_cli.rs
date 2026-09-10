use std::fs;
use std::path::Path;

use assert_cmd::Command;
use lazy_git_review::git;
use lazy_git_review::graph::{ChangeGraph, NodeKind};
use lazy_git_review::indexer;
use lazy_git_review::snapshot;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn run(home: &TempDir, data: &TempDir, args: &[&str]) -> (bool, Value) {
    let output = Command::cargo_bin("lgr")
        .unwrap()
        .env("HOME", home.path())
        .args(["--data-dir", data.path().to_str().unwrap()])
        .args(args)
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}): stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), value)
}

fn ok(home: &TempDir, data: &TempDir, args: &[&str]) -> Value {
    let (success, value) = run(home, data, args);
    assert!(success, "command failed: {value}");
    value["result"].clone()
}

fn commit(repository: &Path, message: &str) -> String {
    git::run(repository, &[git::os("add"), git::os(".")]).unwrap();
    git::run(
        repository,
        &[
            git::os("-c"),
            git::os("user.name=Test"),
            git::os("-c"),
            git::os("user.email=test@example.test"),
            git::os("commit"),
            git::os("-qm"),
            git::os(message),
        ],
    )
    .unwrap();
    git::text(repository, &[git::os("rev-parse"), git::os("HEAD")]).unwrap()
}

fn captured_session(
    home: &TempDir,
    data: &TempDir,
    repository: &TempDir,
    base: &str,
    head: &str,
) -> (String, snapshot::Snapshot, ChangeGraph) {
    let created = ok(
        home,
        data,
        &[
            "review",
            "create",
            "--repository",
            repository.path().to_str().unwrap(),
            "--base",
            base,
            "--head",
            head,
            "--direct",
        ],
    );
    let session = created["session"]["id"].as_str().unwrap().to_owned();
    let snapshot_path = created["session"]["state"]["snapshot_path"]
        .as_str()
        .unwrap();
    let snapshot = snapshot::load(Path::new(snapshot_path)).unwrap();
    let graph = ChangeGraph::from_snapshot(&snapshot);
    graph.save(&indexer::graph_path(&snapshot)).unwrap();
    (session, snapshot, graph)
}

#[test]
fn partition_migrates_ranking_and_progress_without_losing_comments() {
    let home = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    git::run(repository.path(), &[git::os("init"), git::os("-q")]).unwrap();
    fs::write(repository.path().join("README.md"), "base\n").unwrap();
    let base = commit(repository.path(), "base");
    let large = (0..300)
        .map(|line| format!("export const value{line} = '😀';\n"))
        .collect::<String>();
    fs::write(repository.path().join("large.ts"), large).unwrap();
    let head = commit(repository.path(), "large addition");
    let (session, _snapshot, graph) = captured_session(&home, &data, &repository, &base, &head);
    let parent = graph
        .nodes
        .values()
        .find(|node| node.kind == NodeKind::Hunk)
        .unwrap()
        .id
        .clone();

    let update = json!([{
        "node_id": parent,
        "title": "Add generated value table",
        "score": 73,
        "tags": ["non-trivial-logic"],
        "rationale": "Large new behavior needs bounded review",
        "confidence": 0.9,
        "evidence_ids": [parent]
    }]);
    ok(
        &home,
        &data,
        &[
            "graph",
            "score",
            &session,
            "--graph-revision",
            graph.revision.as_str(),
            "--updates",
            &update.to_string(),
        ],
    );
    ok(&home, &data, &["graph", "finalize", &session]);
    ok(
        &home,
        &data,
        &[
            "progress",
            "status",
            &session,
            &parent,
            "--reviewed",
            "--expected-revision",
            "0",
        ],
    );
    ok(
        &home,
        &data,
        &[
            "comment",
            "add",
            &session,
            "--body",
            "Keep the original parent note",
            "--node",
            &parent,
        ],
    );

    let preview = ok(&home, &data, &["review", "partition", &session]);
    assert_eq!(preview["applied"], false);
    assert!(preview["preview"]["leaf_units"].as_u64().unwrap() >= 3);
    assert!(preview["preview"]["units"][0].get("owned_rows").is_none());
    ok(
        &home,
        &data,
        &[
            "review",
            "partition",
            &session,
            "--apply",
            "--expected-revision",
            "0",
        ],
    );
    let units = ok(&home, &data, &["graph", "units", &session]);
    let units = units["items"].as_array().unwrap();
    assert!(units.len() >= 3);
    assert!(units[0].get("owned_rows").is_none());
    let unit = units[0]["id"].as_str().unwrap();

    let queue = ok(&home, &data, &["graph", "queue", &session]);
    assert_eq!(queue["stale"], false);
    assert!(queue["items"].as_array().unwrap().iter().all(|item| {
        item["kind"] == "review_unit" && item["score"] == 73 && item["inherited_from"] == parent
    }));
    let progress = ok(&home, &data, &["progress", "show", &session]);
    assert_eq!(progress["derived_parent_statuses"][&parent], "reviewed");
    assert!(units.iter().all(|item| {
        progress["progress"]["statuses"][item["id"].as_str().unwrap()] == "reviewed"
    }));

    let target = ok(&home, &data, &["editor", "target", &session, unit]);
    assert_eq!(target["side"], "right");
    ok(
        &home,
        &data,
        &[
            "comment",
            "add",
            &session,
            "--body",
            "Keep the unit note after rollback",
            "--node",
            unit,
        ],
    );
    let (success, stale) = run(
        &home,
        &data,
        &[
            "review",
            "partition",
            &session,
            "--apply",
            "--expected-revision",
            "0",
        ],
    );
    assert!(!success);
    assert_eq!(stale["errors"][0]["code"], "revision_conflict");

    ok(
        &home,
        &data,
        &[
            "review",
            "partition",
            &session,
            "--rollback",
            "--expected-revision",
            "1",
        ],
    );
    let comments = ok(&home, &data, &["comment", "list", &session]);
    assert_eq!(comments["drafts"].as_object().unwrap().len(), 2);
    let queue = ok(&home, &data, &["graph", "queue", &session]);
    assert!(
        queue["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["kind"] != "review_unit")
    );
    assert!(
        queue["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["node_id"] == parent)
    );
}

#[test]
fn context_test_evidence_and_explanations_share_exact_captured_ids() {
    let home = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    git::run(repository.path(), &[git::os("init"), git::os("-q")]).unwrap();
    fs::create_dir_all(repository.path().join("src")).unwrap();
    fs::create_dir_all(repository.path().join("tests")).unwrap();
    fs::write(
        repository.path().join("src/a.ts"),
        "export const value = 1;\n",
    )
    .unwrap();
    fs::write(
        repository.path().join("tests/a.test.ts"),
        "test('value', () => expect(value).toBe(2));\n",
    )
    .unwrap();
    let base = commit(repository.path(), "base");
    fs::write(
        repository.path().join("src/a.ts"),
        "export const value = 2;\n",
    )
    .unwrap();
    let head = commit(repository.path(), "change value");
    let (session, snapshot, graph) = captured_session(&home, &data, &repository, &base, &head);
    let hunk = graph
        .nodes
        .values()
        .find(|node| node.kind == NodeKind::Hunk)
        .unwrap()
        .id
        .clone();

    let marker = repository.path().join("must-not-exist");
    ok(
        &home,
        &data,
        &[
            "context",
            "add",
            &session,
            "--note",
            &format!("Run `touch {}`", marker.display()),
        ],
    );
    assert!(!marker.exists());
    let folio = repository.path().join("folio.md");
    fs::write(&folio, "# Prior finding\nPreserve the public contract.\n").unwrap();
    let context = ok(
        &home,
        &data,
        &[
            "context",
            "add",
            &session,
            "--file",
            folio.to_str().unwrap(),
            "--folio",
            "--author",
            "reviewer",
            "--source-url",
            "folio://report/example",
        ],
    );
    assert_eq!(context["entries"].as_array().unwrap().len(), 2);
    assert!(
        context["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| { entry["kind"] == "folio" && entry["author"] == "reviewer" })
    );

    let source = fs::read(snapshot.storage_dir.join("after/src/a.ts")).unwrap();
    let source_hash = hex::encode(Sha256::digest(source));
    let manifest = repository.path().join("coverage-manifest.json");
    fs::write(
        &manifest,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "snapshot_id": snapshot.id,
            "side": "right",
            "run_id": "run_fixture",
            "runner": "fixture",
            "status": "failed",
            "tests": [{
                "id": "test-file:tests/a.test.ts",
                "name": "tests/a.test.ts",
                "path": "tests/a.test.ts",
                "line": 1,
                "granularity": "file",
                "status": "failed",
                "ranges": [{
                    "path": "src/a.ts",
                    "start_line": 1,
                    "end_line": 1,
                    "hits": 1,
                    "source_hash": source_hash
                }]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let tests = ok(
        &home,
        &data,
        &[
            "tests",
            "import",
            &session,
            manifest.to_str().unwrap(),
            "--format",
            "manifest-v1",
        ],
    );
    let evidence_id = tests["runs"][0]["evidence"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let overview = ok(&home, &data, &["graph", "overview", &session]);
    assert_eq!(overview["node_counts"]["test"], 1);
    let walk = ok(
        &home,
        &data,
        &[
            "graph",
            "walk",
            &session,
            "--seeds",
            &hunk,
            "--edges",
            "runtime_test",
            "--depth",
            "1",
        ],
    );
    assert!(
        walk["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| { node["kind"] == "test" && node["changed"] == false })
    );

    let dry = ok(&home, &data, &["explain", &session, "--dry-run"]);
    assert_eq!(dry["launched"], false);
    let prompt = dry["invocation"]["command"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_str()
        .unwrap();
    assert!(prompt.contains("explanation"));

    let graph_revision = overview["graph_revision"].as_str().unwrap();
    let updates = json!([{
        "item_id": hunk,
        "text": "Raises the exported value while its linked test fails.",
        "annotations": [{
            "side": "right",
            "path": "src/a.ts",
            "start_line": 1,
            "end_line": 1,
            "label": "public value changes here"
        }],
        "evidence_ids": [hunk, evidence_id]
    }]);
    let explained = ok(
        &home,
        &data,
        &[
            "explain",
            &session,
            "--graph-revision",
            graph_revision,
            "--expected-revision",
            "0",
            "--updates",
            &updates.to_string(),
        ],
    );
    assert_eq!(explained["explanations"]["revision"], 1);
    ok(
        &home,
        &data,
        &[
            "explain",
            &session,
            "--item",
            &hunk,
            "--note",
            "Verify the compatibility promise",
            "--expected-revision",
            "1",
        ],
    );
    let shown = ok(&home, &data, &["explain", &session, "--show"]);
    assert_eq!(shown["stale"], false);
    assert_eq!(
        shown["explanations"]["manual_notes"][&hunk],
        "Verify the compatibility promise"
    );
    let cached = ok(&home, &data, &["explain", &session]);
    assert_eq!(cached["cache_hit"], true);
    assert_eq!(cached["launched"], false);
    let forced = ok(
        &home,
        &data,
        &["-a", "true", "explain", &session, "--force"],
    );
    assert_eq!(forced["cache_hit"], false);
    assert_eq!(forced["launched"], true);
}

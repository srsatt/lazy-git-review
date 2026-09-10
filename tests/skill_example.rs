use std::fs;
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use lazy_git_review::git;
use serde_json::Value;
use tempfile::TempDir;

fn lgr(data: &std::path::Path, args: &[&str]) -> Value {
    let output = Command::new(cargo_bin("lgr"))
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn shipped_example_completes_a_ranking() {
    let repo = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    git::run(repo.path(), &[git::os("init"), git::os("-q")]).unwrap();
    fs::write(repo.path().join("a.ts"), "export const value = 1;\n").unwrap();
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
    fs::write(
        repo.path().join("a.ts"),
        format!("export const value = 2;\n// {}\n", "x".repeat(20_000)),
    )
    .unwrap();
    let created = lgr(
        data.path(),
        &[
            "review",
            "create",
            "--repository",
            repo.path().to_str().unwrap(),
            "--unstaged",
        ],
    );
    let session = created["result"]["session"]["id"].as_str().unwrap();
    let built = lgr(data.path(), &["graph", "build", session]);
    let revision = built["result"]["graph_revision"].as_str().unwrap();
    let snapshot_path = created["result"]["snapshot"]["storage_dir"]
        .as_str()
        .unwrap();
    let graph: Value = serde_json::from_slice(
        &fs::read(std::path::Path::new(snapshot_path).join("graph.json")).unwrap(),
    )
    .unwrap();
    let hunk = graph["nodes"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, node)| node["kind"] == "hunk")
        .map(|(id, _)| id)
        .unwrap();
    let bounded = lgr(
        data.path(),
        &["graph", "evidence", session, "--max-bytes", "4096"],
    );
    assert_eq!(bounded["result"]["evidence_complete"], false);
    assert!(bounded["result"]["items"][0]["next_offset"].is_number());
    assert!(serde_json::to_vec(&bounded).unwrap().len() <= 4096);
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("skills/semantic-review-ranker/scripts/example-ranking.sh");
    let output = Command::new(&script)
        .args([session, revision, hunk])
        .env("LGR_BIN", cargo_bin("lgr"))
        .env("LGR_DATA_DIR", data.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["result"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(responses[0]["result"]["evidence_complete"], true);
    assert_eq!(responses[1]["result"]["submitted"], 1);
    assert!(responses[1]["result"].get("queue").is_none());
    assert!(serde_json::to_vec(&responses[1]).unwrap().len() < 2_048);
    assert_eq!(responses[2]["result"]["finalized"], true);
    assert!(responses[2]["result"].get("ranking").is_none());
    assert!(serde_json::to_vec(&responses[2]).unwrap().len() < 2_048);
    let queue = lgr(data.path(), &["graph", "queue", session]);
    assert_eq!(queue["result"]["fully_ranked"], true);
    let before_score = queue["result"]["items"][0]["score"].clone();
    let output = Command::new(&script)
        .args([session, revision, hunk, "titles-only"])
        .env("LGR_BIN", cargo_bin("lgr"))
        .env("LGR_DATA_DIR", data.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let enriched = lgr(data.path(), &["graph", "queue", session]);
    assert_eq!(
        enriched["result"]["items"][0]["title"],
        "Explain the selected semantic change"
    );
    assert_eq!(enriched["result"]["items"][0]["score"], before_score);
    assert_eq!(enriched["result"]["finalized"], true);
}

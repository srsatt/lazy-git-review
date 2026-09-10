use std::fs;

use assert_cmd::Command;
use lazy_git_review::git;
use serde_json::Value;
use tempfile::TempDir;

fn commit(repository: &std::path::Path, message: &str) -> String {
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
            git::os(message),
        ],
    )
    .unwrap();
    git::text(repository, &[git::os("rev-parse"), git::os("HEAD")]).unwrap()
}

fn create_review(home: &TempDir, repository: &TempDir, revisions: &str) -> Value {
    let output = Command::cargo_bin("lgr")
        .unwrap()
        .env("HOME", home.path())
        .args([
            "review",
            "create",
            revisions,
            "--repository",
            repository.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn diffview_ranges_choose_direct_or_merge_base_comparison() {
    let home = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    git::run(repository.path(), &[git::os("init"), git::os("-q")]).unwrap();
    fs::write(
        repository.path().join("base.ts"),
        "export const base = 1;\n",
    )
    .unwrap();
    let base = commit(repository.path(), "base");

    git::run(
        repository.path(),
        &[git::os("checkout"), git::os("-qb"), git::os("feature")],
    )
    .unwrap();
    fs::write(
        repository.path().join("feature.ts"),
        "export const feature = true;\n",
    )
    .unwrap();
    let feature = commit(repository.path(), "feature");

    git::run(
        repository.path(),
        &[
            git::os("checkout"),
            git::os("-qb"),
            git::os("develop"),
            git::os(&base),
        ],
    )
    .unwrap();
    fs::write(
        repository.path().join("develop.ts"),
        "export const develop = true;\n",
    )
    .unwrap();
    let develop = commit(repository.path(), "develop");

    let direct = create_review(&home, &repository, "feature..develop");
    assert_eq!(direct["result"]["snapshot"]["original_base"], feature);
    assert_eq!(direct["result"]["snapshot"]["original_head"], develop);
    assert_eq!(
        direct["result"]["snapshot"]["comparison_base"],
        direct["result"]["snapshot"]["original_base"]
    );

    let merge_base = create_review(&home, &repository, "feature...develop");
    assert_eq!(merge_base["result"]["snapshot"]["original_base"], feature);
    assert_eq!(merge_base["result"]["snapshot"]["original_head"], develop);
    assert_eq!(merge_base["result"]["snapshot"]["comparison_base"], base);
}

#[test]
fn malformed_revision_range_is_a_structured_error() {
    let home = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    let output = Command::cargo_bin("lgr")
        .unwrap()
        .env("HOME", home.path())
        .args([
            "review",
            "create",
            "feature...",
            "--repository",
            repository.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["errors"][0]["code"], "invalid_revision_spec");
}

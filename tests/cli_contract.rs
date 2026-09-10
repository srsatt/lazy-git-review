use assert_cmd::Command;
use lazy_git_review::git;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

#[test]
fn help_names_binary_and_commands() {
    Command::cargo_bin("lgr")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Rank Git changes").and(predicate::str::contains("doctor")),
        );
}

#[test]
fn tui_session_is_optional_and_session_list_is_discoverable() {
    Command::cargo_bin("lgr")
        .unwrap()
        .args(["session", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"));
    Command::cargo_bin("lgr")
        .unwrap()
        .args(["tui", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[SESSION]"));
}

#[test]
fn enrichment_help_is_actionable_and_explain_modes_are_unambiguous() {
    Command::cargo_bin("lgr")
        .unwrap()
        .args(["tests", "run", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("disposable captured snapshot side")
                .and(predicate::str::contains("without executing")),
        );
    Command::cargo_bin("lgr")
        .unwrap()
        .args([
            "explain",
            "ses_example",
            "--show",
            "--item",
            "h_one",
            "--note",
            "ambiguous",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn success_stdout_is_one_json_envelope() {
    let output = Command::cargo_bin("lgr")
        .unwrap()
        .arg("doctor")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["ok"], true);
    assert!(value["result"]["dependencies"].is_array());
}

#[test]
fn failure_stdout_is_structured_and_uses_stable_exit_code() {
    let dir = TempDir::new().unwrap();
    let output = Command::cargo_bin("lgr")
        .unwrap()
        .args([
            "--data-dir",
            dir.path().to_str().unwrap(),
            "session",
            "show",
            "bad",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["errors"][0]["code"], "invalid_session_id");
}

#[test]
fn debug_keeps_json_clean_and_writes_versioned_diagnostic_to_stderr() {
    let dir = TempDir::new().unwrap();
    let output = Command::cargo_bin("lgr")
        .unwrap()
        .args([
            "--debug",
            "--data-dir",
            dir.path().to_str().unwrap(),
            "session",
            "show",
            "bad",
        ])
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["errors"][0]["code"], "invalid_session_id");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains(concat!("lgr v", env!("CARGO_PKG_VERSION"))));
    assert!(diagnostic.contains("invalid_session_id"));
}

#[test]
fn tui_rejects_redirected_stdio_before_raw_mode() {
    let home = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    git::run(repository.path(), &[git::os("init"), git::os("-q")]).unwrap();
    std::fs::write(repository.path().join("a.ts"), "const a = 1;\n").unwrap();
    git::run(repository.path(), &[git::os("add"), git::os(".")]).unwrap();
    git::run(
        repository.path(),
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
    std::fs::write(repository.path().join("a.ts"), "const a = 2;\n").unwrap();
    let created = Command::cargo_bin("lgr")
        .unwrap()
        .env("HOME", home.path())
        .args(["--data-dir", data.path().to_str().unwrap()])
        .args([
            "review",
            "create",
            "--repository",
            repository.path().to_str().unwrap(),
            "--unstaged",
        ])
        .output()
        .unwrap();
    let created: Value = serde_json::from_slice(&created.stdout).unwrap();
    let session = created["result"]["session"]["id"].as_str().unwrap();
    Command::cargo_bin("lgr")
        .unwrap()
        .env("HOME", home.path())
        .args(["--data-dir", data.path().to_str().unwrap()])
        .args(["graph", "build", session])
        .assert()
        .success();
    let output = Command::cargo_bin("lgr")
        .unwrap()
        .env("HOME", home.path())
        .args(["--data-dir", data.path().to_str().unwrap()])
        .args(["tui", session])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains(&0x1b));
    let failure: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(failure["errors"][0]["code"], "tui_requires_terminal");
}

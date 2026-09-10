use std::fs;
use std::os::unix::fs::PermissionsExt;

use assert_cmd::Command;
use lazy_git_review::git;
use serde_json::Value;
use tempfile::TempDir;

fn run(home: &TempDir, args: &[&str]) -> Value {
    let output = Command::cargo_bin("lgr")
        .unwrap()
        .env("HOME", home.path())
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

fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn initializes_json_layout_and_selects_generic_agent_profiles_without_running_them() {
    let home = TempDir::new().unwrap();
    let initialized = run(&home, &["config", "init"]);
    let root = home.path().join(".lgr");
    assert_eq!(
        initialized["result"]["path"],
        root.join("settings.json").to_string_lossy().as_ref()
    );
    assert!(root.join("data").is_dir());
    assert!(root.join("scripts").is_dir());
    assert_eq!(
        fs::metadata(root.join("settings.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let codex = run(&home, &["agent-profile", "resolve"]);
    assert_eq!(codex["result"]["name"], "codex");
    assert_eq!(
        codex["result"]["profile"]["command"],
        serde_json::json!([
            "codex",
            "exec",
            "--json",
            "--sandbox",
            "read-only",
            "--ephemeral"
        ])
    );
    let opencode = run(&home, &["agent-profile", "select", "opencode"]);
    assert_eq!(
        opencode["result"]["profile"]["command"],
        serde_json::json!(["opencode", "run"])
    );
    assert_eq!(
        run(&home, &["agent-profile", "resolve"])["result"]["name"],
        "opencode"
    );
}

#[test]
fn rank_supports_ad_hoc_agent_and_named_profile_with_injected_skill() {
    let home = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    git::run(repository.path(), &[git::os("init"), git::os("-q")]).unwrap();
    fs::write(repository.path().join("change.txt"), "base\n").unwrap();
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
    fs::write(repository.path().join("change.txt"), "changed\n").unwrap();
    run(&home, &["config", "init"]);
    let created = run(
        &home,
        &[
            "review",
            "create",
            "--repository",
            repository.path().to_str().unwrap(),
            "--unstaged",
        ],
    );
    let session = created["result"]["session"]["id"].as_str().unwrap();
    run(&home, &["graph", "build", session]);

    let dry_run = run(&home, &["-p", "codex", "rank", session, "--dry-run"]);
    let command = dry_run["result"]["invocation"]["command"]
        .as_array()
        .unwrap();
    assert_eq!(
        &command[..6],
        &serde_json::json!([
            "codex",
            "exec",
            "--json",
            "--sandbox",
            "read-only",
            "--ephemeral"
        ])
        .as_array()
        .unwrap()[..]
    );
    assert!(command.last().unwrap().as_str().unwrap().contains(
        "Follow this embedded semantic-review skill without requiring it to be installed in your harness"
    ));

    let ranker = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mock_ranker.py")
        .to_string_lossy()
        .into_owned();
    let launched = run(&home, &["-a", &ranker, "rank", session]);
    assert_eq!(launched["result"]["launched"], true);
    assert_eq!(launched["result"]["ranking"]["fully_ranked"], true);
    assert_eq!(launched["result"]["ranking"]["finalized"], true);
    assert_eq!(
        fs::read_to_string(home.path().join(".lgr/data/agent-session")).unwrap(),
        session
    );
    assert!(
        fs::read_to_string(home.path().join(".lgr/data/agent-prompt"))
            .unwrap()
            .contains("Semantic Review Ranker")
    );
    assert_eq!(
        fs::read_to_string(home.path().join(".lgr/data/agent-budget")).unwrap(),
        "false"
    );
}

#[test]
fn session_commands_use_configured_data_directory_without_flag() {
    let home = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    run(&home, &["config", "init"]);
    let created = run(
        &home,
        &[
            "session",
            "create",
            "--repository",
            repository.path().to_str().unwrap(),
        ],
    );
    let id = created["result"]["id"].as_str().unwrap();
    let shown = run(&home, &["session", "show", id]);
    assert_eq!(shown["result"]["id"], id);
    assert!(home.path().join(".lgr/data/sessions.sqlite3").is_file());
}

#[test]
fn copied_data_directory_rebases_snapshot_paths() {
    let repository = TempDir::new().unwrap();
    git::run(repository.path(), &[git::os("init"), git::os("-q")]).unwrap();
    fs::write(
        repository.path().join("value.ts"),
        "export const value = 1;\n",
    )
    .unwrap();
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
    let base = git::text(repository.path(), &[git::os("rev-parse"), git::os("HEAD")]).unwrap();
    fs::write(
        repository.path().join("value.ts"),
        "export const value = 2;\n",
    )
    .unwrap();
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
            git::os("head"),
        ],
    )
    .unwrap();
    let head = git::text(repository.path(), &[git::os("rev-parse"), git::os("HEAD")]).unwrap();
    let original = TempDir::new().unwrap();
    let copied = TempDir::new().unwrap();
    let created = Command::cargo_bin("lgr")
        .unwrap()
        .args(["--data-dir", original.path().to_str().unwrap()])
        .args([
            "review",
            "create",
            "--repository",
            repository.path().to_str().unwrap(),
            "--base",
            &base,
            "--head",
            &head,
            "--direct",
        ])
        .output()
        .unwrap();
    assert!(created.status.success());
    let created: Value = serde_json::from_slice(&created.stdout).unwrap();
    let session = created["result"]["session"]["id"].as_str().unwrap();
    copy_tree(original.path(), copied.path());
    original.close().unwrap();

    let listed = Command::cargo_bin("lgr")
        .unwrap()
        .args(["--data-dir", copied.path().to_str().unwrap()])
        .args(["comment", "list", session])
        .output()
        .unwrap();
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stdout)
    );
    let listed: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(listed["ok"], true);
}

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use assert_cmd::Command;
use lazy_git_review::git;
use lazy_git_review::profiles::{GitHubProfile, ProfileStore};
use serde_json::Value;
use tempfile::TempDir;

fn output(command: &mut Command) -> Value {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn configures_and_resolves_repository_profiles() {
    let directory = TempDir::new().unwrap();
    let config = directory.path().join("profiles.json");

    let set = output(
        Command::cargo_bin("lgr")
            .unwrap()
            .args([
                "--profile-config",
                config.to_str().unwrap(),
                "profile",
                "set",
                "work",
            ])
            .args(["--match", "github.example.test/company/*"])
            .args(["--github-command", "gh-work"])
            .args(["--github-arg", "--hostname"])
            .args(["--expected-account", "employee"])
            .args(["--api-host", "api.github.example.test"]),
    );
    assert_eq!(set["result"]["profile"]["name"], "work");
    assert_eq!(
        set["result"]["profile"]["api_host"],
        "api.github.example.test"
    );

    let resolved = output(
        Command::cargo_bin("lgr")
            .unwrap()
            .args([
                "--profile-config",
                config.to_str().unwrap(),
                "profile",
                "resolve",
            ])
            .args(["--host", "github.example.test"])
            .args(["--repository", "company/service"]),
    );
    assert_eq!(resolved["result"]["profile"]["name"], "work");
    assert_eq!(
        resolved["result"]["profile"]["github_command"],
        serde_json::json!(["gh-work", "--hostname"])
    );
    assert_eq!(
        fs::metadata(config).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn github_fetch_uses_automatically_matched_profile_command() {
    let directory = TempDir::new().unwrap();
    let config = directory.path().join("profiles.json");
    let state = directory.path().join("mutations");
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_github_cli.py");
    ProfileStore {
        version: 1,
        profiles: vec![GitHubProfile {
            name: "primary-test".into(),
            repository_patterns: vec!["github.com/example/repository".into()],
            github_command: vec![
                "python3".into(),
                script.to_string_lossy().into_owned(),
                state.to_string_lossy().into_owned(),
                "normal".into(),
                "base-sha".into(),
                "head-sha".into(),
            ],
            expected_account: "reviewer".into(),
            api_host: None,
        }],
    }
    .save(&config)
    .unwrap();

    let fetched = output(
        Command::cargo_bin("lgr")
            .unwrap()
            .args(["--profile-config", config.to_str().unwrap()])
            .args(["github", "--repository", "example/repository", "fetch", "7"]),
    );
    assert_eq!(fetched["result"]["title"], "Test PR");
    assert!(
        !state.exists(),
        "read-only fetch must not mutate GitHub state"
    );
}

#[test]
fn automatic_resolution_fails_closed_for_unknown_repository() {
    let directory = TempDir::new().unwrap();
    let config = directory.path().join("profiles.json");
    ProfileStore::default().save(&config).unwrap();
    let output = Command::cargo_bin("lgr")
        .unwrap()
        .args(["--profile-config", config.to_str().unwrap()])
        .args(["github", "--repository", "company/service", "fetch", "7"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["errors"][0]["code"], "profile_not_matched");
}

#[test]
fn executes_github_cli_through_directory_matched_profile() {
    let directory = TempDir::new().unwrap();
    let repository = directory.path().join("checkout");
    fs::create_dir(&repository).unwrap();
    git::run(&repository, &[git::os("init"), git::os("-q")]).unwrap();
    git::run(
        &repository,
        &[
            git::os("remote"),
            git::os("add"),
            git::os("origin"),
            git::os("git@github.example.test:company/service.git"),
        ],
    )
    .unwrap();

    let command = directory.path().join("gh-work");
    fs::write(
        &command,
        "#!/bin/sh\nif [ \"$1\" = api ]; then [ \"$3\" = api.github.example.test ] || exit 9; printf 'employee\\n'; exit 0; fi\nprintf '%s|%s\\n' \"$PWD\" \"$*\"\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&command).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&command, permissions).unwrap();

    let config = directory.path().join("profiles.json");
    ProfileStore {
        version: 1,
        profiles: vec![GitHubProfile {
            name: "work".into(),
            repository_patterns: vec!["github.example.test/company/*".into()],
            github_command: vec![command.to_string_lossy().into_owned()],
            expected_account: "employee".into(),
            api_host: Some("api.github.example.test".into()),
        }],
    }
    .save(&config)
    .unwrap();

    let dry_run = output(
        Command::cargo_bin("lgr")
            .unwrap()
            .args(["--profile-config", config.to_str().unwrap()])
            .args([
                "github-profile",
                "exec",
                "--repository-dir",
                repository.to_str().unwrap(),
                "--dry-run",
                "--",
                "dash",
            ]),
    );
    assert_eq!(dry_run["result"]["profile"], "work");
    assert_eq!(dry_run["result"]["api_host"], "api.github.example.test");
    assert_eq!(
        dry_run["result"]["command"],
        serde_json::json!([command.to_string_lossy(), "dash"])
    );

    let launched = Command::cargo_bin("lgr")
        .unwrap()
        .args(["--profile-config", config.to_str().unwrap()])
        .args([
            "github-profile",
            "exec",
            "--repository-dir",
            repository.to_str().unwrap(),
            "--",
            "dash",
            "--debug",
        ])
        .output()
        .unwrap();
    assert!(launched.status.success());
    assert_eq!(
        String::from_utf8_lossy(&launched.stdout).trim(),
        format!(
            "{}|dash --debug",
            fs::canonicalize(&repository).unwrap().display()
        )
    );
}

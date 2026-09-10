use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use tempfile::TempDir;

fn executable_fixture(path: &Path) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions).unwrap();
}

fn scripts() -> (PathBuf, PathBuf, PathBuf) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    (
        root.join("scripts/lgr-gh-dash"),
        root.join("scripts/lgr-fetch-youtrack"),
        root.join("tests/fixtures/mock_lgr_dash_cli.py"),
    )
}

fn youtrack_server() -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut bytes = [0_u8; 8192];
        let size = stream.read(&mut bytes).unwrap();
        let request = String::from_utf8_lossy(&bytes[..size]).into_owned();
        let body = r#"{"idReadable":"JT-95384","summary":"Hub settings","description":"Keep controls usable."}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        request
    });
    (format!("http://{address}"), handle)
}

fn invocation_log(path: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn selected_pr_adds_ticket_context_opens_tui_and_submits_only_after_yes() {
    let (launcher, fetcher, mock_lgr) = scripts();
    executable_fixture(&launcher);
    executable_fixture(&fetcher);
    executable_fixture(&mock_lgr);
    let directory = TempDir::new().unwrap();
    let repository = directory.path().join("checkout");
    fs::create_dir(&repository).unwrap();
    let log = directory.path().join("lgr.log");
    let (youtrack_url, server) = youtrack_server();

    let mut child = Command::new(&launcher)
        .args(["--repository", "example/repository", "--number", "7"])
        .args(["--local-repository", repository.to_str().unwrap()])
        .args(["--youtrack-url", &youtrack_url])
        .env("LGR_BIN", &mock_lgr)
        .env("LGR_FETCH_YOUTRACK_BIN", &fetcher)
        .env("MOCK_LGR_LOG", &log)
        .env("MOCK_LGR_DRAFTS", "1")
        .env("YOUTRACK_API_KEY", "secret-api-key")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"yes\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let request = server.join().unwrap();
    assert!(request.starts_with("GET /api/issues/JT-95384?fields="));
    assert!(request.contains("Authorization: Bearer secret-api-key"));
    let calls = invocation_log(&log);
    assert!(
        calls
            .iter()
            .any(|call| call["args"] == serde_json::json!(["tui", "ses_test"]))
    );
    assert!(
        calls
            .iter()
            .all(|call| call["has_youtrack_secret"] == false)
    );
    let context = calls
        .iter()
        .find_map(|call| call.get("context").and_then(|value| value.as_str()))
        .unwrap();
    assert!(context.contains("# JT-95384 — Hub settings"));
    assert!(context.contains("Keep controls usable."));
    assert_eq!(
        calls
            .iter()
            .filter(|call| call["args"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("submit")))
            .count(),
        1
    );
    let status = String::from_utf8_lossy(&output.stderr);
    for state in [
        "Capture PR",
        "YouTrack context",
        "Build graph",
        "Rank changes",
        "Open review",
        "Review ready",
    ] {
        assert!(status.contains(state), "missing state {state}: {status}");
    }
    assert!(!status.contains("RANKER_INTERNAL_WALL"));
    assert!(!status.contains("LGR 1/4"));
    assert!(status.contains("Submitted GitHub review 9001"));
}

#[test]
fn neovim_dashboard_hands_prepared_session_to_bridge() {
    let (launcher, _fetcher, mock_lgr) = scripts();
    executable_fixture(&launcher);
    executable_fixture(&mock_lgr);
    let directory = TempDir::new().unwrap();
    let repository = directory.path().join("checkout");
    fs::create_dir(&repository).unwrap();
    let log = directory.path().join("lgr.log");
    let output = Command::new(&launcher)
        .args(["--repository", "example/repository", "--number", "7"])
        .args(["--local-repository", repository.to_str().unwrap()])
        .args(["--skip-youtrack", "--no-publish-prompt"])
        .env("LGR_BIN", &mock_lgr)
        .env("LGR_NVIM_BIN", &mock_lgr)
        .env("MOCK_LGR_LOG", &log)
        .env("NVIM", "/tmp/nvim-test-server")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = invocation_log(&log);
    let remote = calls
        .iter()
        .find(|call| {
            call["args"].as_array().unwrap().first() == Some(&serde_json::json!("--server"))
        })
        .unwrap();
    assert!(
        remote["args"][3]
            .as_str()
            .unwrap()
            .contains("open_github_session")
    );
    assert!(
        remote["args"][3]
            .as_str()
            .unwrap()
            .contains("\"publish\": 0")
    );
    assert!(!calls.iter().any(|call| {
        call["args"].as_array().unwrap().first() == Some(&serde_json::json!("tui"))
    }));
}

#[test]
fn default_confirmation_keeps_drafts_local() {
    let (launcher, _fetcher, mock_lgr) = scripts();
    executable_fixture(&launcher);
    executable_fixture(&mock_lgr);
    let directory = TempDir::new().unwrap();
    let repository = directory.path().join("checkout");
    fs::create_dir(&repository).unwrap();
    let log = directory.path().join("lgr.log");
    let mut child = Command::new(&launcher)
        .args(["--repository", "example/repository", "--number", "7"])
        .args(["--local-repository", repository.to_str().unwrap()])
        .arg("--skip-youtrack")
        .env("LGR_BIN", &mock_lgr)
        .env("MOCK_LGR_LOG", &log)
        .env("MOCK_LGR_DRAFTS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let calls = invocation_log(&log);
    assert!(!calls.iter().any(|call| {
        call["args"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("submit"))
    }));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Drafts retained locally"));
}

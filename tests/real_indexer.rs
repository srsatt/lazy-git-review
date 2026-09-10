use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

use lazy_git_review::git;
use lazy_git_review::graph::{EdgeKind, NodeKind};
use lazy_git_review::indexer::{IndexOptions, build};
use lazy_git_review::lsp::ServerProfile;
use lazy_git_review::snapshot::{SnapshotInput, capture};
use tempfile::TempDir;

fn typescript_profile(bin: &std::path::Path) -> ServerProfile {
    ServerProfile {
        name: "typescript".into(),
        command: vec![
            bin.join("typescript-language-server").into_os_string(),
            OsString::from("--stdio"),
        ],
        languages: BTreeMap::from([
            ("ts".into(), "typescript".into()),
            ("tsx".into(), "typescriptreact".into()),
        ]),
        initialization_options: Some(serde_json::json!({
            "tsserver": { "path": bin.parent().unwrap().join("typescript/lib/tsserver.js") }
        })),
        workspace_configuration: serde_json::Value::Null,
    }
}

#[tokio::test]
async fn indexes_separate_projects_aliases_jsx_decorators_and_excluded_tests() {
    let Some(bin) = std::env::var_os("LGR_LSP_BIN_DIR").map(PathBuf::from) else {
        return;
    };
    let repo = TempDir::new().unwrap();
    git::run(repo.path(), &[git::os("init"), git::os("-q")]).unwrap();
    for project in ["frontend", "backend"] {
        fs::create_dir_all(repo.path().join(project).join("src")).unwrap();
        fs::create_dir_all(repo.path().join(project).join("tests")).unwrap();
        fs::write(
            repo.path().join(project).join("package.json"),
            r#"{"private":true,"type":"module"}"#,
        )
        .unwrap();
    }
    fs::write(
        repo.path().join("frontend/tsconfig.json"),
        r#"{"compilerOptions":{"jsx":"preserve","baseUrl":".","paths":{"@/*":["src/*"]}},"include":["src"]}"#,
    )
    .unwrap();
    fs::write(
        repo.path().join("backend/tsconfig.json"),
        r#"{"compilerOptions":{"experimentalDecorators":true},"include":["src"]}"#,
    )
    .unwrap();
    fs::write(
        repo.path().join("frontend/src/card.tsx"),
        "export function risk() { return 1; }\nexport const Card = () => <main>{risk()}</main>;\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("frontend/tests/card.test.ts"),
        "import { risk } from '@/card';\nvoid risk();\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("backend/src/service.ts"),
        "function sealed(_: Function) {}\n@sealed\nexport class Service { value() { return 1; } }\n",
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
        repo.path().join("frontend/src/card.tsx"),
        "export function risk() { return 2; }\nexport const Card = () => <main>{risk()}</main>;\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("backend/src/service.ts"),
        "function sealed(_: Function) {}\n@sealed\nexport class Service { value() { return 2; } }\n",
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
        SnapshotInput::Revisions { base, head },
        data.path(),
    )
    .unwrap();
    let graph = build(
        &snapshot,
        &IndexOptions {
            profiles: vec![typescript_profile(&bin)],
            request_timeout_seconds: 10,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        graph
            .nodes
            .values()
            .any(|node| { node.kind == NodeKind::Symbol && node.name == "risk" && node.changed })
    );
    assert!(
        graph.nodes.values().any(|node| {
            node.kind == NodeKind::Symbol && node.name == "Service" && node.changed
        })
    );
    assert!(
        graph
            .coverage
            .iter()
            .any(|entry| entry.project.ends_with("frontend"))
    );
    assert!(
        graph
            .coverage
            .iter()
            .any(|entry| entry.project.ends_with("backend"))
    );
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::TestReference)
    );
    assert!(graph.edges.iter().any(|edge| {
        edge.kind == EdgeKind::Definition && edge.producer == "lsp" && edge.confidence == 1.0
    }));
    assert!(
        graph
            .coverage
            .iter()
            .all(|entry| entry.server_version.is_some())
    );
    assert!(graph.coverage.iter().all(|entry| {
        !entry
            .supported_relations
            .contains(&"call_hierarchy".to_owned())
    }));
    assert!(
        graph
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::Counterpart)
    );

    let cached = build(
        &snapshot,
        &IndexOptions {
            profiles: vec![typescript_profile(&bin)],
            request_timeout_seconds: 10,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(cached.revision, graph.revision);
    fs::write(
        snapshot.storage_dir.join("after/frontend/tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"jsx":"preserve"},"include":["src"]}"#,
    )
    .unwrap();
    let invalidated = build(
        &snapshot,
        &IndexOptions {
            profiles: vec![typescript_profile(&bin)],
            request_timeout_seconds: 10,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_ne!(invalidated.revision, graph.revision);

    let partial = build(
        &snapshot,
        &IndexOptions {
            profiles: vec![typescript_profile(&bin)],
            max_files: 0,
            force: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!partial.unfinished_frontier.is_empty());
    assert!(
        partial
            .nodes
            .values()
            .any(|node| node.kind == NodeKind::Hunk)
    );
}

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use lazy_git_review::lsp::{LspClient, ServerProfile};
use tempfile::TempDir;

fn profile(mode: &str) -> ServerProfile {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mock_lsp.py");
    ServerProfile {
        name: format!("mock-{mode}"),
        command: vec![
            OsString::from("python3"),
            fixture.into_os_string(),
            OsString::from(mode),
        ],
        languages: BTreeMap::from([("ts".into(), "typescript".into())]),
        initialization_options: None,
        workspace_configuration: serde_json::Value::Null,
    }
}

#[tokio::test]
async fn negotiates_capabilities_and_reads_symbols() {
    let root = TempDir::new().unwrap();
    let path = root.path().join("a.ts");
    fs::write(&path, "export function changed() {\n return 2;\n}\n").unwrap();
    let mut client = LspClient::start(&profile("normal"), root.path(), Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(client.info.version.as_deref(), Some("1.0.0"));
    assert_eq!(client.info.position_encoding, "utf-16");
    assert!(client.info.supports_references);
    let symbols = client
        .document_symbols(&path, "typescript", fs::read_to_string(&path).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lazy_git_review::graph::normalize_symbols(symbols).len(), 1);
    let position = async_lsp::lsp_types::Position::new(0, 16);
    assert_eq!(client.references(&path, position).await.unwrap().len(), 1);
    assert_eq!(client.definition(&path, position).await.unwrap().len(), 1);
    let items = client
        .prepare_call_hierarchy(&path, position)
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert!(
        client
            .incoming_calls(items[0].clone())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        client
            .outgoing_calls(items[0].clone())
            .await
            .unwrap()
            .is_empty()
    );
    client.finish().await.unwrap();
}

#[tokio::test]
async fn exposes_unsupported_capabilities_and_bounds_timeout_and_crash() {
    let root = TempDir::new().unwrap();
    let client = LspClient::start(&profile("unsupported"), root.path(), Duration::from_secs(2))
        .await
        .unwrap();
    assert!(!client.info.supports_references);
    assert!(!client.info.supports_definition);
    assert!(!client.info.supports_call_hierarchy);
    client.finish().await.unwrap();

    assert!(
        LspClient::start(&profile("timeout"), root.path(), Duration::from_millis(50))
            .await
            .is_err()
    );
    assert!(
        LspClient::start(&profile("crash"), root.path(), Duration::from_secs(1))
            .await
            .is_err()
    );
}

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use lazy_git_review::graph::normalize_symbols;
use lazy_git_review::lsp::{LspClient, ServerProfile};
use tempfile::TempDir;

fn configured_profile(
    bin: &std::path::Path,
    name: &str,
    executable: &str,
    extension: &str,
    language: &str,
) -> ServerProfile {
    ServerProfile {
        name: name.into(),
        command: vec![
            bin.join(executable).into_os_string(),
            OsString::from("--stdio"),
        ],
        languages: BTreeMap::from([(extension.into(), language.into())]),
        initialization_options: None,
        workspace_configuration: serde_json::Value::Null,
    }
}

#[tokio::test]
async fn qualified_servers_expose_real_document_structure() {
    let Some(bin) = std::env::var_os("LGR_LSP_BIN_DIR").map(PathBuf::from) else {
        return;
    };
    let root = TempDir::new().unwrap();
    std::os::unix::fs::symlink(bin.parent().unwrap(), root.path().join("node_modules")).unwrap();
    fs::write(
        root.path().join("package.json"),
        r#"{"private":true,"devDependencies":{"typescript":"*"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"jsx":"preserve","experimentalDecorators":true}}"#,
    )
    .unwrap();
    let cases = [
        (
            configured_profile(
                &bin,
                "typescript",
                "typescript-language-server",
                "tsx",
                "typescriptreact",
            ),
            "component.tsx",
            "export const Component = () => <main>ok</main>;\n",
        ),
        (
            configured_profile(&bin, "html", "vscode-html-language-server", "html", "html"),
            "index.html",
            "<main><section>ok</section></main>\n",
        ),
        (
            configured_profile(&bin, "css", "vscode-css-language-server", "css", "css"),
            "style.css",
            ".root { color: red; }\n",
        ),
    ];
    for (mut profile, name, source) in cases {
        if profile.name == "typescript" {
            profile.initialization_options = Some(serde_json::json!({
                "tsserver": {
                    "path": bin.parent().unwrap().join("typescript/lib/tsserver.js")
                }
            }));
        }
        let path = root.path().join(name);
        fs::write(&path, source).unwrap();
        let language = profile
            .languages
            .get(path.extension().unwrap().to_str().unwrap())
            .unwrap()
            .clone();
        let mut client = LspClient::start(&profile, root.path(), Duration::from_secs(10))
            .await
            .unwrap();
        let response = client
            .document_symbols(&path, &language, source.into())
            .await
            .unwrap();
        assert!(
            response.is_some(),
            "{} returned no symbol response",
            profile.name
        );
        assert!(
            !normalize_symbols(response.unwrap()).is_empty(),
            "{} returned no structure",
            profile.name
        );
        client.finish().await.unwrap();
    }
}

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::future::ready;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_lsp::lsp_types::notification::{LogMessage, Progress, PublishDiagnostics, ShowMessage};
use async_lsp::lsp_types::{
    ApplyWorkspaceEditParams, ApplyWorkspaceEditResponse, CallHierarchyIncomingCall,
    CallHierarchyIncomingCallsParams, CallHierarchyItem, CallHierarchyOutgoingCall,
    CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams, ClientCapabilities,
    ConfigurationParams, DidOpenTextDocumentParams, DocumentSymbolParams, DocumentSymbolResponse,
    GeneralClientCapabilities, GotoDefinitionParams, GotoDefinitionResponse, InitializeParams,
    InitializeResult, InitializedParams, Location, Position, PositionEncodingKind,
    ReferenceContext, ReferenceParams, RegistrationParams, TextDocumentClientCapabilities,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
    WindowClientCapabilities, WorkspaceClientCapabilities, WorkspaceFolder,
};
use async_lsp::router::Router;
use async_lsp::{LanguageClient, LanguageServer, ResponseError};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::error::{AppError, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerProfile {
    pub name: String,
    pub command: Vec<OsString>,
    pub languages: BTreeMap<String, String>,
    #[serde(default)]
    pub initialization_options: Option<Value>,
    #[serde(default)]
    pub workspace_configuration: Value,
}

impl ServerProfile {
    pub fn typescript() -> Self {
        let typescript_path = std::env::var_os("LGR_TSSERVER_PATH")
            .map(PathBuf::from)
            .or_else(|| discover_typescript_path(std::env::var_os("PATH").as_deref()));
        let initialization_options = typescript_path.map(|path| {
            serde_json::json!({
                "tsserver": { "path": path }
            })
        });
        Self {
            name: "typescript".into(),
            command: vec!["typescript-language-server".into(), "--stdio".into()],
            languages: BTreeMap::from([
                ("ts".into(), "typescript".into()),
                ("tsx".into(), "typescriptreact".into()),
                ("js".into(), "javascript".into()),
                ("jsx".into(), "javascriptreact".into()),
            ]),
            initialization_options,
            workspace_configuration: Value::Null,
        }
    }

    pub fn html() -> Self {
        Self {
            name: "html".into(),
            command: vec!["vscode-html-language-server".into(), "--stdio".into()],
            languages: BTreeMap::from([
                ("html".into(), "html".into()),
                ("htm".into(), "html".into()),
            ]),
            initialization_options: None,
            workspace_configuration: Value::Null,
        }
    }

    pub fn css() -> Self {
        Self {
            name: "css".into(),
            command: vec!["vscode-css-language-server".into(), "--stdio".into()],
            languages: BTreeMap::from([
                ("css".into(), "css".into()),
                ("scss".into(), "scss".into()),
                ("less".into(), "less".into()),
            ]),
            initialization_options: None,
            workspace_configuration: Value::Null,
        }
    }

    pub fn defaults() -> Vec<Self> {
        vec![Self::typescript(), Self::html(), Self::css()]
    }

    pub fn language_for(&self, path: &Path) -> Option<&str> {
        self.languages
            .get(path.extension()?.to_str()?)
            .map(String::as_str)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerInfo {
    pub profile: String,
    pub version: Option<String>,
    pub position_encoding: String,
    pub supports_document_symbols: bool,
    pub supports_references: bool,
    pub supports_definition: bool,
    pub supports_call_hierarchy: bool,
}

struct ClientState {
    workspace: WorkspaceFolder,
    configuration: Value,
}

impl LanguageClient for ClientState {
    type Error = ResponseError;
    type NotifyResult = ControlFlow<async_lsp::Result<()>>;

    fn publish_diagnostics(
        &mut self,
        _: async_lsp::lsp_types::PublishDiagnosticsParams,
    ) -> Self::NotifyResult {
        ControlFlow::Continue(())
    }
    fn progress(&mut self, _: async_lsp::lsp_types::ProgressParams) -> Self::NotifyResult {
        ControlFlow::Continue(())
    }
    fn log_message(&mut self, _: async_lsp::lsp_types::LogMessageParams) -> Self::NotifyResult {
        ControlFlow::Continue(())
    }
    fn show_message(&mut self, _: async_lsp::lsp_types::ShowMessageParams) -> Self::NotifyResult {
        ControlFlow::Continue(())
    }

    fn configuration(
        &mut self,
        params: ConfigurationParams,
    ) -> BoxFuture<'static, std::result::Result<Vec<Value>, Self::Error>> {
        Box::pin(ready(Ok(vec![
            self.configuration.clone();
            params.items.len()
        ])))
    }

    fn workspace_folders(
        &mut self,
        _: (),
    ) -> BoxFuture<'static, std::result::Result<Option<Vec<WorkspaceFolder>>, Self::Error>> {
        let folders = vec![self.workspace.clone()];
        Box::pin(ready(Ok(Some(folders))))
    }

    fn work_done_progress_create(
        &mut self,
        _: async_lsp::lsp_types::WorkDoneProgressCreateParams,
    ) -> BoxFuture<'static, std::result::Result<(), Self::Error>> {
        Box::pin(ready(Ok(())))
    }

    fn register_capability(
        &mut self,
        _: RegistrationParams,
    ) -> BoxFuture<'static, std::result::Result<(), Self::Error>> {
        Box::pin(ready(Ok(())))
    }

    fn apply_edit(
        &mut self,
        _: ApplyWorkspaceEditParams,
    ) -> BoxFuture<'static, std::result::Result<ApplyWorkspaceEditResponse, Self::Error>> {
        Box::pin(ready(Ok(ApplyWorkspaceEditResponse {
            applied: false,
            failure_reason: Some(
                "lazy-git-review indexes immutable snapshots and rejects workspace edits".into(),
            ),
            failed_change: None,
        })))
    }
}

impl ClientState {
    fn router(workspace: WorkspaceFolder, configuration: Value) -> Router<Self> {
        let mut router = Router::from_language_client(Self {
            workspace,
            configuration,
        });
        router
            .notification::<PublishDiagnostics>(|_, _| ControlFlow::Continue(()))
            .notification::<Progress>(|_, _| ControlFlow::Continue(()))
            .notification::<LogMessage>(|_, _| ControlFlow::Continue(()))
            .notification::<ShowMessage>(|_, _| ControlFlow::Continue(()))
            .event(|_, _: Stop| ControlFlow::Break(Ok(())));
        router
    }
}

struct Stop;

pub struct LspClient {
    root: PathBuf,
    timeout: Duration,
    server: async_lsp::ServerSocket,
    child: Child,
    mainloop: JoinHandle<async_lsp::Result<()>>,
    pub info: ServerInfo,
}

impl LspClient {
    pub async fn start(
        profile: &ServerProfile,
        root: &Path,
        request_timeout: Duration,
    ) -> Result<Self> {
        let root = root.canonicalize()?;
        let uri = Url::from_file_path(&root)
            .map_err(|_| AppError::Lsp(format!("cannot convert {} to file URI", root.display())))?;
        let workspace = WorkspaceFolder {
            uri: uri.clone(),
            name: root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("workspace")
                .into(),
        };
        let workspace_for_router = workspace.clone();
        let configuration = profile.workspace_configuration.clone();
        let (mainloop, mut server) = async_lsp::MainLoop::new_client(move |_| {
            ClientState::router(workspace_for_router, configuration)
        });
        let executable = profile
            .command
            .first()
            .ok_or_else(|| AppError::InvalidInput {
                code: "empty_lsp_command",
                message: format!("language server profile {} has no executable", profile.name),
            })?;
        let (program, prefix_args) =
            resolved_program(executable, std::env::var_os("PATH").as_deref());
        let mut child = Command::new(program)
            .args(prefix_args)
            .args(&profile.command[1..])
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| AppError::Lsp(format!("cannot start {}: {error}", profile.name)))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Lsp("language server stdout unavailable".into()))?
            .compat();
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::Lsp("language server stdin unavailable".into()))?
            .compat_write();
        let mainloop = tokio::spawn(mainloop.run_buffered(stdout, stdin));
        let initialize = InitializeParams {
            process_id: Some(std::process::id()),
            workspace_folders: Some(vec![workspace]),
            initialization_options: profile.initialization_options.clone(),
            capabilities: ClientCapabilities {
                workspace: Some(WorkspaceClientCapabilities {
                    configuration: Some(true),
                    workspace_folders: Some(true),
                    ..Default::default()
                }),
                text_document: Some(TextDocumentClientCapabilities::default()),
                window: Some(WindowClientCapabilities {
                    work_done_progress: Some(true),
                    ..Default::default()
                }),
                general: Some(GeneralClientCapabilities {
                    position_encodings: Some(vec![
                        PositionEncodingKind::UTF8,
                        PositionEncodingKind::UTF16,
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let initialized = timeout(request_timeout, server.initialize(initialize))
            .await
            .map_err(|_| AppError::Lsp(format!("{} initialize timed out", profile.name)))?
            .map_err(|error| AppError::Lsp(error.to_string()))?;
        server
            .initialized(InitializedParams {})
            .map_err(|error| AppError::Lsp(error.to_string()))?;
        let info = server_info(profile, &initialized);
        Ok(Self {
            root,
            timeout: request_timeout,
            server,
            child,
            mainloop,
            info,
        })
    }

    pub async fn document_symbols(
        &mut self,
        path: &Path,
        language_id: &str,
        text: String,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = Url::from_file_path(path)
            .map_err(|_| AppError::Lsp(format!("cannot convert {} to file URI", path.display())))?;
        self.open_document(path, language_id, text)?;
        timeout(
            self.timeout,
            self.server.document_symbol(DocumentSymbolParams {
                text_document: TextDocumentIdentifier { uri },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            }),
        )
        .await
        .map_err(|_| AppError::Lsp(format!("document symbols timed out for {}", path.display())))?
        .map_err(|error| AppError::Lsp(error.to_string()))
    }

    pub async fn references(&mut self, path: &Path, position: Position) -> Result<Vec<Location>> {
        let text_document_position = self.text_document_position(path, position)?;
        timeout(
            self.timeout,
            self.server.references(ReferenceParams {
                text_document_position,
                context: ReferenceContext {
                    include_declaration: true,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            }),
        )
        .await
        .map_err(|_| AppError::Lsp(format!("references timed out for {}", path.display())))?
        .map(|locations| locations.unwrap_or_default())
        .map_err(|error| AppError::Lsp(error.to_string()))
    }

    pub async fn definition(&mut self, path: &Path, position: Position) -> Result<Vec<Location>> {
        let text_document_position_params = self.text_document_position(path, position)?;
        let response = timeout(
            self.timeout,
            self.server.definition(GotoDefinitionParams {
                text_document_position_params,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            }),
        )
        .await
        .map_err(|_| AppError::Lsp(format!("definition timed out for {}", path.display())))?
        .map_err(|error| AppError::Lsp(error.to_string()))?;
        Ok(match response {
            None => Vec::new(),
            Some(GotoDefinitionResponse::Scalar(location)) => vec![location],
            Some(GotoDefinitionResponse::Array(locations)) => locations,
            Some(GotoDefinitionResponse::Link(links)) => links
                .into_iter()
                .map(|link| Location {
                    uri: link.target_uri,
                    range: link.target_selection_range,
                })
                .collect(),
        })
    }

    pub async fn prepare_call_hierarchy(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<CallHierarchyItem>> {
        let text_document_position_params = self.text_document_position(path, position)?;
        timeout(
            self.timeout,
            self.server
                .prepare_call_hierarchy(CallHierarchyPrepareParams {
                    text_document_position_params,
                    work_done_progress_params: Default::default(),
                }),
        )
        .await
        .map_err(|_| AppError::Lsp(format!("call hierarchy timed out for {}", path.display())))?
        .map(|items| items.unwrap_or_default())
        .map_err(|error| AppError::Lsp(error.to_string()))
    }

    pub async fn incoming_calls(
        &mut self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>> {
        timeout(
            self.timeout,
            self.server
                .incoming_calls(CallHierarchyIncomingCallsParams {
                    item,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                }),
        )
        .await
        .map_err(|_| AppError::Lsp("incoming calls timed out".into()))?
        .map(|items| items.unwrap_or_default())
        .map_err(|error| AppError::Lsp(error.to_string()))
    }

    pub async fn outgoing_calls(
        &mut self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>> {
        timeout(
            self.timeout,
            self.server
                .outgoing_calls(CallHierarchyOutgoingCallsParams {
                    item,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                }),
        )
        .await
        .map_err(|_| AppError::Lsp("outgoing calls timed out".into()))?
        .map(|items| items.unwrap_or_default())
        .map_err(|error| AppError::Lsp(error.to_string()))
    }

    fn text_document_position(
        &self,
        path: &Path,
        position: Position,
    ) -> Result<TextDocumentPositionParams> {
        let uri = Url::from_file_path(path)
            .map_err(|_| AppError::Lsp(format!("cannot convert {} to file URI", path.display())))?;
        Ok(TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        })
    }

    pub fn open_document(&mut self, path: &Path, language_id: &str, text: String) -> Result<()> {
        let uri = Url::from_file_path(path)
            .map_err(|_| AppError::Lsp(format!("cannot convert {} to file URI", path.display())))?;
        self.server
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: language_id.into(),
                    version: 0,
                    text,
                },
            })
            .map_err(|error| AppError::Lsp(error.to_string()))?;
        Ok(())
    }

    pub async fn finish(mut self) -> Result<()> {
        let _ = timeout(self.timeout, self.server.shutdown(())).await;
        let _ = self.server.exit(());
        let _ = self.server.emit(Stop);
        let _ = timeout(self.timeout, self.mainloop).await;
        if self.child.try_wait()?.is_none() {
            let _ = self.child.kill().await;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn server_info(profile: &ServerProfile, initialized: &InitializeResult) -> ServerInfo {
    let capabilities = &initialized.capabilities;
    ServerInfo {
        profile: profile.name.clone(),
        version: initialized
            .server_info
            .as_ref()
            .and_then(|i| i.version.clone())
            .or_else(|| command_version(profile)),
        position_encoding: capabilities
            .position_encoding
            .as_ref()
            .map(|e| e.as_str())
            .unwrap_or("utf-16")
            .into(),
        supports_document_symbols: provider_enabled(capabilities.document_symbol_provider.as_ref()),
        supports_references: provider_enabled(capabilities.references_provider.as_ref()),
        supports_definition: provider_enabled(capabilities.definition_provider.as_ref()),
        supports_call_hierarchy: provider_enabled(capabilities.call_hierarchy_provider.as_ref()),
    }
}

pub(crate) fn command_version(profile: &ServerProfile) -> Option<String> {
    let executable = profile.command.first()?;
    let (program, prefix_args) = resolved_program(executable, std::env::var_os("PATH").as_deref());
    let output = std::process::Command::new(program)
        .args(prefix_args)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .lines()
            .next()
            .unwrap_or("")
            .to_owned()
    })
}

fn discover_typescript_path(path: Option<&OsStr>) -> Option<PathBuf> {
    let server = find_on_path(OsStr::new("typescript-language-server"), path)?;
    let canonical = server.canonicalize().unwrap_or_else(|_| server.clone());
    for ancestor in canonical.ancestors() {
        if ancestor.file_name() == Some(OsStr::new("node_modules")) {
            let candidate = ancestor.join("typescript/lib");
            if candidate.join("tsserver.js").is_file() {
                return Some(candidate);
            }
        }
    }
    let bun_root = server
        .ancestors()
        .find(|ancestor| ancestor.file_name() == Some(OsStr::new(".bun")))?;
    let candidate = bun_root.join("install/global/node_modules/typescript/lib");
    candidate.join("tsserver.js").is_file().then_some(candidate)
}

fn resolved_program(executable: &OsStr, path: Option<&OsStr>) -> (OsString, Vec<OsString>) {
    let resolved = find_on_path(executable, path).unwrap_or_else(|| PathBuf::from(executable));
    let installed_by_bun = resolved
        .components()
        .any(|component| component.as_os_str() == OsStr::new(".bun"));
    if installed_by_bun && let Some(bun) = find_on_path(OsStr::new("bun"), path) {
        return (bun.into_os_string(), vec![resolved.into_os_string()]);
    }
    (resolved.into_os_string(), Vec::new())
}

fn find_on_path(executable: &OsStr, path: Option<&OsStr>) -> Option<PathBuf> {
    let executable_path = Path::new(executable);
    if executable_path.components().count() > 1 {
        return executable_path
            .is_file()
            .then(|| executable_path.to_owned());
    }
    path.into_iter()
        .flat_map(std::env::split_paths)
        .map(|directory| directory.join(executable))
        .find(|candidate| candidate.is_file())
}

fn provider_enabled<T: Serialize>(provider: Option<&T>) -> bool {
    provider
        .and_then(|value| serde_json::to_value(value).ok())
        .is_some_and(|value| value != Value::Bool(false) && !value.is_null())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn bun_installed_servers_bypass_node_runtime_shims() {
        let root = TempDir::new().unwrap();
        let bun_bin = root.path().join(".bun/bin");
        fs::create_dir_all(&bun_bin).unwrap();
        let bun = bun_bin.join("bun");
        let server = bun_bin.join("typescript-language-server");
        fs::write(&bun, "").unwrap();
        fs::write(&server, "#!/usr/bin/env node\n").unwrap();
        let path = std::env::join_paths([&bun_bin]).unwrap();

        let (program, args) = resolved_program(
            OsStr::new("typescript-language-server"),
            Some(path.as_os_str()),
        );

        assert_eq!(PathBuf::from(program), bun);
        assert_eq!(args, vec![server.into_os_string()]);
    }

    #[test]
    fn discovers_typescript_next_to_bun_global_servers() {
        let root = TempDir::new().unwrap();
        let bun_bin = root.path().join(".bun/bin");
        let typescript = root
            .path()
            .join(".bun/install/global/node_modules/typescript/lib");
        fs::create_dir_all(&bun_bin).unwrap();
        fs::create_dir_all(&typescript).unwrap();
        fs::write(bun_bin.join("typescript-language-server"), "").unwrap();
        fs::write(typescript.join("tsserver.js"), "").unwrap();
        let path = std::env::join_paths([&bun_bin]).unwrap();

        assert_eq!(
            discover_typescript_path(Some(path.as_os_str())),
            Some(typescript)
        );
    }
}

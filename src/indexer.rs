use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use async_lsp::lsp_types::{CallHierarchyItem, Location, Position as LspPosition, Range};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::graph::{ChangeGraph, CoverageEntry, EdgeKind, GraphNode, SourceLocation, SourceSide};
use crate::lsp::{LspClient, ServerProfile};
use crate::position::{Position, TextRange};
use crate::snapshot::{GitPath, Snapshot};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IndexOptions {
    pub time_budget_seconds: u64,
    pub request_timeout_seconds: u64,
    pub max_files: usize,
    pub max_symbols: usize,
    pub force: bool,
    #[serde(default)]
    pub profiles: Vec<ServerProfile>,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            time_budget_seconds: 180,
            request_timeout_seconds: 15,
            max_files: 500,
            max_symbols: 2_000,
            force: false,
            profiles: vec![
                ServerProfile::typescript(),
                ServerProfile::html(),
                ServerProfile::css(),
            ],
        }
    }
}

#[derive(Clone, Debug)]
struct Document {
    side: SourceSide,
    relative: GitPath,
    absolute: PathBuf,
    language_id: String,
    project: PathBuf,
    profile: ServerProfile,
}

pub fn graph_path(snapshot: &Snapshot) -> PathBuf {
    snapshot.storage_dir.join("graph.json")
}

pub async fn build(snapshot: &Snapshot, options: &IndexOptions) -> Result<ChangeGraph> {
    let destination = graph_path(snapshot);
    let mut documents = discover_documents(snapshot, &options.profiles)?;
    for document in &documents {
        if document.profile.name == "typescript" {
            ensure_test_project_configuration(&document.project)?;
        }
    }
    let expected_fingerprint = analysis_fingerprint(snapshot, &options.profiles)?;
    if !options.force && destination.exists() {
        let cached = ChangeGraph::load(&destination)?;
        if cached.fingerprint == expected_fingerprint {
            return Ok(cached);
        }
    }

    let mut graph = ChangeGraph::from_snapshot(snapshot);
    graph.fingerprint = expected_fingerprint;
    let started = Instant::now();
    documents.sort_by(|a, b| {
        (&a.side, &a.project, &a.relative.display).cmp(&(&b.side, &b.project, &b.relative.display))
    });
    if documents.len() > options.max_files {
        for document in documents.drain(options.max_files..) {
            graph.unfinished_frontier.push(format!(
                "{:?}:{}:file_limit",
                document.side, document.relative.display
            ));
        }
    }

    let mut groups: BTreeMap<(SourceSide, PathBuf, String), Vec<Document>> = BTreeMap::new();
    for document in documents {
        groups
            .entry((
                document.side,
                document.project.clone(),
                document.profile.name.clone(),
            ))
            .or_default()
            .push(document);
    }

    let mut symbol_count = 0usize;
    for ((side, project, profile_name), documents) in groups {
        if started.elapsed() >= Duration::from_secs(options.time_budget_seconds) {
            graph.unfinished_frontier.push(format!(
                "{side:?}:{}:{profile_name}:time_budget",
                project.display()
            ));
            continue;
        }
        let profile = documents[0].profile.clone();
        let mut coverage = CoverageEntry {
            project: project.clone(),
            side,
            profile: profile_name.clone(),
            server_version: None,
            position_encoding: None,
            status: "starting".into(),
            supported_relations: Vec::new(),
            message: None,
        };
        let mut client = match LspClient::start(
            &profile,
            &project,
            Duration::from_secs(options.request_timeout_seconds),
        )
        .await
        {
            Ok(client) => client,
            Err(error) => {
                coverage.status = "unavailable".into();
                coverage.message = Some(error.to_string());
                graph.coverage.push(coverage);
                continue;
            }
        };
        open_test_documents(&mut client, &profile, &project, options.max_files).await;
        coverage.server_version = client.info.version.clone();
        coverage.position_encoding = Some(client.info.position_encoding.clone());
        if client.info.supports_document_symbols {
            coverage.supported_relations.push("document_symbol".into());
        }
        if client.info.supports_references {
            coverage.supported_relations.push("references".into());
        }
        if client.info.supports_definition {
            coverage.supported_relations.push("definition".into());
        }
        if client.info.supports_call_hierarchy {
            coverage.supported_relations.push("call_hierarchy".into());
        }

        for document in documents {
            if started.elapsed() >= Duration::from_secs(options.time_budget_seconds)
                || symbol_count >= options.max_symbols
            {
                graph.unfinished_frontier.push(format!(
                    "{:?}:{}:{}",
                    document.side,
                    document.relative.display,
                    if symbol_count >= options.max_symbols {
                        "symbol_limit"
                    } else {
                        "time_budget"
                    }
                ));
                continue;
            }
            let text = match fs::read_to_string(&document.absolute) {
                Ok(text) => text,
                Err(error) => {
                    graph.unfinished_frontier.push(format!(
                        "{:?}:{}:unreadable:{error}",
                        document.side, document.relative.display
                    ));
                    continue;
                }
            };
            let response = match client
                .document_symbols(&document.absolute, &document.language_id, text)
                .await
            {
                Ok(Some(response)) => response,
                Ok(None) => continue,
                Err(error) => {
                    graph.unfinished_frontier.push(format!(
                        "{:?}:{}:symbols:{error}",
                        document.side, document.relative.display
                    ));
                    continue;
                }
            };
            let ids =
                graph.add_document_symbols(document.side, document.relative.clone(), response);
            symbol_count += ids.len();
            for id in ids {
                if graph.nodes.get(&id).is_some_and(|node| node.changed) {
                    expand_symbol(&mut graph, &mut client, &document, &id).await;
                }
            }
        }
        coverage.status = if graph.unfinished_frontier.iter().any(|item| {
            item.starts_with(&format!("{side:?}:"))
                && item.contains(project.file_name().and_then(|v| v.to_str()).unwrap_or(""))
        }) {
            "partial".into()
        } else {
            "complete".into()
        };
        if let Err(error) = client.finish().await {
            coverage.status = "partial".into();
            coverage.message = Some(error.to_string());
        }
        graph.coverage.push(coverage);
    }

    graph.link_counterparts(snapshot);

    graph.save(&destination)?;
    Ok(graph)
}

async fn open_test_documents(
    client: &mut LspClient,
    profile: &ServerProfile,
    project: &Path,
    limit: usize,
) {
    let mut opened = 0usize;
    let walker = ignore::WalkBuilder::new(project)
        .hidden(false)
        .git_ignore(true)
        .build();
    for entry in walker.filter_map(std::result::Result::ok) {
        if opened >= limit || !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        let relative = path.strip_prefix(project).unwrap_or(path).to_string_lossy();
        if !crate::graph::is_test_path(&relative) {
            continue;
        }
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let Some(language_id) = profile.languages.get(&extension) else {
            continue;
        };
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        if client.open_document(path, language_id, text).is_ok() {
            opened += 1;
        }
    }
}

fn ensure_test_project_configuration(project: &Path) -> Result<()> {
    let walker = ignore::WalkBuilder::new(project)
        .max_depth(Some(4))
        .hidden(false)
        .git_ignore(true)
        .build();
    let mut directories = BTreeSet::new();
    for entry in walker.filter_map(std::result::Result::ok) {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let relative = entry.path().strip_prefix(project).unwrap_or(entry.path());
        let first = relative
            .components()
            .next()
            .and_then(|component| component.as_os_str().to_str());
        if matches!(first, Some("test" | "tests" | "__tests__"))
            && let Some(first) = first
        {
            directories.insert(project.join(first));
        }
    }
    for directory in directories {
        let config = directory.join("tsconfig.json");
        if config.exists() {
            continue;
        }
        let relative_project =
            pathdiff::diff_paths(project, &directory).unwrap_or_else(|| PathBuf::from(".."));
        let extends = relative_project.join("tsconfig.json");
        fs::write(
            config,
            serde_json::to_vec_pretty(&serde_json::json!({
                "extends": extends.to_string_lossy().replace('\\', "/"),
                "include": [
                    "./**/*",
                    format!("{}/src/**/*", relative_project.to_string_lossy())
                ]
            }))?,
        )?;
    }
    Ok(())
}

async fn expand_symbol(
    graph: &mut ChangeGraph,
    client: &mut LspClient,
    document: &Document,
    symbol_id: &str,
) {
    let Some(node) = graph.nodes.get(symbol_id).cloned() else {
        return;
    };
    let selection = node
        .selection_range
        .unwrap_or_else(|| node.locations[0].range);
    let position = LspPosition::new(selection.start.line, selection.start.character);
    if client.info.supports_references {
        match client.references(&document.absolute, position).await {
            Ok(locations) => {
                for location in locations {
                    if let Some(location) = source_location(document, location) {
                        graph.add_reference(symbol_id, location, true);
                    }
                }
            }
            Err(error) => graph
                .unfinished_frontier
                .push(format!("{symbol_id}:references:{error}")),
        }
    }
    if client.info.supports_definition {
        match client.definition(&document.absolute, position).await {
            Ok(locations) => {
                for location in locations {
                    if let Some(location) = source_location(document, location) {
                        graph.add_location_relation(
                            symbol_id,
                            location,
                            EdgeKind::Definition,
                            true,
                            "resolved",
                            1.0,
                        );
                    }
                }
            }
            Err(error) => graph
                .unfinished_frontier
                .push(format!("{symbol_id}:definition:{error}")),
        }
    }
    if client.info.supports_call_hierarchy {
        match client
            .prepare_call_hierarchy(&document.absolute, position)
            .await
        {
            Ok(items) => expand_calls(graph, client, document, symbol_id, items).await,
            Err(error) => graph
                .unfinished_frontier
                .push(format!("{symbol_id}:calls:{error}")),
        }
    }
}

async fn expand_calls(
    graph: &mut ChangeGraph,
    client: &mut LspClient,
    document: &Document,
    symbol_id: &str,
    items: Vec<CallHierarchyItem>,
) {
    for item in items {
        match client.incoming_calls(item.clone()).await {
            Ok(calls) => {
                for call in calls {
                    if let Some(location) = item_location(document, &call.from) {
                        graph.add_location_relation(
                            symbol_id,
                            location,
                            EdgeKind::Calls,
                            false,
                            "resolved",
                            1.0,
                        );
                    }
                }
            }
            Err(error) => graph
                .unfinished_frontier
                .push(format!("{symbol_id}:incoming_calls:{error}")),
        }
        match client.outgoing_calls(item).await {
            Ok(calls) => {
                for call in calls {
                    if let Some(location) = item_location(document, &call.to) {
                        graph.add_location_relation(
                            symbol_id,
                            location,
                            EdgeKind::Calls,
                            true,
                            "resolved",
                            1.0,
                        );
                    }
                }
            }
            Err(error) => graph
                .unfinished_frontier
                .push(format!("{symbol_id}:outgoing_calls:{error}")),
        }
    }
}

fn item_location(document: &Document, item: &CallHierarchyItem) -> Option<SourceLocation> {
    source_location(
        document,
        Location {
            uri: item.uri.clone(),
            range: item.selection_range,
        },
    )
}

fn source_location(document: &Document, location: Location) -> Option<SourceLocation> {
    let absolute = location.uri.to_file_path().ok()?;
    let side_root = side_root_from_document(document)?;
    let relative = absolute.strip_prefix(side_root).ok()?;
    Some(SourceLocation {
        side: document.side,
        path: GitPath::from_bytes(relative.as_os_str().as_bytes().to_vec()),
        range: convert_range(location.range),
    })
}

fn side_root_from_document(document: &Document) -> Option<&Path> {
    document.absolute.ancestors().find(|ancestor| {
        ancestor.file_name().and_then(|value| value.to_str())
            == Some(match document.side {
                SourceSide::Left => "before",
                SourceSide::Right => "after",
            })
    })
}

fn convert_range(range: Range) -> TextRange {
    TextRange {
        start: Position {
            line: range.start.line,
            character: range.start.character,
        },
        end: Position {
            line: range.end.line,
            character: range.end.character,
        },
    }
}

fn discover_documents(snapshot: &Snapshot, profiles: &[ServerProfile]) -> Result<Vec<Document>> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for file in &snapshot.files {
        for (side, path) in [
            (SourceSide::Left, file.old_path.as_ref()),
            (SourceSide::Right, file.new_path.as_ref()),
        ] {
            let Some(path) = path else { continue };
            let relative = path.to_path_buf()?;
            let side_name = match side {
                SourceSide::Left => "before",
                SourceSide::Right => "after",
            };
            let absolute = snapshot.storage_dir.join(side_name).join(&relative);
            if !absolute.is_file() || !seen.insert((side, path.bytes_base64.clone())) {
                continue;
            }
            let extension = absolute
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let Some((profile, language_id)) = profiles.iter().find_map(|profile| {
                profile
                    .languages
                    .get(&extension)
                    .map(|language| (profile.clone(), language.clone()))
            }) else {
                continue;
            };
            let root = discover_project_root(&absolute, &snapshot.storage_dir.join(side_name));
            result.push(Document {
                side,
                relative: path.clone(),
                absolute,
                language_id,
                project: root,
                profile,
            });
        }
    }
    Ok(result)
}

fn discover_project_root(path: &Path, side_root: &Path) -> PathBuf {
    let mut current = path.parent().unwrap_or(side_root);
    loop {
        if ["tsconfig.json", "jsconfig.json", "package.json"]
            .iter()
            .any(|name| current.join(name).is_file())
        {
            return current.to_owned();
        }
        if current == side_root {
            return side_root.to_owned();
        }
        let Some(parent) = current.parent() else {
            return side_root.to_owned();
        };
        current = parent;
    }
}

fn analysis_fingerprint(snapshot: &Snapshot, profiles: &[ServerProfile]) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(snapshot.id.as_str());
    digest.update(&snapshot.source_fingerprint);
    for profile in profiles {
        digest.update(profile.name.as_bytes());
        for argument in &profile.command {
            digest.update(argument.as_bytes());
        }
        if let Some(version) = crate::lsp::command_version(profile) {
            digest.update(version.as_bytes());
        }
        digest.update(serde_json::to_vec(&profile.initialization_options)?);
        digest.update(serde_json::to_vec(&profile.workspace_configuration)?);
    }
    for side in ["before", "after"] {
        let root = snapshot.storage_dir.join(side);
        for entry in ignore::WalkBuilder::new(&root)
            .hidden(false)
            .git_ignore(true)
            .build()
            .filter_map(std::result::Result::ok)
        {
            let path = entry.path();
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    matches!(name, "tsconfig.json" | "jsconfig.json" | "package.json")
                })
            {
                digest.update(
                    path.strip_prefix(&root)
                        .unwrap_or(path)
                        .as_os_str()
                        .as_bytes(),
                );
                digest.update(fs::read(path)?);
            }
        }
    }
    Ok(hex::encode(digest.finalize()))
}

pub fn node_position(node: &GraphNode) -> Option<LspPosition> {
    node.selection_range
        .or_else(|| node.locations.first().map(|location| location.range))
        .map(|range| LspPosition::new(range.start.line, range.start.character))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_nearest_project_root() {
        let root = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(root.path().join("frontend/src")).unwrap();
        fs::write(root.path().join("frontend/package.json"), "{}").unwrap();
        let file = root.path().join("frontend/src/a.ts");
        fs::write(&file, "").unwrap();
        assert_eq!(
            discover_project_root(&file, root.path()),
            root.path().join("frontend")
        );
    }
}

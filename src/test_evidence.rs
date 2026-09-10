use std::collections::BTreeSet;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use uuid::Uuid;

use crate::error::{AppError, Result};
use crate::graph::{ChangeGraph, EdgeKind, GraphNode, NodeKind, SourceLocation, SourceSide};
use crate::position::{Position, TextRange};
use crate::settings::TestProfile;
use crate::snapshot::{GitPath, Snapshot};

pub const TEST_EVIDENCE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributionGranularity {
    Case,
    File,
    Suite,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Passed,
    Failed,
    Cancelled,
    TimedOut,
    Partial,
    Skipped,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TestIdentity {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub granularity: AttributionGranularity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExecutedRange {
    pub path: String,
    pub side: SourceSide,
    pub start_line: u32,
    pub end_line: u32,
    pub hits: u64,
    pub source_hash: Option<String>,
    pub compatible: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TestEvidence {
    pub id: String,
    pub run_id: String,
    pub test: TestIdentity,
    pub status: RunStatus,
    pub ranges: Vec<ExecutedRange>,
    pub producer: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TestRun {
    pub id: String,
    pub snapshot_id: String,
    pub side: SourceSide,
    pub profile: String,
    pub runner_version: Option<String>,
    pub command: Vec<String>,
    pub selection: Vec<String>,
    pub cache_key: String,
    pub status: RunStatus,
    pub completed: bool,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub stdout: String,
    pub stderr: String,
    pub output_truncated: bool,
    pub report_digest: Option<String>,
    pub evidence: Vec<TestEvidence>,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TestEvidenceStore {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub revision: u64,
    pub digest: String,
    pub runs: Vec<TestRun>,
}

#[derive(Clone, Debug, Deserialize)]
struct ManifestV1 {
    schema_version: u32,
    snapshot_id: String,
    side: SourceSide,
    run_id: String,
    runner: String,
    status: RunStatus,
    tests: Vec<ManifestTest>,
}

#[derive(Clone, Debug, Deserialize)]
struct ManifestTest {
    id: String,
    name: String,
    path: Option<String>,
    line: Option<u32>,
    granularity: AttributionGranularity,
    #[serde(default)]
    status: Option<RunStatus>,
    #[serde(default)]
    ranges: Vec<ManifestRange>,
}

#[derive(Clone, Debug, Deserialize)]
struct ManifestRange {
    path: String,
    start_line: u32,
    end_line: u32,
    hits: u64,
    source_hash: String,
}

impl TestEvidenceStore {
    pub fn load(snapshot: &Snapshot) -> Result<Self> {
        let path = path(snapshot);
        if !path.is_file() {
            return Ok(Self {
                schema_version: TEST_EVIDENCE_SCHEMA_VERSION,
                snapshot_id: snapshot.id.to_string(),
                revision: 0,
                digest: String::new(),
                runs: Vec::new(),
            });
        }
        let store: Self = serde_json::from_slice(&fs::read(path)?)?;
        if store.schema_version != TEST_EVIDENCE_SCHEMA_VERSION
            || store.snapshot_id != snapshot.id.as_str()
        {
            return Err(invalid(
                "test_evidence_snapshot_mismatch",
                "test evidence uses an unsupported schema or another snapshot",
            ));
        }
        Ok(store)
    }

    pub fn save(&mut self, snapshot: &Snapshot) -> Result<()> {
        self.revision += 1;
        self.recompute_digest();
        let destination = path(snapshot);
        let temporary = destination.with_extension(format!("json.{}.tmp", std::process::id()));
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary, destination)?;
        Ok(())
    }

    pub fn evidence_ids(&self) -> BTreeSet<String> {
        self.runs
            .iter()
            .flat_map(|run| run.evidence.iter().map(|evidence| evidence.id.clone()))
            .collect()
    }

    pub fn completed_cache(&self, key: &str) -> Option<&TestRun> {
        self.runs
            .iter()
            .rev()
            .find(|run| run.completed && run.cache_key == key)
    }

    fn recompute_digest(&mut self) {
        let mut digest = Sha256::new();
        for run in &self.runs {
            digest.update(run.id.as_bytes());
            digest.update(run.cache_key.as_bytes());
            digest.update(format!("{:?}", run.status).as_bytes());
            digest.update(run.report_digest.as_deref().unwrap_or("").as_bytes());
        }
        self.digest = hex::encode(digest.finalize());
    }
}

pub fn path(snapshot: &Snapshot) -> PathBuf {
    snapshot.storage_dir.join("test-evidence.json")
}

pub fn cache_key(
    snapshot: &Snapshot,
    side: SourceSide,
    profile_name: &str,
    profile: &TestProfile,
    selection: &[String],
) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(snapshot.id.as_str());
    digest.update(snapshot.source_fingerprint.as_bytes());
    digest.update(format!("{side:?}:{profile_name}").as_bytes());
    digest.update(serde_json::to_vec(profile)?);
    for selected in selection {
        digest.update(selected.as_bytes());
    }
    let root = captured_root(snapshot, side);
    for lock in [
        "package.json",
        "bun.lock",
        "bun.lockb",
        "pnpm-lock.yaml",
        "package-lock.json",
        "yarn.lock",
    ] {
        if let Ok(bytes) = fs::read(root.join(&profile.project_root).join(lock)) {
            digest.update(lock.as_bytes());
            digest.update(Sha256::digest(bytes));
        }
    }
    Ok(hex::encode(digest.finalize()))
}

pub fn dry_run(
    snapshot: &Snapshot,
    side: SourceSide,
    profile: &TestProfile,
    selection: &[String],
) -> Result<(PathBuf, Vec<String>)> {
    validate_relative(&profile.project_root)?;
    validate_relative(&profile.report_path)?;
    let workspace = PathBuf::from("<disposable-captured-workspace>");
    let report = workspace
        .join(&profile.project_root)
        .join(&profile.report_path);
    let argv = expand_argv(
        &profile.argv,
        &workspace,
        &snapshot.repository,
        &report,
        selection,
    );
    let mut command = vec![expand_value(
        &profile.executable.to_string_lossy(),
        &workspace,
        &snapshot.repository,
        &report,
    )];
    command.extend(argv);
    let _ = captured_root(snapshot, side);
    Ok((report, command))
}

pub async fn run(
    snapshot: &Snapshot,
    side: SourceSide,
    profile_name: &str,
    profile: &TestProfile,
    selection: Vec<String>,
) -> Result<TestRun> {
    if profile.attribution == "file" && selection.len() != 1 {
        return Err(invalid(
            "test_file_isolation_required",
            "file attribution requires exactly one selected test file per run",
        ));
    }
    let run_id = format!("run_{}", Uuid::new_v4().simple());
    let temporary = std::env::temp_dir().join(format!("lgr-{run_id}"));
    let workspace = temporary.join("workspace");
    fs::create_dir_all(&workspace)?;
    copy_tree(&captured_root(snapshot, side), &workspace)?;
    let project = workspace.join(&profile.project_root);
    let report = project.join(&profile.report_path);
    let cache_key = cache_key(snapshot, side, profile_name, profile, &selection)?;
    let argv = expand_argv(
        &profile.argv,
        &workspace,
        &snapshot.repository,
        &report,
        &selection,
    );
    let executable = PathBuf::from(expand_value(
        &profile.executable.to_string_lossy(),
        &workspace,
        &snapshot.repository,
        &report,
    ));
    let mut command_display = vec![executable.to_string_lossy().into_owned()];
    command_display.extend(argv.clone());
    let started_at = Utc::now();
    let mut diagnostics = Vec::new();
    let execution = async {
        if let Some(executable) = &profile.prepare_executable {
            let executable = PathBuf::from(expand_value(
                &executable.to_string_lossy(),
                &workspace,
                &snapshot.repository,
                &report,
            ));
            let prepare = run_process(
                &executable,
                &expand_argv(
                    &profile.prepare_argv,
                    &workspace,
                    &snapshot.repository,
                    &report,
                    &selection,
                ),
                &project,
                profile.timeout_seconds,
                profile.max_output_bytes,
            )
            .await?;
            if prepare.status != RunStatus::Passed {
                return Ok::<_, AppError>(prepare);
            }
        }
        run_process(
            &executable,
            &argv,
            &project,
            profile.timeout_seconds,
            profile.max_output_bytes,
        )
        .await
    }
    .await;
    let (status, stdout, stderr, output_truncated) = match execution {
        Ok(output) => (
            output.status,
            output.stdout,
            output.stderr,
            output.truncated,
        ),
        Err(error) => (RunStatus::Error, String::new(), error.to_string(), false),
    };
    let mut evidence = Vec::new();
    let mut report_digest = None;
    if report.is_file() {
        let bytes = bounded_read(&report, profile.max_report_bytes)?;
        report_digest = Some(hex::encode(Sha256::digest(&bytes)));
        let identity = if profile.attribution == "file" {
            let selected = selection[0].clone();
            let path = repository_relative_test_path(&profile.project_root, &selected)?;
            Some(TestIdentity {
                id: format!("test_file:{path}"),
                name: selected.clone(),
                path: Some(path),
                line: None,
                granularity: AttributionGranularity::File,
            })
        } else {
            None
        };
        evidence = import_report(
            snapshot,
            side,
            &profile.report_format,
            &bytes,
            &run_id,
            status,
            identity,
            true,
            &mut diagnostics,
        )?;
    } else {
        diagnostics.push(format!(
            "coverage report {} was not produced",
            profile.report_path.display()
        ));
    }
    let completed = !matches!(
        status,
        RunStatus::Cancelled | RunStatus::TimedOut | RunStatus::Error
    );
    let result = TestRun {
        id: run_id,
        snapshot_id: snapshot.id.to_string(),
        side,
        profile: profile_name.into(),
        runner_version: None,
        command: command_display,
        selection,
        cache_key,
        status,
        completed,
        started_at,
        completed_at: Some(Utc::now()),
        stdout,
        stderr,
        output_truncated,
        report_digest,
        evidence,
        diagnostics,
    };
    let _ = fs::remove_dir_all(&temporary);
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
pub fn import_external(
    snapshot: &Snapshot,
    side: SourceSide,
    format: &str,
    report: &Path,
    max_bytes: usize,
    test_file: Option<String>,
    status: RunStatus,
    trusted_captured_sources: bool,
) -> Result<TestRun> {
    let bytes = bounded_read(report, max_bytes)?;
    let run_id = format!("run_{}", Uuid::new_v4().simple());
    let identity = test_file.map(|path| TestIdentity {
        id: format!("test_file:{path}"),
        name: path.clone(),
        path: Some(path),
        line: None,
        granularity: AttributionGranularity::File,
    });
    let mut diagnostics = Vec::new();
    let evidence = import_report(
        snapshot,
        side,
        format,
        &bytes,
        &run_id,
        status,
        identity,
        trusted_captured_sources,
        &mut diagnostics,
    )?;
    Ok(TestRun {
        id: run_id,
        snapshot_id: snapshot.id.to_string(),
        side,
        profile: "import".into(),
        runner_version: None,
        command: Vec::new(),
        selection: Vec::new(),
        cache_key: hex::encode(Sha256::digest(&bytes)),
        status,
        completed: true,
        started_at: Utc::now(),
        completed_at: Some(Utc::now()),
        stdout: String::new(),
        stderr: String::new(),
        output_truncated: false,
        report_digest: Some(hex::encode(Sha256::digest(&bytes))),
        evidence,
        diagnostics,
    })
}

#[allow(clippy::too_many_arguments)]
fn import_report(
    snapshot: &Snapshot,
    side: SourceSide,
    format: &str,
    bytes: &[u8],
    run_id: &str,
    status: RunStatus,
    identity: Option<TestIdentity>,
    trusted: bool,
    diagnostics: &mut Vec<String>,
) -> Result<Vec<TestEvidence>> {
    match format {
        "manifest-v1" => import_manifest(snapshot, bytes),
        "istanbul-json" => {
            let ranges = import_istanbul(snapshot, side, bytes, trusted, diagnostics)?;
            Ok(vec![evidence(run_id, status, identity, ranges, "istanbul")])
        }
        "lcov" => {
            let ranges = import_lcov(snapshot, side, bytes, trusted, diagnostics)?;
            Ok(vec![evidence(run_id, status, identity, ranges, "lcov")])
        }
        _ => Err(invalid(
            "unsupported_coverage_format",
            "format must be manifest-v1, istanbul-json, or lcov",
        )),
    }
}

fn evidence(
    run_id: &str,
    status: RunStatus,
    identity: Option<TestIdentity>,
    ranges: Vec<ExecutedRange>,
    producer: &str,
) -> TestEvidence {
    let test = identity.unwrap_or_else(|| TestIdentity {
        id: format!("suite:{run_id}"),
        name: "combined suite".into(),
        path: None,
        line: None,
        granularity: AttributionGranularity::Suite,
    });
    let raw = format!("{run_id}:{}:{producer}", test.id);
    let digest = hex::encode(Sha256::digest(raw.as_bytes()));
    TestEvidence {
        id: format!("tev_{}", &digest[..20]),
        run_id: run_id.into(),
        test,
        status,
        ranges,
        producer: producer.into(),
    }
}

fn import_manifest(snapshot: &Snapshot, bytes: &[u8]) -> Result<Vec<TestEvidence>> {
    let manifest: ManifestV1 = serde_json::from_slice(bytes).map_err(|error| {
        invalid(
            "invalid_test_manifest",
            format!("manifest is invalid: {error}"),
        )
    })?;
    if manifest.schema_version != 1 || manifest.snapshot_id != snapshot.id.as_str() {
        return Err(invalid(
            "test_manifest_mismatch",
            "manifest schema or snapshot does not match",
        ));
    }
    let mut result = Vec::new();
    for test in manifest.tests {
        let mut ranges = Vec::new();
        for range in test.ranges {
            if range.start_line == 0 || range.end_line < range.start_line {
                return Err(invalid(
                    "invalid_coverage_range",
                    "manifest coverage range is invalid",
                ));
            }
            let current = source_hash(snapshot, manifest.side, &range.path)?;
            ranges.push(ExecutedRange {
                path: range.path,
                side: manifest.side,
                start_line: range.start_line,
                end_line: range.end_line,
                hits: range.hits,
                compatible: current.as_deref() == Some(range.source_hash.as_str()),
                source_hash: Some(range.source_hash),
            });
        }
        result.push(evidence(
            &manifest.run_id,
            test.status.unwrap_or(manifest.status),
            Some(TestIdentity {
                id: test.id,
                name: test.name,
                path: test.path,
                line: test.line,
                granularity: test.granularity,
            }),
            ranges,
            &manifest.runner,
        ));
    }
    Ok(result)
}

fn import_istanbul(
    snapshot: &Snapshot,
    side: SourceSide,
    bytes: &[u8],
    trusted: bool,
    diagnostics: &mut Vec<String>,
) -> Result<Vec<ExecutedRange>> {
    let root: Value = serde_json::from_slice(bytes)
        .map_err(|error| invalid("invalid_istanbul_json", error.to_string()))?;
    let files = root
        .as_object()
        .ok_or_else(|| invalid("invalid_istanbul_json", "report root must be an object"))?;
    let mut ranges = Vec::new();
    for (reported_path, file) in files {
        let path = report_path(
            snapshot,
            side,
            file.get("path")
                .and_then(Value::as_str)
                .unwrap_or(reported_path),
        );
        let Some(path) = path else {
            diagnostics.push(format!("unmapped coverage source {reported_path}"));
            continue;
        };
        let statements = file
            .get("statementMap")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("invalid_istanbul_json", "statementMap is missing"))?;
        let counts = file
            .get("s")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("invalid_istanbul_json", "statement counts are missing"))?;
        let hash = source_hash(snapshot, side, &path)?;
        for (id, statement) in statements {
            let start = statement
                .pointer("/start/line")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32;
            let end = statement
                .pointer("/end/line")
                .and_then(Value::as_u64)
                .unwrap_or(start as u64) as u32;
            let hits = counts.get(id).and_then(Value::as_u64).unwrap_or(0);
            if start == 0 || end < start {
                continue;
            }
            ranges.push(ExecutedRange {
                path: path.clone(),
                side,
                start_line: start,
                end_line: end,
                hits,
                source_hash: trusted.then(|| hash.clone()).flatten(),
                compatible: trusted && hash.is_some(),
            });
        }
    }
    if !trusted {
        diagnostics.push(
            "coverage has no captured-source fingerprint; ranges are informational only".into(),
        );
    }
    Ok(ranges)
}

fn import_lcov(
    snapshot: &Snapshot,
    side: SourceSide,
    bytes: &[u8],
    trusted: bool,
    diagnostics: &mut Vec<String>,
) -> Result<Vec<ExecutedRange>> {
    let text =
        std::str::from_utf8(bytes).map_err(|error| invalid("invalid_lcov", error.to_string()))?;
    let mut current = None;
    let mut ranges = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("SF:") {
            current = report_path(snapshot, side, value);
        } else if let Some(value) = line.strip_prefix("DA:") {
            let Some(path) = current.clone() else {
                continue;
            };
            let Some((line, hits)) = value.split_once(',') else {
                continue;
            };
            let Ok(line) = line.parse::<u32>() else {
                continue;
            };
            let Ok(hits) = hits.parse::<u64>() else {
                continue;
            };
            let hash = source_hash(snapshot, side, &path)?;
            ranges.push(ExecutedRange {
                path,
                side,
                start_line: line,
                end_line: line,
                hits,
                source_hash: trusted.then(|| hash.clone()).flatten(),
                compatible: trusted && hash.is_some(),
            });
        }
    }
    if !trusted {
        diagnostics.push(
            "coverage has no captured-source fingerprint; ranges are informational only".into(),
        );
    }
    Ok(ranges)
}

pub fn project_runtime(
    mut graph: ChangeGraph,
    snapshot: &Snapshot,
    store: &TestEvidenceStore,
) -> ChangeGraph {
    let projected_tests: BTreeSet<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::RuntimeTest && edge.producer == "test-evidence")
        .flat_map(|edge| [&edge.from, &edge.to])
        .filter(|id| {
            graph
                .nodes
                .get(*id)
                .is_some_and(|node| node.kind == NodeKind::Test)
        })
        .cloned()
        .collect();
    graph
        .edges
        .retain(|edge| edge.kind != EdgeKind::RuntimeTest || edge.producer != "test-evidence");
    graph.nodes.retain(|id, _| !projected_tests.contains(id));
    let review_ids = crate::review_units::active_leaf_ids(&graph);
    for run in store
        .runs
        .iter()
        .rev()
        .filter(|run| run.snapshot_id == snapshot.id.as_str())
    {
        for evidence in &run.evidence {
            if evidence.test.granularity == AttributionGranularity::Suite {
                continue;
            }
            let Some(test_path) = evidence.test.path.as_deref() else {
                continue;
            };
            let Some(git_path) = snapshot_path(snapshot, run.side, test_path) else {
                continue;
            };
            let test_id = format!(
                "test_{}",
                &hex::encode(Sha256::digest(
                    format!("{:?}:{test_path}:{}", run.side, evidence.test.id).as_bytes()
                ))[..20]
            );
            let line = evidence
                .test
                .line
                .or_else(|| changed_test_line(snapshot, run.side, test_path))
                .unwrap_or(1)
                .saturating_sub(1);
            graph
                .nodes
                .entry(test_id.clone())
                .or_insert_with(|| GraphNode {
                    id: test_id.clone(),
                    kind: NodeKind::Test,
                    name: evidence.test.name.clone(),
                    symbol_kind: Some(
                        format!("{:?}", evidence.test.granularity).to_ascii_lowercase(),
                    ),
                    changed: false,
                    locations: vec![SourceLocation {
                        side: run.side,
                        path: git_path.clone(),
                        range: TextRange {
                            start: Position { line, character: 0 },
                            end: Position {
                                line: line + 1,
                                character: 0,
                            },
                        },
                    }],
                    selection_range: None,
                    hunk_ids: Vec::new(),
                });
            for range in evidence
                .ranges
                .iter()
                .filter(|range| range.compatible && range.hits > 0)
            {
                for node_id in &review_ids {
                    let Some(node) = graph.nodes.get(node_id) else {
                        continue;
                    };
                    let intersects = node.locations.iter().any(|location| {
                        location.side == range.side
                            && location.path.display == range.path
                            && location.range.start.line < range.end_line
                            && range.start_line.saturating_sub(1) < location.range.end.line
                    });
                    if intersects {
                        let exists = graph.edges.iter().any(|edge| {
                            edge.from == *node_id
                                && edge.to == test_id
                                && edge.kind == EdgeKind::RuntimeTest
                        });
                        if !exists {
                            graph.add_edge(
                                node_id.clone(),
                                test_id.clone(),
                                EdgeKind::RuntimeTest,
                                "test-evidence",
                                &format!("{}:{:?}", evidence.id, evidence.status),
                                1.0,
                                None,
                            );
                        }
                    }
                }
            }
        }
    }
    graph
}

fn captured_root(snapshot: &Snapshot, side: SourceSide) -> PathBuf {
    snapshot.storage_dir.join(match side {
        SourceSide::Left => "before",
        SourceSide::Right => "after",
    })
}

fn changed_test_line(snapshot: &Snapshot, side: SourceSide, path: &str) -> Option<u32> {
    snapshot.files.iter().find_map(|file| {
        let candidate = match side {
            SourceSide::Left => file.old_path.as_ref(),
            SourceSide::Right => file.new_path.as_ref(),
        }?;
        (candidate.display == path).then(|| {
            file.hunks.first().map_or(1, |hunk| match side {
                SourceSide::Left => hunk.old_start,
                SourceSide::Right => hunk.new_start,
            })
        })
    })
}

fn snapshot_path(snapshot: &Snapshot, side: SourceSide, path: &str) -> Option<GitPath> {
    if let Some(path) = snapshot.files.iter().find_map(|file| {
        match side {
            SourceSide::Left => file.old_path.as_ref(),
            SourceSide::Right => file.new_path.as_ref(),
        }
        .filter(|candidate| candidate.display == path)
    }) {
        return Some(path.clone());
    }
    let relative = Path::new(path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || !captured_root(snapshot, side).join(relative).is_file()
    {
        return None;
    }
    Some(GitPath::from_bytes(
        relative.as_os_str().as_bytes().to_vec(),
    ))
}

fn source_hash(snapshot: &Snapshot, side: SourceSide, path: &str) -> Result<Option<String>> {
    let Some(git_path) = snapshot_path(snapshot, side, path) else {
        return Ok(None);
    };
    let bytes = fs::read(captured_root(snapshot, side).join(git_path.to_path_buf()?))?;
    Ok(Some(hex::encode(Sha256::digest(bytes))))
}

fn report_path(snapshot: &Snapshot, side: SourceSide, reported: &str) -> Option<String> {
    let normalized = reported.replace('\\', "/");
    snapshot
        .files
        .iter()
        .filter_map(|file| match side {
            SourceSide::Left => file.old_path.as_ref(),
            SourceSide::Right => file.new_path.as_ref(),
        })
        .find(|candidate| {
            normalized == candidate.display
                || normalized.ends_with(&format!("/{}", candidate.display))
        })
        .map(|path| path.display.clone())
}

fn validate_relative(path: &Path) -> Result<()> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(invalid(
            "test_path_escape",
            "test project and report paths must stay inside the disposable workspace",
        ));
    }
    Ok(())
}

fn repository_relative_test_path(project_root: &Path, selection: &str) -> Result<String> {
    let path = project_root.join(selection);
    validate_relative(&path)?;
    let normalized = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value),
            Component::CurDir => None,
            _ => None,
        })
        .collect::<PathBuf>();
    Ok(normalized.to_string_lossy().replace('\\', "/"))
}

fn expand_argv(
    template: &[String],
    workspace: &Path,
    repository: &Path,
    report: &Path,
    selection: &[String],
) -> Vec<String> {
    let mut result = Vec::new();
    for value in template {
        match value.as_str() {
            "{selection}" => result.extend(selection.iter().cloned()),
            _ => result.push(expand_value(value, workspace, repository, report)),
        }
    }
    result
}

fn expand_value(value: &str, workspace: &Path, repository: &Path, report: &Path) -> String {
    value
        .replace("{workspace}", &workspace.to_string_lossy())
        .replace("{repository}", &repository.to_string_lossy())
        .replace("{report}", &report.to_string_lossy())
}

struct ProcessOutput {
    status: RunStatus,
    stdout: String,
    stderr: String,
    truncated: bool,
}

async fn run_process(
    executable: &Path,
    argv: &[String],
    cwd: &Path,
    timeout_seconds: u64,
    max_output: usize,
) -> Result<ProcessOutput> {
    run_process_with_cancel(executable, argv, cwd, timeout_seconds, max_output, None).await
}

async fn run_process_with_cancel(
    executable: &Path,
    argv: &[String],
    cwd: &Path,
    timeout_seconds: u64,
    max_output: usize,
    cancellation: Option<tokio::sync::oneshot::Receiver<()>>,
) -> Result<ProcessOutput> {
    let mut command = Command::new(executable);
    command
        .args(argv)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command.as_std_mut().process_group(0);
    let mut child = command.spawn().map_err(|error| {
        invalid(
            "test_dependency_missing",
            format!("could not start {}: {error}", executable.display()),
        )
    })?;
    let pid = child.id().unwrap_or(0);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| invalid("test_output_unavailable", "test stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| invalid("test_output_unavailable", "test stderr unavailable"))?;
    let stdout_task = tokio::spawn(read_bounded(stdout, max_output));
    let stderr_task = tokio::spawn(read_bounded(stderr, max_output));
    let cancellation_signal = async move {
        if let Some(receiver) = cancellation {
            let _ = receiver.await;
        } else {
            let _ = tokio::signal::ctrl_c().await;
        }
    };
    let (status, outcome) = tokio::select! {
        result = child.wait() => (result?, None),
        _ = tokio::time::sleep(Duration::from_secs(timeout_seconds)) => {
            let status = stop_group(&mut child, pid).await?;
            (status, Some(RunStatus::TimedOut))
        }
        _ = cancellation_signal => {
            let status = stop_group(&mut child, pid).await?;
            (status, Some(RunStatus::Cancelled))
        }
    };
    let (stdout, stdout_truncated) = stdout_task
        .await
        .map_err(|error| invalid("test_output_task_failed", error.to_string()))??;
    let (stderr, stderr_truncated) = stderr_task
        .await
        .map_err(|error| invalid("test_output_task_failed", error.to_string()))??;
    Ok(ProcessOutput {
        status: outcome.unwrap_or(if status.success() {
            RunStatus::Passed
        } else {
            RunStatus::Failed
        }),
        stdout,
        stderr,
        truncated: stdout_truncated || stderr_truncated,
    })
}

async fn read_bounded<R: tokio::io::AsyncRead + Unpin>(
    reader: R,
    limit: usize,
) -> Result<(String, bool)> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

async fn stop_group(
    child: &mut tokio::process::Child,
    pid: u32,
) -> std::io::Result<std::process::ExitStatus> {
    if pid == 0 {
        child.kill().await?;
        return child.wait().await;
    }
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
    match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
        Ok(status) => status,
        Err(_) => {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            child.wait().await
        }
    }
}

fn bounded_read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > limit as u64 {
        return Err(invalid(
            "coverage_report_too_large",
            format!("{} exceeds {limit} bytes", path.display()),
        ));
    }
    Ok(fs::read(path)?)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        let entry = entry.map_err(|error| invalid("captured_copy_failed", error.to_string()))?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|error| invalid("captured_copy_failed", error.to_string()))?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_symlink() {
            symlink(fs::read_link(entry.path())?, target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn invalid(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::InvalidInput {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn lcov_and_istanbul_aggregate_do_not_invent_tests_or_current_links() {
        let snapshot = fixture();
        let mut diagnostics = Vec::new();
        let lcov = import_lcov(
            &snapshot,
            SourceSide::Right,
            b"SF:src/a.ts\nDA:1,2\nend_of_record\n",
            false,
            &mut diagnostics,
        )
        .unwrap();
        assert_eq!(lcov.len(), 1);
        assert!(!lcov[0].compatible);
        let evidence = evidence("run", RunStatus::Passed, None, lcov, "lcov");
        assert_eq!(evidence.test.granularity, AttributionGranularity::Suite);
        assert!(evidence.test.path.is_none());
    }

    #[test]
    fn cache_changes_with_side_selection_and_configuration() {
        let mut snapshot = fixture();
        let profile = TestProfile {
            executable: "node".into(),
            argv: vec!["test.js".into(), "{selection}".into()],
            prepare_argv: Vec::new(),
            prepare_executable: None,
            project_root: ".".into(),
            timeout_seconds: 10,
            max_output_bytes: 1024,
            max_report_bytes: 1024,
            report_format: "lcov".into(),
            report_path: "coverage/lcov.info".into(),
            attribution: "file".into(),
        };
        let one = cache_key(
            &snapshot,
            SourceSide::Right,
            "jest",
            &profile,
            &["a.test.ts".into()],
        )
        .unwrap();
        let two = cache_key(
            &snapshot,
            SourceSide::Left,
            "jest",
            &profile,
            &["a.test.ts".into()],
        )
        .unwrap();
        let three = cache_key(
            &snapshot,
            SourceSide::Right,
            "jest",
            &profile,
            &["b.test.ts".into()],
        )
        .unwrap();
        assert_ne!(one, two);
        assert_ne!(one, three);

        let mut runner_changed = profile.clone();
        runner_changed.executable = "bun".into();
        let four = cache_key(
            &snapshot,
            SourceSide::Right,
            "jest",
            &runner_changed,
            &["a.test.ts".into()],
        )
        .unwrap();
        assert_ne!(one, four);

        snapshot.source_fingerprint = "changed-source".into();
        let five = cache_key(
            &snapshot,
            SourceSide::Right,
            "jest",
            &profile,
            &["a.test.ts".into()],
        )
        .unwrap();
        assert_ne!(one, five);

        snapshot.source_fingerprint = "source".into();
        fs::write(
            snapshot.storage_dir.join("after/package-lock.json"),
            b"{\"lockfileVersion\":3}",
        )
        .unwrap();
        let six = cache_key(
            &snapshot,
            SourceSide::Right,
            "jest",
            &profile,
            &["a.test.ts".into()],
        )
        .unwrap();
        assert_ne!(one, six);
    }

    #[test]
    fn coverage_import_rejects_malformed_and_oversized_reports() {
        let snapshot = fixture();
        let mut diagnostics = Vec::new();
        let malformed = import_istanbul(
            &snapshot,
            SourceSide::Right,
            br#"["not-an-object"]"#,
            false,
            &mut diagnostics,
        )
        .unwrap_err();
        assert!(
            malformed
                .to_string()
                .contains("report root must be an object")
        );
        assert!(
            import_lcov(
                &snapshot,
                SourceSide::Right,
                &[0xff, 0xfe],
                false,
                &mut diagnostics,
            )
            .is_err()
        );

        let report = snapshot.storage_dir.join("oversized.lcov");
        fs::write(&report, vec![b'x'; 65]).unwrap();
        let oversized = import_external(
            &snapshot,
            SourceSide::Right,
            "lcov",
            &report,
            64,
            None,
            RunStatus::Passed,
            false,
        )
        .unwrap_err();
        assert!(oversized.to_string().contains("exceeds 64 bytes"));
    }

    #[tokio::test]
    async fn timeout_terminates_process_group_and_bounds_output() {
        let started = Instant::now();
        let output = run_process(
            Path::new("/bin/sh"),
            &["-c".into(), "yes x | head -c 4096; sleep 30 & wait".into()],
            Path::new("/tmp"),
            1,
            128,
        )
        .await
        .unwrap();
        assert_eq!(output.status, RunStatus::TimedOut);
        assert!(output.truncated);
        assert!(output.stdout.len() <= 128);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn explicit_cancellation_terminates_process_group_and_keeps_output() {
        let started = Instant::now();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = sender.send(());
        });
        let output = run_process_with_cancel(
            Path::new("/bin/sh"),
            &["-c".into(), "printf started; sleep 30 & wait".into()],
            Path::new("/tmp"),
            30,
            128,
            Some(receiver),
        )
        .await
        .unwrap();
        assert_eq!(output.status, RunStatus::Cancelled);
        assert_eq!(output.stdout, "started");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn failed_isolated_file_run_retains_hits_without_mutating_capture() {
        let snapshot = fixture();
        let before = fs::read(snapshot.storage_dir.join("after/src/a.ts")).unwrap();
        let report = r#"{"src/a.ts":{"path":"src/a.ts","statementMap":{"0":{"start":{"line":1,"column":0},"end":{"line":1,"column":18}}},"s":{"0":1}}}"#;
        let script = format!(
            "mkdir -p coverage; printf '%s' '{}' > coverage/coverage-final.json; exit 1",
            report
        );
        let profile = TestProfile {
            executable: "/bin/sh".into(),
            argv: vec!["-c".into(), script],
            prepare_argv: Vec::new(),
            prepare_executable: None,
            project_root: ".".into(),
            timeout_seconds: 10,
            max_output_bytes: 1024,
            max_report_bytes: 4096,
            report_format: "istanbul-json".into(),
            report_path: "coverage/coverage-final.json".into(),
            attribution: "file".into(),
        };
        let result = run(
            &snapshot,
            SourceSide::Right,
            "jest",
            &profile,
            vec!["src/a.test.ts".into()],
        )
        .await
        .unwrap();
        assert_eq!(result.status, RunStatus::Failed);
        assert_eq!(
            result.evidence[0].test.granularity,
            AttributionGranularity::File
        );
        assert!(result.evidence[0].ranges[0].compatible);
        assert_eq!(result.evidence[0].ranges[0].hits, 1);
        assert_eq!(
            fs::read(snapshot.storage_dir.join("after/src/a.ts")).unwrap(),
            before
        );
    }

    #[test]
    fn manifest_requires_matching_hash_and_preserves_case_precision() {
        let snapshot = fixture();
        let manifest = serde_json::json!({
            "schema_version": 1,
            "snapshot_id": snapshot.id,
            "side": "right",
            "run_id": "run_manifest",
            "runner": "fixture",
            "status": "passed",
            "tests": [{
                "id": "case:one",
                "name": "does one",
                "path": "src/a.test.ts",
                "line": 3,
                "granularity": "case",
                "ranges": [{"path":"src/a.ts","start_line":1,"end_line":1,"hits":1,"source_hash":"wrong"}]
            }]
        });
        let evidence = import_manifest(&snapshot, &serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert_eq!(evidence[0].test.granularity, AttributionGranularity::Case);
        assert!(!evidence[0].ranges[0].compatible);
    }

    #[test]
    fn runtime_projection_links_changed_code_to_unchanged_test_in_both_directions() {
        let snapshot = fixture();
        fs::write(
            snapshot.storage_dir.join("after/src/a.test.ts"),
            "test('a', () => a());\n",
        )
        .unwrap();
        let location = SourceLocation {
            side: SourceSide::Right,
            path: GitPath::from_bytes(b"src/a.ts".to_vec()),
            range: TextRange {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 1,
                    character: 0,
                },
            },
        };
        let mut graph = ChangeGraph::from_snapshot(&snapshot);
        graph.nodes.insert(
            "h_changed".into(),
            GraphNode {
                id: "h_changed".into(),
                kind: NodeKind::Hunk,
                name: "change".into(),
                symbol_kind: None,
                changed: true,
                locations: vec![location],
                selection_range: None,
                hunk_ids: vec!["h_changed".into()],
            },
        );
        let evidence = TestEvidence {
            id: "tev_one".into(),
            run_id: "run_one".into(),
            status: RunStatus::Failed,
            producer: "fixture".into(),
            test: TestIdentity {
                id: "test-file".into(),
                name: "a.test.ts".into(),
                path: Some("src/a.test.ts".into()),
                line: Some(1),
                granularity: AttributionGranularity::File,
            },
            ranges: vec![ExecutedRange {
                path: "src/a.ts".into(),
                side: SourceSide::Right,
                start_line: 1,
                end_line: 1,
                hits: 1,
                source_hash: Some("hash".into()),
                compatible: true,
            }],
        };
        let mut store = TestEvidenceStore {
            schema_version: 1,
            snapshot_id: snapshot.id.to_string(),
            revision: 1,
            digest: "digest".into(),
            runs: vec![TestRun {
                id: "run_one".into(),
                snapshot_id: snapshot.id.to_string(),
                side: SourceSide::Right,
                profile: "fixture".into(),
                runner_version: None,
                command: vec![],
                selection: vec![],
                cache_key: "key".into(),
                status: RunStatus::Failed,
                completed: true,
                started_at: Utc::now(),
                completed_at: Some(Utc::now()),
                stdout: String::new(),
                stderr: String::new(),
                output_truncated: false,
                report_digest: None,
                evidence: vec![evidence],
                diagnostics: vec![],
            }],
        };
        let mut latest = store.runs[0].clone();
        latest.id = "run_two".into();
        latest.status = RunStatus::Passed;
        latest.evidence[0].id = "tev_two".into();
        latest.evidence[0].run_id = "run_two".into();
        latest.evidence[0].status = RunStatus::Passed;
        store.runs.push(latest);
        let projected = project_runtime(graph, &snapshot, &store);
        let test = projected
            .nodes
            .values()
            .find(|node| node.kind == NodeKind::Test)
            .unwrap();
        let test_id = test.id.clone();
        assert!(!test.changed);
        assert_eq!(test.locations[0].path.display, "src/a.test.ts");
        assert!(projected.edges.iter().any(|edge| edge.from == "h_changed"
            && edge.to == test_id
            && edge.kind == EdgeKind::RuntimeTest
            && edge.evidence_kind == "tev_two:Passed"));
        let projected_twice = project_runtime(projected, &snapshot, &store);
        assert_eq!(
            projected_twice
                .edges
                .iter()
                .filter(|edge| edge.kind == EdgeKind::RuntimeTest)
                .count(),
            1
        );
        let walked = projected_twice.walk(
            std::slice::from_ref(&test_id),
            &BTreeSet::from([EdgeKind::RuntimeTest]),
            1,
            10,
        );
        assert!(walked.contains(&"h_changed".to_owned()));
    }

    #[test]
    fn dry_run_expands_repository_in_executable_and_arguments() {
        let snapshot = fixture();
        let profile = TestProfile {
            executable: "{repository}/tools/node".into(),
            argv: vec![
                "{repository}/runner.mjs".into(),
                "--workspace={workspace}".into(),
                "--report={report}".into(),
                "{selection}".into(),
            ],
            prepare_argv: Vec::new(),
            prepare_executable: None,
            project_root: "project".into(),
            timeout_seconds: 10,
            max_output_bytes: 1024,
            max_report_bytes: 1024,
            report_format: "istanbul-json".into(),
            report_path: "coverage/coverage-final.json".into(),
            attribution: "file".into(),
        };

        let (_, command) = dry_run(
            &snapshot,
            SourceSide::Right,
            &profile,
            &["test/a.test.ts".into()],
        )
        .unwrap();
        let repository = snapshot.repository.to_string_lossy();
        assert_eq!(command[0], format!("{repository}/tools/node"));
        assert_eq!(command[1], format!("{repository}/runner.mjs"));
        assert_eq!(command[4], "test/a.test.ts");
    }

    #[test]
    fn file_attribution_normalizes_selection_from_project_to_repository() {
        assert_eq!(
            repository_relative_test_path(
                Path::new("frontend/simplified"),
                "src/editor/editor.test.tsx"
            )
            .unwrap(),
            "frontend/simplified/src/editor/editor.test.tsx"
        );
        assert_eq!(
            repository_relative_test_path(Path::new("."), "tests/a.test.ts").unwrap(),
            "tests/a.test.ts"
        );
        assert!(repository_relative_test_path(Path::new("frontend"), "../secret.test.ts").is_err());
    }

    #[test]
    fn file_level_test_preview_uses_changed_hunk_coordinates() {
        let mut snapshot = fixture();
        snapshot.files.push(crate::snapshot::FileChange {
            status: "M".into(),
            old_path: Some(GitPath::from_bytes(b"src/a.test.ts".to_vec())),
            new_path: Some(GitPath::from_bytes(b"src/a.test.ts".to_vec())),
            old_mode: String::new(),
            new_mode: String::new(),
            old_object: String::new(),
            new_object: String::new(),
            before_blob: None,
            after_blob: None,
            binary: false,
            submodule: false,
            hunks: vec![crate::snapshot::Hunk {
                id: "h_test".into(),
                old_start: 40,
                old_count: 2,
                new_start: 44,
                new_count: 3,
                header: "@@ -40,2 +44,3 @@".into(),
                patch: String::new(),
            }],
        });

        assert_eq!(
            changed_test_line(&snapshot, SourceSide::Left, "src/a.test.ts"),
            Some(40)
        );
        assert_eq!(
            changed_test_line(&snapshot, SourceSide::Right, "src/a.test.ts"),
            Some(44)
        );
    }

    fn fixture() -> Snapshot {
        let root = tempfile::tempdir().unwrap().keep();
        fs::create_dir_all(root.join("after/src")).unwrap();
        fs::create_dir_all(root.join("before/src")).unwrap();
        fs::write(root.join("after/src/a.ts"), "export const a = 1;\n").unwrap();
        fs::write(root.join("before/src/a.ts"), "export const a = 0;\n").unwrap();
        Snapshot {
            id: crate::model::SnapshotId::new(),
            repository: root.clone(),
            input: crate::snapshot::SnapshotInput::Uncommitted,
            original_base: String::new(),
            original_head: String::new(),
            comparison_base: String::new(),
            before_commit: String::new(),
            after_commit: String::new(),
            captured_at: Utc::now(),
            source_fingerprint: "source".into(),
            files: vec![crate::snapshot::FileChange {
                status: "M".into(),
                old_path: Some(GitPath::from_bytes(b"src/a.ts".to_vec())),
                new_path: Some(GitPath::from_bytes(b"src/a.ts".to_vec())),
                old_mode: String::new(),
                new_mode: String::new(),
                old_object: String::new(),
                new_object: String::new(),
                before_blob: None,
                after_blob: None,
                binary: false,
                submodule: false,
                hunks: Vec::new(),
            }],
            storage_dir: root,
        }
    }
}

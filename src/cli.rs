use std::collections::BTreeSet;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::agent;
use crate::comments::{CommentAnchor, DraftStore, comments_path};
use crate::context::{
    CaptureStatus, ContextAnchor, ContextBundle, ContextMetadata, ContextSourceKind, HookConfig,
};
use crate::doctor;
use crate::error::{AppError, Result};
use crate::explanations::{ExplanationStore, ExplanationUpdate};
use crate::github::{
    GitHubAdapter, GitHubConfig, PullRequestContext, ReviewEvent, intent_path, load_preview,
    preview_path, record_remote_comment_ids, save_preview,
};
use crate::graph::{ChangeGraph, EdgeKind, NodeKind, SourceSide};
use crate::indexer::{self, IndexOptions};
use crate::model::{Envelope, SessionId};
use crate::profiles::{GitHubProfile, ProfileStore};
use crate::progress::{ReviewProgress, ReviewStatus, progress_path};
use crate::ranking::{Assessment, HunkLabelUpdate, RankingFileLock, RankingState, ranking_path};
use crate::review_units::{self, ReviewUnits};
use crate::settings::{AgentProfile, Settings};
use crate::snapshot::{self, SnapshotInput};
use crate::storage::{SessionRecord, Store};
use crate::test_evidence::{self, RunStatus, TestEvidenceStore};

#[derive(Debug, Parser)]
#[command(
    name = "lgr",
    version,
    about = "Rank Git changes by semantic review importance"
)]
pub struct Cli {
    /// Run ranking with an ad-hoc executable or script from ~/.lgr/scripts.
    #[arg(short = 'a', long, global = true, conflicts_with = "profile")]
    pub agent: Option<String>,
    /// Use a named agent profile (or GitHub profile for GitHub commands).
    #[arg(short = 'p', long, global = true, conflicts_with = "agent")]
    pub profile: Option<String>,
    /// Override the user data directory (defaults to the configured ~/.lgr data directory).
    #[arg(long, global = true, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,
    /// Read agent-profile overrides from an explicit JSON file.
    #[arg(long, global = true, value_name = "FILE")]
    pub profile_config: Option<PathBuf>,
    /// Emit versioned diagnostic details to stderr on failure.
    #[arg(long, global = true)]
    pub debug: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect external dependencies without changing the repository.
    Doctor,
    /// Create and inspect persistent review sessions.
    Session(SessionArgs),
    /// Capture immutable Git inputs for review.
    Review(ReviewArgs),
    /// Build, traverse, and rank the semantic change graph.
    Graph(GraphArgs),
    /// Attach bounded Markdown intent and run configured pre-review hooks.
    Context(ContextArgs),
    /// Run configured tests and inspect or import runtime coverage evidence.
    Tests(TestsArgs),
    /// Manage persistent inline and general review drafts.
    Comment(CommentArgs),
    /// Open the lightweight ranked review queue.
    Tui {
        session: Option<String>,
        /// Use the newest indexed session for this repository when SESSION is omitted.
        #[arg(long, default_value = ".")]
        repository: PathBuf,
        #[arg(long)]
        nvim_server: Option<String>,
        #[arg(long, default_value = "nvim")]
        nvim_command: PathBuf,
        /// Disable custom TUI colors.
        #[arg(long)]
        no_color: bool,
    },
    /// Share queue selection and review status with editor integrations.
    Progress(ProgressArgs),
    /// Resolve captured revision and path coordinates for editor bridges.
    Editor(EditorArgs),
    /// Initialize and inspect ~/.lgr/settings.json.
    Config(ConfigArgs),
    /// Launch an external agent with the embedded ranking skill.
    Rank {
        session: String,
        /// Resolve and print the invocation without launching it.
        #[arg(long)]
        dry_run: bool,
        /// Run the agent even when a current finalized ranking exists.
        #[arg(long)]
        force: bool,
        /// Only generate or refresh short semantic hunk titles.
        #[arg(long)]
        titles_only: bool,
    },
    /// Generate, import, or inspect source-backed review explanations.
    Explain(ExplainArgs),
    /// Configure named external agent profiles.
    #[command(name = "agent-profile", visible_alias = "harness")]
    AgentProfile(AgentProfileArgs),
    /// Configure repository-matched GitHub identities without global account switching.
    #[command(name = "github-profile", visible_alias = "profile")]
    GithubProfile(ProfileArgs),
    /// Read GitHub PR context and explicitly preview or submit reviews through a profile.
    Github(GithubArgs),
}

#[derive(Debug, Args)]
pub struct SessionArgs {
    #[command(subcommand)]
    command: SessionCommand,
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    Create {
        #[arg(long, default_value = ".")]
        repository: PathBuf,
    },
    Show {
        id: String,
    },
    /// List recent review sessions for a repository.
    List {
        #[arg(long, default_value = ".")]
        repository: PathBuf,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    SetState {
        id: String,
        #[arg(long)]
        expected_revision: i64,
        #[arg(long)]
        state: String,
    },
}

#[derive(Debug, Args)]
pub struct ReviewArgs {
    #[command(subcommand)]
    command: ReviewCommand,
}

#[derive(Debug, Subcommand)]
enum ReviewCommand {
    Create(ReviewCreate),
    Refresh {
        session: String,
    },
    /// Preview, apply, or roll back deterministic large-hunk partitioning.
    Partition {
        session: String,
        /// Persist the previewed review-unit projection and migrate ranking/progress.
        #[arg(long, conflicts_with = "rollback")]
        apply: bool,
        /// Restore the last projection backup while retaining current source-anchored drafts.
        #[arg(long, conflicts_with = "apply")]
        rollback: bool,
        /// Required current projection revision; use zero when no projection exists.
        #[arg(long, default_value_t = 0)]
        expected_revision: u64,
    },
}

#[derive(Debug, Args)]
pub struct GraphArgs {
    #[command(subcommand)]
    command: GraphCommand,
}

#[derive(Debug, Subcommand)]
enum GraphCommand {
    Build {
        session: String,
        #[arg(long, default_value_t = 180)]
        time_budget: u64,
        #[arg(long, default_value_t = 15)]
        request_timeout: u64,
        #[arg(long, default_value_t = 500)]
        max_files: usize,
        #[arg(long, default_value_t = 2_000)]
        max_symbols: usize,
        #[arg(long)]
        force: bool,
    },
    Expand {
        session: String,
        #[arg(long, default_value_t = 60)]
        time_budget: u64,
        #[arg(long, default_value_t = 500)]
        max_files: usize,
        #[arg(long, default_value_t = 5_000)]
        max_symbols: usize,
    },
    Overview {
        session: String,
    },
    /// Return all active changes, context, and bounded patch evidence for ranking.
    Evidence {
        session: String,
        /// Maximum serialized response bytes, including context and metadata.
        #[arg(long, default_value_t = 131_072)]
        max_bytes: usize,
    },
    Nodes {
        session: String,
        #[arg(required = true)]
        ids: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        fields: Vec<String>,
    },
    Walk {
        session: String,
        #[arg(long, required = true, value_delimiter = ',')]
        seeds: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        edges: Vec<String>,
        #[arg(long, default_value_t = 2)]
        depth: usize,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long)]
        continuation: Option<String>,
    },
    Source {
        session: String,
        #[arg(required = true)]
        nodes: Vec<String>,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 65_536)]
        max_bytes: usize,
    },
    Hunks {
        session: String,
        #[arg(required = true)]
        ids: Vec<String>,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 65_536)]
        max_bytes: usize,
    },
    /// Inspect the active review-unit projection.
    Units {
        session: String,
        /// Return only units owned by this raw parent hunk.
        #[arg(long)]
        parent: Option<String>,
    },
    Score {
        session: String,
        #[arg(long)]
        graph_revision: String,
        #[arg(long, conflicts_with = "updates_file")]
        updates: Option<String>,
        #[arg(long, conflicts_with = "updates")]
        updates_file: Option<PathBuf>,
        /// Return counts instead of the full ranking and queue.
        #[arg(long)]
        compact: bool,
        /// Reject the batch unless every active change is assessed.
        #[arg(long)]
        require_complete: bool,
    },
    /// Persist semantic hunk titles without changing scores.
    Label {
        session: String,
        #[arg(long)]
        graph_revision: String,
        #[arg(long, conflicts_with = "updates_file")]
        updates: Option<String>,
        #[arg(long, conflicts_with = "updates")]
        updates_file: Option<PathBuf>,
    },
    Queue {
        session: String,
    },
    Finalize {
        session: String,
        /// Return counts instead of the full ranking and queue.
        #[arg(long)]
        compact: bool,
        /// Refuse to finalize unless every active change is assessed.
        #[arg(long)]
        require_complete: bool,
    },
}

#[derive(Debug, Args)]
pub struct ContextArgs {
    #[command(subcommand)]
    command: ContextCommand,
}

#[derive(Debug, Args)]
pub struct TestsArgs {
    #[command(subcommand)]
    command: TestsCommand,
}

#[derive(Debug, Subcommand)]
enum TestsCommand {
    /// Execute one configured profile against a disposable captured snapshot side.
    Run {
        session: String,
        /// Name from test_profiles in ~/.lgr/settings.json.
        #[arg(long)]
        profile: String,
        /// Captured source side: left or right.
        #[arg(long, default_value = "right")]
        side: String,
        /// Test selector passed through each {selection} argv placeholder; repeat as needed.
        #[arg(long = "select")]
        selection: Vec<String>,
        /// Resolve command, report path, selection, and cache key without executing.
        #[arg(long)]
        dry_run: bool,
        /// Execute even when an identical completed run is cached.
        #[arg(long)]
        force: bool,
    },
    /// Import a bounded coverage report without executing tests.
    Import {
        session: String,
        report: PathBuf,
        /// manifest-v1, istanbul-json, or lcov.
        #[arg(long)]
        format: String,
        /// Captured source side: left or right.
        #[arg(long, default_value = "right")]
        side: String,
        /// Attribute an Istanbul/LCOV report to exactly this test file.
        #[arg(long)]
        test_file: Option<String>,
        /// Test outcome: passed, failed, cancelled, timed-out, partial, skipped, or error.
        #[arg(long, default_value = "passed")]
        status: String,
        /// Report was produced inside this session's captured side.
        #[arg(long)]
        captured_source_report: bool,
        /// Reject reports larger than this byte count.
        #[arg(long, default_value_t = 64 * 1024 * 1024)]
        max_bytes: usize,
    },
    /// Show stored runs, attribution precision, diagnostics, and current links.
    Show { session: String },
}

#[derive(Debug, Subcommand)]
enum ContextCommand {
    Add {
        session: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        note: Option<String>,
        /// Truncate captured Markdown to this byte count on a UTF-8 boundary.
        #[arg(long, default_value_t = 262_144)]
        max_bytes: usize,
        /// Treat the supplied Markdown file as an explicit Folio export.
        #[arg(long, requires = "file")]
        folio: bool,
        /// Preserve an original URL as provenance without fetching it.
        #[arg(long)]
        source_url: Option<String>,
        /// Preserve the supplied document author as provenance.
        #[arg(long)]
        author: Option<String>,
    },
    Show {
        session: String,
    },
    Hook {
        session: String,
        #[arg(long)]
        executable: PathBuf,
        #[arg(long)]
        argv: Vec<String>,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        #[arg(long, default_value_t = 262_144)]
        max_bytes: usize,
        #[arg(long)]
        continue_without_context: bool,
    },
}

#[derive(Debug, Args)]
pub struct CommentArgs {
    #[command(subcommand)]
    command: CommentCommand,
}

#[derive(Debug, Subcommand)]
enum CommentCommand {
    Add {
        session: String,
        #[arg(long)]
        body: String,
        #[arg(long)]
        node: Option<String>,
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        side: Option<String>,
        #[arg(long)]
        start_line: Option<u32>,
        #[arg(long)]
        end_line: Option<u32>,
        #[arg(long)]
        plugin_id: Option<String>,
    },
    Update {
        session: String,
        id: String,
        #[arg(long)]
        body: String,
        #[arg(long)]
        expected_revision: u64,
    },
    Delete {
        session: String,
        id: String,
    },
    List {
        session: String,
    },
    Export {
        session: String,
        #[arg(long, default_value = "review.md")]
        output: PathBuf,
    },
    Import {
        session: String,
        input: PathBuf,
    },
}

#[derive(Debug, Args)]
pub struct EditorArgs {
    #[command(subcommand)]
    command: EditorCommand,
}

#[derive(Debug, Subcommand)]
enum EditorCommand {
    Target { session: String, node: String },
}

#[derive(Debug, Args)]
pub struct GithubArgs {
    #[arg(long, default_value = "github.com")]
    host: String,
    #[arg(long)]
    repository: String,
    #[command(subcommand)]
    command: GithubCommand,
}

#[derive(Debug, Args)]
pub struct ProfileArgs {
    #[command(subcommand)]
    command: ProfileCommand,
}

#[derive(Debug, Subcommand)]
enum ProfileCommand {
    /// Create or replace a named profile.
    Set {
        name: String,
        #[arg(long = "match", required = true)]
        repository_patterns: Vec<String>,
        #[arg(long)]
        github_command: String,
        #[arg(long = "github-arg", allow_hyphen_values = true)]
        github_args: Vec<String>,
        #[arg(long)]
        expected_account: String,
        /// Canonical API hostname when repository patterns use an SSH host alias.
        #[arg(long)]
        api_host: Option<String>,
    },
    /// List configured profiles.
    List,
    /// Show which profile owns a repository.
    Resolve {
        #[arg(long, default_value = "github.com")]
        host: String,
        #[arg(long)]
        repository: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// Run a GitHub CLI command through the profile matched to a local repository.
    Exec {
        #[arg(long, default_value = ".")]
        repository_dir: PathBuf,
        #[arg(long, default_value = "origin")]
        remote: String,
        /// Resolve and print the command without authentication or execution.
        #[arg(long)]
        dry_run: bool,
        #[arg(last = true, required = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum GithubCommand {
    Fetch {
        number: u64,
    },
    Create {
        number: u64,
        #[arg(long, default_value = ".")]
        local_repository: PathBuf,
    },
    Refresh {
        session: String,
    },
    Preview {
        session: String,
        number: u64,
        #[arg(long, value_delimiter = ',')]
        comments: Vec<String>,
        #[arg(long, default_value = "comment")]
        event: String,
        #[arg(long, default_value = "")]
        summary: String,
    },
    Submit {
        session: String,
    },
    Reconcile {
        session: String,
    },
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Create settings and data/scripts directories if absent.
    Init,
    /// Print effective settings and their path.
    Show,
}

#[derive(Debug, Args)]
pub struct AgentProfileArgs {
    #[command(subcommand)]
    command: AgentProfileCommand,
}

#[derive(Debug, Args)]
pub struct ExplainArgs {
    pub session: String,
    /// Show stored model interpretations and manual notes without launching an agent.
    #[arg(
        long,
        conflicts_with_all = ["dry_run", "force", "graph_revision", "updates", "updates_file", "note", "item"]
    )]
    pub show: bool,
    /// Resolve and print the agent invocation without launching it.
    #[arg(
        long,
        conflicts_with_all = ["graph_revision", "updates", "updates_file", "note", "item"]
    )]
    pub dry_run: bool,
    /// Launch even when current explanations are already cached.
    #[arg(
        long,
        conflicts_with_all = ["graph_revision", "updates", "updates_file", "note", "item"]
    )]
    pub force: bool,
    /// Active graph revision required by an imported atomic update batch.
    #[arg(long)]
    pub graph_revision: Option<String>,
    /// Current explanation-store revision required by a write.
    #[arg(long, default_value_t = 0)]
    pub expected_revision: u64,
    /// Inline JSON array of explanation updates.
    #[arg(
        long,
        conflicts_with_all = ["updates_file", "note", "item", "show", "dry_run", "force"]
    )]
    pub updates: Option<String>,
    /// File containing a JSON array of explanation updates.
    #[arg(
        long,
        conflicts_with_all = ["updates", "note", "item", "show", "dry_run", "force"]
    )]
    pub updates_file: Option<PathBuf>,
    /// Save a reviewer-authored note, kept separate from model interpretation.
    #[arg(
        long,
        requires = "item",
        conflicts_with_all = ["graph_revision", "updates", "updates_file", "show", "dry_run", "force"]
    )]
    pub note: Option<String>,
    /// Review hunk or review-unit receiving --note.
    #[arg(long, requires = "note")]
    pub item: Option<String>,
}

#[derive(Debug, Subcommand)]
enum AgentProfileCommand {
    /// List configured agent profiles and the selected default.
    List,
    /// Select the default agent profile by name.
    Select { name: String },
    /// Create or replace an agent profile command.
    Set {
        name: String,
        #[arg(long)]
        command: String,
        #[arg(long = "arg", allow_hyphen_values = true)]
        args: Vec<String>,
        #[arg(long)]
        select: bool,
    },
    /// Print one profile, or the selected default when NAME is omitted.
    Resolve { name: Option<String> },
}

#[derive(Debug, Args)]
pub struct ProgressArgs {
    #[command(subcommand)]
    command: ProgressCommand,
}

#[derive(Debug, Subcommand)]
enum ProgressCommand {
    Show {
        session: String,
    },
    Select {
        session: String,
        node: String,
        #[arg(long)]
        expected_revision: u64,
    },
    Status {
        session: String,
        node: String,
        #[arg(long)]
        reviewed: bool,
        #[arg(long)]
        expected_revision: u64,
    },
    Next {
        session: String,
        #[arg(long)]
        previous: bool,
        #[arg(long)]
        expected_revision: u64,
    },
}

#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("source").required(true).multiple(false).args(["uncommitted", "staged", "unstaged", "base", "revisions"])))]
struct ReviewCreate {
    /// Git/Diffview-style A..B (direct) or A...B (merge-base) comparison.
    #[arg(value_name = "REVISIONS")]
    revisions: Option<String>,
    #[arg(long, default_value = ".")]
    repository: PathBuf,
    /// Reuse the newest session whose selected Git input has identical content.
    #[arg(long)]
    reuse: bool,
    #[arg(long, requires = "head")]
    base: Option<String>,
    #[arg(long, requires = "base")]
    head: Option<String>,
    #[arg(long, requires = "base")]
    direct: bool,
    /// Review all changes from HEAD through the working tree, including untracked files.
    #[arg(long)]
    uncommitted: bool,
    #[arg(long)]
    staged: bool,
    #[arg(long)]
    unstaged: bool,
    #[arg(long, requires = "unstaged")]
    include_untracked: bool,
}

pub async fn run(cli: Cli) -> Result<Value> {
    match cli.command {
        Command::Doctor => Ok(json!({ "dependencies": doctor::diagnose() })),
        Command::Session(args) => {
            let db = cli
                .data_dir
                .map(|p| p.join("sessions.sqlite3"))
                .map(Ok)
                .unwrap_or_else(Store::default_path)?;
            let mut store = Store::open(db)?;
            match args.command {
                SessionCommand::Create { repository } => {
                    Ok(serde_json::to_value(store.create_session(repository)?)?)
                }
                SessionCommand::Show { id } => Ok(serde_json::to_value(
                    store.get_session(&parse_session_id(id)?)?,
                )?),
                SessionCommand::List { repository, limit } => {
                    if limit == 0 || limit > 100 {
                        return Err(invalid(
                            "invalid_session_limit",
                            "--limit must be between 1 and 100",
                        ));
                    }
                    let sessions = store
                        .sessions_for_repository(repository)?
                        .into_iter()
                        .take(limit)
                        .map(|record| session_summary(&record))
                        .collect::<Vec<_>>();
                    Ok(json!({ "sessions": sessions }))
                }
                SessionCommand::SetState {
                    id,
                    expected_revision,
                    state,
                } => {
                    let state: Value =
                        serde_json::from_str(&state).map_err(|error| AppError::InvalidInput {
                            code: "invalid_state_json",
                            message: format!("--state must be valid JSON: {error}"),
                        })?;
                    Ok(serde_json::to_value(store.update_state(
                        &parse_session_id(id)?,
                        expected_revision,
                        &state,
                    )?)?)
                }
            }
        }
        Command::Review(args) => {
            let db = cli
                .data_dir
                .map(|p| p.join("sessions.sqlite3"))
                .map(Ok)
                .unwrap_or_else(Store::default_path)?;
            let mut store = Store::open(db)?;
            match args.command {
                ReviewCommand::Create(args) => {
                    let input = if args.uncommitted {
                        SnapshotInput::Uncommitted
                    } else if args.staged {
                        SnapshotInput::Staged
                    } else if args.unstaged {
                        SnapshotInput::Unstaged {
                            include_untracked: args.include_untracked,
                        }
                    } else if let Some(revisions) = args.revisions {
                        parse_revision_spec(&revisions)?
                    } else {
                        let base = args.base.expect("clap requires base");
                        let head = args.head.expect("clap requires head");
                        if args.direct {
                            SnapshotInput::Revisions { base, head }
                        } else {
                            SnapshotInput::Branch { base, head }
                        }
                    };
                    if args.reuse
                        && let Some((session, snapshot)) =
                            reusable_session(&store, &args.repository, &input)?
                    {
                        return Ok(json!({
                            "session": session,
                            "snapshot": snapshot,
                            "cache_hit": true,
                        }));
                    }
                    let snapshot = snapshot::capture(&args.repository, input, store.root_dir())?;
                    let session = store.create_session(&args.repository)?;
                    let state = json!({
                        "snapshot_id": snapshot.id,
                        "snapshot_path": snapshot.storage_dir.join("snapshot.json"),
                        "review_units_version": 1,
                    });
                    let session = store.update_state(&session.id, session.revision, &state)?;
                    Ok(json!({ "session": session, "snapshot": snapshot, "cache_hit": false }))
                }
                ReviewCommand::Refresh { session } => {
                    let id = parse_session_id(session)?;
                    let current = store.get_session(&id)?;
                    let path = current
                        .state
                        .get("snapshot_path")
                        .and_then(Value::as_str)
                        .ok_or_else(|| AppError::InvalidInput {
                            code: "session_has_no_snapshot",
                            message: format!("session {id} has no captured snapshot"),
                        })?;
                    let previous = snapshot::load(PathBuf::from(path).as_path())?;
                    let next = snapshot::capture(
                        &current.repository,
                        previous.input.clone(),
                        store.root_dir(),
                    )?;
                    let transferred_hunks = snapshot::transferable_hunks(&previous, &next);
                    let previous_drafts = DraftStore::load(&comments_path(&previous))?;
                    previous_drafts
                        .carry_to_snapshot(&next)?
                        .save(&comments_path(&next))?;
                    let old_progress = ReviewProgress::load(&progress_path(&previous.storage_dir))?;
                    let mut next_progress = old_progress.carry(&transferred_hunks);
                    next_progress.save_checked(&progress_path(&next.storage_dir), 0)?;
                    let state = json!({
                        "snapshot_id": next.id,
                        "snapshot_path": next.storage_dir.join("snapshot.json"),
                        "refresh_from": previous.id,
                        "transferred_hunks": transferred_hunks,
                        "review_units_version": 1,
                    });
                    let session = store.update_state(&id, current.revision, &state)?;
                    Ok(
                        json!({ "session": session, "snapshot": next, "transferred_hunks": transferred_hunks }),
                    )
                }
                ReviewCommand::Partition {
                    session,
                    apply,
                    rollback,
                    expected_revision,
                } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let raw_graph = ChangeGraph::load(&indexer::graph_path(&snapshot))?;
                    if rollback {
                        let restored = review_units::rollback(&snapshot, expected_revision)?;
                        restore_partition_state(
                            &snapshot,
                            expected_revision.saturating_sub(1),
                            false,
                        )?;
                        return Ok(json!({
                            "rolled_back": true,
                            "projection": review_unit_projection_summary(&restored),
                        }));
                    }
                    let settings = Settings::load(&Settings::default_path()?)?;
                    let preview =
                        review_units::preview(&snapshot, &raw_graph, &settings.review_units);
                    if apply {
                        review_units::check_revision(&snapshot, expected_revision)?;
                        backup_partition_state(&snapshot, expected_revision)?;
                        let result = (|| -> Result<()> {
                            review_units::save_checked(
                                &snapshot,
                                preview.review_units.clone(),
                                expected_revision,
                            )?;
                            migrate_partition_ranking(
                                &snapshot,
                                &raw_graph,
                                &preview.review_units,
                            )?;
                            migrate_partition_progress(&snapshot, &preview.review_units)
                        })();
                        if let Err(error) = result {
                            restore_partition_state(&snapshot, expected_revision, true)?;
                            return Err(error);
                        }
                    }
                    Ok(json!({
                        "applied": apply,
                        "preview": partition_preview_summary(&preview),
                    }))
                }
            }
        }
        Command::Graph(args) => {
            let db = cli
                .data_dir
                .map(|p| p.join("sessions.sqlite3"))
                .map(Ok)
                .unwrap_or_else(Store::default_path)?;
            let store = Store::open(db)?;
            match args.command {
                GraphCommand::Build {
                    session,
                    time_budget,
                    request_timeout,
                    max_files,
                    max_symbols,
                    force,
                } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let previous_revision = ChangeGraph::load(&indexer::graph_path(&snapshot))
                        .ok()
                        .map(|graph| graph.revision);
                    let graph = indexer::build(
                        &snapshot,
                        &IndexOptions {
                            time_budget_seconds: time_budget,
                            request_timeout_seconds: request_timeout,
                            max_files,
                            max_symbols,
                            force,
                            ..Default::default()
                        },
                    )
                    .await?;
                    if record
                        .state
                        .get("review_units_version")
                        .and_then(Value::as_u64)
                        == Some(1)
                    {
                        let settings = Settings::load(&Settings::default_path()?)?;
                        ensure_review_units(&snapshot, &graph, &settings.review_units)?;
                    }
                    let active_graph = review_units::project_graph(
                        graph.clone(),
                        review_units::load(&snapshot)?.as_ref(),
                    )?;
                    Ok(json!({
                        "graph_revision": active_graph.revision,
                        "graph_path": indexer::graph_path(&snapshot),
                        "nodes": active_graph.nodes.len(),
                        "edges": active_graph.edges.len(),
                        "coverage": graph.coverage,
                        "unfinished_frontier": graph.unfinished_frontier,
                        "cache_hit": !force
                            && previous_revision.as_ref() == Some(&graph.revision),
                    }))
                }
                GraphCommand::Expand {
                    session,
                    time_budget,
                    max_files,
                    max_symbols,
                } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let previous = ChangeGraph::load(&indexer::graph_path(&snapshot)).ok();
                    let graph = indexer::build(
                        &snapshot,
                        &IndexOptions {
                            time_budget_seconds: time_budget,
                            max_files,
                            max_symbols,
                            force: true,
                            ..Default::default()
                        },
                    )
                    .await?;
                    if record
                        .state
                        .get("review_units_version")
                        .and_then(Value::as_u64)
                        == Some(1)
                    {
                        let settings = Settings::load(&Settings::default_path()?)?;
                        ensure_review_units(&snapshot, &graph, &settings.review_units)?;
                    }
                    let active_graph = review_units::project_graph(
                        graph.clone(),
                        review_units::load(&snapshot)?.as_ref(),
                    )?;
                    Ok(json!({
                        "previous_graph_revision": previous.map(|value| value.revision),
                        "graph_revision": active_graph.revision,
                        "coverage": graph.coverage,
                        "unfinished_frontier": graph.unfinished_frontier,
                    }))
                }
                GraphCommand::Overview { session } => {
                    let started = std::time::Instant::now();
                    let (snapshot, graph) = load_graph(&store, session)?;
                    let mut kinds = serde_json::Map::new();
                    for kind in [
                        NodeKind::Hunk,
                        NodeKind::ReviewUnit,
                        NodeKind::Symbol,
                        NodeKind::FileChange,
                        NodeKind::Reference,
                        NodeKind::Test,
                    ] {
                        kinds.insert(
                            format!("{kind:?}").to_ascii_lowercase(),
                            json!(
                                graph
                                    .nodes
                                    .values()
                                    .filter(|node| node.kind == kind)
                                    .count()
                            ),
                        );
                    }
                    let response = json!({
                        "graph_revision": graph.revision,
                        "snapshot_id": graph.snapshot_id,
                        "node_counts": kinds,
                        "edge_count": graph.edges.len(),
                        "coverage": graph.coverage,
                        "unfinished_frontier": graph.unfinished_frontier,
                    });
                    record_query_metrics(&snapshot, &graph, &response, 0, started.elapsed())?;
                    Ok(response)
                }
                GraphCommand::Evidence { session, max_bytes } => {
                    let started = std::time::Instant::now();
                    let (snapshot, graph) = load_graph(&store, session)?;
                    let response = bounded_ranking_evidence(&snapshot, &graph, max_bytes)?;
                    let source_bytes = response_content_bytes(&response);
                    record_query_metrics(
                        &snapshot,
                        &graph,
                        &response,
                        source_bytes,
                        started.elapsed(),
                    )?;
                    Ok(response)
                }
                GraphCommand::Nodes {
                    session,
                    ids,
                    fields,
                } => {
                    let started = std::time::Instant::now();
                    let (snapshot, graph) = load_graph(&store, session)?;
                    let results: Vec<Value> = ids
                        .into_iter()
                        .map(|id| match graph.nodes.get(&id) {
                            Some(node) => json!({ "id": id, "node": project_node(node, &fields) }),
                            None => json!({
                                "id": id,
                                "error": { "code": "unknown_node", "message": "node does not exist" }
                            }),
                        })
                        .collect();
                    let response = json!({ "graph_revision": graph.revision, "items": results });
                    record_query_metrics(&snapshot, &graph, &response, 0, started.elapsed())?;
                    Ok(response)
                }
                GraphCommand::Walk {
                    session,
                    seeds,
                    edges,
                    depth,
                    limit,
                    continuation,
                } => {
                    if depth > 16 || limit > 10_000 {
                        return Err(invalid(
                            "graph_limit_exceeded",
                            "depth max is 16 and node limit max is 10000",
                        ));
                    }
                    let started = std::time::Instant::now();
                    let (snapshot, graph) = load_graph(&store, session)?;
                    for seed in &seeds {
                        if !graph.nodes.contains_key(seed) {
                            return Err(invalid(
                                "unknown_seed",
                                format!("node {seed} does not exist"),
                            ));
                        }
                    }
                    let kinds = edges
                        .into_iter()
                        .map(|value| parse_edge_kind(&value))
                        .collect::<Result<BTreeSet<_>>>()?;
                    let query_key = walk_query_key(&seeds, &kinds, depth);
                    let offset = continuation
                        .as_deref()
                        .map(|value| {
                            decode_continuation(value, graph.revision.as_str(), &query_key)
                        })
                        .transpose()?
                        .unwrap_or(0);
                    let all = graph.walk(&seeds, &kinds, depth, 10_001);
                    if all.len() > 10_000 {
                        return Err(invalid(
                            "graph_global_limit",
                            "walk exceeds global 10000 node limit; narrow edge filters or depth",
                        ));
                    }
                    let ids: Vec<_> = all.iter().skip(offset).take(limit).cloned().collect();
                    let next_offset = offset + ids.len();
                    let has_more = next_offset < all.len();
                    let response = json!({
                        "graph_revision": graph.revision,
                        "nodes": ids.iter().filter_map(|id| graph.nodes.get(id)).collect::<Vec<_>>(),
                        "has_more": has_more,
                        "omitted_reason": has_more.then_some("node_limit"),
                        "continuation": has_more.then(|| encode_continuation(graph.revision.as_str(), &query_key, next_offset)),
                    });
                    record_query_metrics(&snapshot, &graph, &response, 0, started.elapsed())?;
                    Ok(response)
                }
                GraphCommand::Source {
                    session,
                    nodes,
                    offset,
                    max_bytes,
                } => {
                    let started = std::time::Instant::now();
                    if max_bytes == 0 || max_bytes > 1_048_576 {
                        return Err(invalid(
                            "invalid_source_limit",
                            "max-bytes must be 1..1048576",
                        ));
                    }
                    let (snapshot, graph) = load_graph(&store, session)?;
                    let items: Vec<Value> = nodes
                        .into_iter()
                        .map(|node| source_item(&snapshot, &graph, &node, offset, max_bytes))
                        .collect();
                    let response = json!({
                        "graph_revision": graph.revision,
                        "items": items,
                    });
                    let source_bytes = response_content_bytes(&response);
                    record_query_metrics(
                        &snapshot,
                        &graph,
                        &response,
                        source_bytes,
                        started.elapsed(),
                    )?;
                    Ok(response)
                }
                GraphCommand::Hunks {
                    session,
                    ids,
                    offset,
                    max_bytes,
                } => {
                    let started = std::time::Instant::now();
                    if max_bytes == 0 || max_bytes > 1_048_576 {
                        return Err(invalid(
                            "invalid_hunk_limit",
                            "max-bytes must be 1..1048576",
                        ));
                    }
                    let (snapshot, graph) = load_graph(&store, session)?;
                    let projection = review_units::load(&snapshot)?;
                    let items: Vec<Value> = ids
                        .into_iter()
                        .map(|id| hunk_item(&snapshot, projection.as_ref(), &id, offset, max_bytes))
                        .collect();
                    let response = json!({ "graph_revision": graph.revision, "items": items });
                    let source_bytes = response_content_bytes(&response);
                    record_query_metrics(
                        &snapshot,
                        &graph,
                        &response,
                        source_bytes,
                        started.elapsed(),
                    )?;
                    Ok(response)
                }
                GraphCommand::Units { session, parent } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let projection = review_units::load(&snapshot)?;
                    let items: Vec<_> = projection
                        .as_ref()
                        .map(|projection| {
                            projection
                                .units
                                .iter()
                                .filter(|unit| {
                                    parent
                                        .as_ref()
                                        .is_none_or(|parent| &unit.parent_hunk_id == parent)
                                })
                                .map(review_unit_summary)
                                .collect()
                        })
                        .unwrap_or_default();
                    Ok(json!({
                        "active": projection.as_ref().is_some_and(|projection| projection.active),
                        "projection_revision": projection.as_ref().map(|projection| projection.projection_revision),
                        "items": items,
                    }))
                }
                GraphCommand::Score {
                    session,
                    graph_revision,
                    updates,
                    updates_file,
                    compact,
                    require_complete,
                } => {
                    let (snapshot, graph) = load_graph(&store, session)?;
                    let raw_updates = match (updates, updates_file) {
                        (Some(value), None) => value,
                        (None, Some(path)) => fs::read_to_string(path)?,
                        _ => {
                            return Err(invalid(
                                "score_batch_required",
                                "provide exactly one of --updates or --updates-file",
                            ));
                        }
                    };
                    let updates: Vec<Assessment> =
                        serde_json::from_str(&raw_updates).map_err(|error| {
                            invalid(
                                "invalid_score_batch",
                                format!("--updates must be a JSON array: {error}"),
                            )
                        })?;
                    let path = ranking_path(&indexer::graph_path(&snapshot));
                    let _lock = RankingFileLock::acquire(&path)?;
                    let context_digest = current_context_digest(&snapshot)?;
                    let mut ranking =
                        RankingState::load_or_new(&path, &graph, context_digest.clone())?;
                    let submitted = updates.len();
                    ranking.apply_batch(&graph, &graph_revision, updates)?;
                    let queue = ranking.queue(&graph, context_digest.as_deref());
                    if require_complete && !queue.fully_ranked {
                        return Err(invalid(
                            "incomplete_ranking",
                            format!(
                                "score batch assesses {} of {} active changes",
                                queue.assessed_changes, queue.total_changes
                            ),
                        ));
                    }
                    ranking.save(&path)?;
                    Ok(if compact {
                        ranking_ack(&queue, Some(submitted))
                    } else {
                        json!({ "ranking": ranking, "queue": queue })
                    })
                }
                GraphCommand::Label {
                    session,
                    graph_revision,
                    updates,
                    updates_file,
                } => {
                    let (snapshot, graph) = load_graph(&store, session)?;
                    let raw_updates = match (updates, updates_file) {
                        (Some(value), None) => value,
                        (None, Some(path)) => fs::read_to_string(path)?,
                        _ => {
                            return Err(invalid(
                                "label_batch_required",
                                "provide exactly one of --updates or --updates-file",
                            ));
                        }
                    };
                    let updates: Vec<HunkLabelUpdate> = serde_json::from_str(&raw_updates)
                        .map_err(|error| {
                            invalid(
                                "invalid_label_batch",
                                format!("--updates must be a JSON array: {error}"),
                            )
                        })?;
                    let path = ranking_path(&indexer::graph_path(&snapshot));
                    let _lock = RankingFileLock::acquire(&path)?;
                    let context_digest = current_context_digest(&snapshot)?;
                    let mut ranking =
                        RankingState::load_or_new(&path, &graph, context_digest.clone())?;
                    ranking.apply_label_batch(&graph, &graph_revision, updates)?;
                    ranking.save(&path)?;
                    Ok(json!({
                        "ranking": ranking,
                        "queue": ranking.queue(&graph, context_digest.as_deref())
                    }))
                }
                GraphCommand::Queue { session } => {
                    let (snapshot, graph) = load_graph(&store, session)?;
                    let context_digest = current_context_digest(&snapshot)?;
                    let ranking = RankingState::load_or_new(
                        &ranking_path(&indexer::graph_path(&snapshot)),
                        &graph,
                        context_digest.clone(),
                    )?;
                    Ok(serde_json::to_value(
                        ranking.queue(&graph, context_digest.as_deref()),
                    )?)
                }
                GraphCommand::Finalize {
                    session,
                    compact,
                    require_complete,
                } => {
                    let (snapshot, graph) = load_graph(&store, session)?;
                    let path = ranking_path(&indexer::graph_path(&snapshot));
                    let _lock = RankingFileLock::acquire(&path)?;
                    let context_digest = current_context_digest(&snapshot)?;
                    let mut ranking =
                        RankingState::load_or_new(&path, &graph, context_digest.clone())?;
                    if ranking.context_digest != context_digest {
                        return Err(invalid(
                            "stale_context_digest",
                            "review context changed; rerank before finalizing",
                        ));
                    }
                    let queue = ranking.queue(&graph, context_digest.as_deref());
                    if require_complete && !queue.fully_ranked {
                        return Err(invalid(
                            "incomplete_ranking",
                            format!(
                                "ranking assesses {} of {} active changes",
                                queue.assessed_changes, queue.total_changes
                            ),
                        ));
                    }
                    let queue = ranking.finalize(&graph)?;
                    ranking.save(&path)?;
                    Ok(if compact {
                        ranking_ack(&queue, None)
                    } else {
                        json!({ "ranking": ranking, "queue": queue })
                    })
                }
            }
        }
        Command::Context(args) => {
            let db = cli
                .data_dir
                .map(|p| p.join("sessions.sqlite3"))
                .map(Ok)
                .unwrap_or_else(Store::default_path)?;
            let store = Store::open(db)?;
            match args.command {
                ContextCommand::Add {
                    session,
                    file,
                    note,
                    max_bytes,
                    folio,
                    source_url,
                    author,
                } => {
                    if file.is_none() == note.is_none() {
                        return Err(invalid(
                            "context_source_required",
                            "provide exactly one of --file or --note",
                        ));
                    }
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let path = context_path(&snapshot);
                    let mut bundle = ContextBundle::load(&path)?;
                    if let Some(file) = file {
                        let markdown = fs::read_to_string(&file)?;
                        let origin = format!("file:{}", file.display());
                        bundle.add_markdown_with(
                            origin.clone(),
                            markdown,
                            max_bytes,
                            ContextMetadata {
                                source_key: origin,
                                kind: if folio {
                                    ContextSourceKind::Folio
                                } else {
                                    ContextSourceKind::Markdown
                                },
                                external_url: source_url,
                                author,
                                ..ContextMetadata::default()
                            },
                        );
                    } else if let Some(note) = note {
                        bundle.add_markdown_with(
                            "inline",
                            note,
                            max_bytes,
                            ContextMetadata {
                                source_key: "inline".into(),
                                external_url: source_url,
                                author,
                                ..ContextMetadata::default()
                            },
                        );
                    }
                    bundle.save(&path)?;
                    Ok(serde_json::to_value(bundle)?)
                }
                ContextCommand::Show { session } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    Ok(serde_json::to_value(ContextBundle::load(&context_path(
                        &snapshot,
                    ))?)?)
                }
                ContextCommand::Hook {
                    session,
                    executable,
                    argv,
                    timeout,
                    max_bytes,
                    continue_without_context,
                } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let path = context_path(&snapshot);
                    let mut bundle = ContextBundle::load(&path)?;
                    let result = bundle
                        .run_hook(
                            &HookConfig {
                                executable,
                                argv: argv.into_iter().map(Into::into).collect(),
                                timeout_seconds: timeout,
                                max_output_bytes: max_bytes,
                            },
                            &json!({
                                "session_id": record.id,
                                "snapshot_id": snapshot.id,
                                "repository": snapshot.repository,
                                "base": snapshot.original_base,
                                "head": snapshot.original_head,
                            }),
                            continue_without_context,
                        )
                        .await;
                    bundle.save(&path)?;
                    result?;
                    Ok(serde_json::to_value(bundle)?)
                }
            }
        }
        Command::Tests(args) => {
            let settings = Settings::load(&Settings::default_path()?)?;
            let data_dir = cli
                .data_dir
                .clone()
                .unwrap_or_else(|| settings.data_dir.clone());
            let store = Store::open(data_dir.join("sessions.sqlite3"))?;
            match args.command {
                TestsCommand::Show { session } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    Ok(serde_json::to_value(TestEvidenceStore::load(&snapshot)?)?)
                }
                TestsCommand::Import {
                    session,
                    report,
                    format,
                    side,
                    test_file,
                    status,
                    captured_source_report,
                    max_bytes,
                } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let side = parse_source_side(&side)?;
                    let status = parse_run_status(&status)?;
                    let run = test_evidence::import_external(
                        &snapshot,
                        side,
                        &format,
                        &report,
                        max_bytes.min(64 * 1024 * 1024),
                        test_file,
                        status,
                        captured_source_report,
                    )?;
                    let mut evidence = TestEvidenceStore::load(&snapshot)?;
                    evidence.runs.push(run);
                    evidence.save(&snapshot)?;
                    Ok(serde_json::to_value(evidence)?)
                }
                TestsCommand::Run {
                    session,
                    profile,
                    side,
                    selection,
                    dry_run,
                    force,
                } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let side = parse_source_side(&side)?;
                    let profile_settings =
                        settings.test_profiles.get(&profile).ok_or_else(|| {
                            invalid(
                                "unknown_test_profile",
                                format!("test profile {profile:?} is not configured"),
                            )
                        })?;
                    let cache_key = test_evidence::cache_key(
                        &snapshot,
                        side,
                        &profile,
                        profile_settings,
                        &selection,
                    )?;
                    let evidence = TestEvidenceStore::load(&snapshot)?;
                    if !force && let Some(cached) = evidence.completed_cache(&cache_key) {
                        return Ok(json!({ "launched": false, "cache_hit": true, "run": cached }));
                    }
                    if dry_run {
                        let (report, command) =
                            test_evidence::dry_run(&snapshot, side, profile_settings, &selection)?;
                        return Ok(json!({
                            "launched": false,
                            "cache_hit": false,
                            "side": side,
                            "profile": profile,
                            "command": command,
                            "report": report,
                            "timeout_seconds": profile_settings.timeout_seconds,
                            "max_output_bytes": profile_settings.max_output_bytes,
                            "max_report_bytes": profile_settings.max_report_bytes,
                        }));
                    }
                    let run =
                        test_evidence::run(&snapshot, side, &profile, profile_settings, selection)
                            .await?;
                    let mut evidence = TestEvidenceStore::load(&snapshot)?;
                    evidence.runs.push(run.clone());
                    evidence.save(&snapshot)?;
                    Ok(json!({ "launched": true, "cache_hit": false, "run": run }))
                }
            }
        }
        Command::Comment(args) => {
            let db = cli
                .data_dir
                .map(|p| p.join("sessions.sqlite3"))
                .map(Ok)
                .unwrap_or_else(Store::default_path)?;
            let store = Store::open(db)?;
            match args.command {
                CommentCommand::Add {
                    session,
                    body,
                    node,
                    path: explicit_path,
                    side: explicit_side,
                    start_line,
                    end_line,
                    plugin_id,
                } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let anchor = match node {
                        Some(node) => {
                            let raw_graph = ChangeGraph::load(&indexer::graph_path(&snapshot))?;
                            let projection = review_units::load(&snapshot)?;
                            let graph =
                                review_units::project_graph(raw_graph, projection.as_ref())?;
                            let location = graph
                                .nodes
                                .get(&node)
                                .and_then(|node| node.preferred_review_location())
                                .ok_or_else(|| {
                                    invalid("invalid_comment_node", "node has no source location")
                                })?;
                            let side = match location.side {
                                SourceSide::Left => "before",
                                SourceSide::Right => "after",
                            };
                            let source = fs::read(
                                snapshot
                                    .storage_dir
                                    .join(side)
                                    .join(location.path.to_path_buf()?),
                            )?;
                            Some(CommentAnchor {
                                snapshot_id: snapshot.id.to_string(),
                                side: location.side,
                                path: location.path.clone(),
                                start_line: location.range.start.line + 1,
                                end_line: location.range.end.line.max(location.range.start.line)
                                    + 1,
                                source_fingerprint: hex::encode(Sha256::digest(source)),
                            })
                        }
                        None if explicit_path.is_some()
                            || explicit_side.is_some()
                            || start_line.is_some()
                            || end_line.is_some() =>
                        {
                            let explicit_path = explicit_path.ok_or_else(|| invalid("incomplete_comment_anchor", "--path, --side, --start-line, and --end-line are required together"))?;
                            let explicit_side = match explicit_side.as_deref() {
                                Some("left") => SourceSide::Left,
                                Some("right") => SourceSide::Right,
                                _ => {
                                    return Err(invalid(
                                        "invalid_comment_side",
                                        "--side must be left or right",
                                    ));
                                }
                            };
                            let start_line = start_line.ok_or_else(|| {
                                invalid("incomplete_comment_anchor", "--start-line is required")
                            })?;
                            let end_line = end_line.ok_or_else(|| {
                                invalid("incomplete_comment_anchor", "--end-line is required")
                            })?;
                            if start_line == 0 || end_line < start_line {
                                return Err(invalid(
                                    "invalid_comment_range",
                                    "comment lines are 1-based and end must not precede start",
                                ));
                            }
                            let git_path = snapshot::GitPath::from_bytes(
                                explicit_path.as_os_str().as_bytes().to_vec(),
                            );
                            let source_side = match explicit_side {
                                SourceSide::Left => "before",
                                SourceSide::Right => "after",
                            };
                            let source = fs::read(
                                snapshot.storage_dir.join(source_side).join(&explicit_path),
                            )?;
                            Some(CommentAnchor {
                                snapshot_id: snapshot.id.to_string(),
                                side: explicit_side,
                                path: git_path,
                                start_line,
                                end_line,
                                source_fingerprint: hex::encode(Sha256::digest(source)),
                            })
                        }
                        None => None,
                    };
                    let path = comments_path(&snapshot);
                    let mut drafts = DraftStore::load(&path)?;
                    let id = drafts.add(body, anchor)?;
                    if let Some(plugin_id) = plugin_id {
                        drafts.set_plugin_id(id.as_str(), plugin_id)?;
                    }
                    drafts.save(&path)?;
                    Ok(json!({ "comment_id": id, "drafts": drafts }))
                }
                CommentCommand::Update {
                    session,
                    id,
                    body,
                    expected_revision,
                } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let path = comments_path(&snapshot);
                    let mut drafts = DraftStore::load(&path)?;
                    drafts.update(&id, body, expected_revision)?;
                    drafts.save(&path)?;
                    Ok(serde_json::to_value(drafts)?)
                }
                CommentCommand::Delete { session, id } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let path = comments_path(&snapshot);
                    let mut drafts = DraftStore::load(&path)?;
                    drafts.delete(&id)?;
                    drafts.save(&path)?;
                    Ok(serde_json::to_value(drafts)?)
                }
                CommentCommand::List { session } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    Ok(serde_json::to_value(DraftStore::load(&comments_path(
                        &snapshot,
                    ))?)?)
                }
                CommentCommand::Export { session, output } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let drafts = DraftStore::load(&comments_path(&snapshot))?;
                    let sidecar = drafts.export_markdown(&snapshot, &output)?;
                    Ok(json!({ "markdown": output, "anchor_metadata": sidecar }))
                }
                CommentCommand::Import { session, input } => {
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let path = comments_path(&snapshot);
                    let mut drafts = DraftStore::load(&path)?;
                    drafts.import_markdown(&snapshot, &input)?;
                    drafts.save(&path)?;
                    Ok(serde_json::to_value(drafts)?)
                }
            }
        }
        Command::Tui {
            session,
            repository,
            nvim_server,
            nvim_command,
            no_color,
        } => {
            let settings = Settings::load(&Settings::default_path()?)?;
            let db = cli
                .data_dir
                .unwrap_or_else(|| settings.data_dir.clone())
                .join("sessions.sqlite3");
            let store = Store::open(db)?;
            let session = match session {
                Some(session) => session,
                None => newest_indexed_session(&store, &repository)?.to_string(),
            };
            let session_for_editor = session.clone();
            let (snapshot, graph) = load_graph(&store, session)?;
            let context_digest = current_context_digest(&snapshot)?;
            let ranking = RankingState::load_or_new(
                &ranking_path(&indexer::graph_path(&snapshot)),
                &graph,
                context_digest.clone(),
            )?;
            let path = progress_path(&snapshot.storage_dir);
            let manual_scores = crate::tui::run(
                ranking.queue(&graph, context_digest.as_deref()),
                &graph,
                &snapshot,
                ReviewProgress::load(&path)?,
                crate::tui::RunOptions {
                    progress_path: &path,
                    theme: settings.tui,
                    no_color: no_color
                        || std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()),
                },
                |action| match action {
                    crate::tui::Action::Open(node) | crate::tui::Action::Comment(node) => {
                        open_neovim(
                            &nvim_command,
                            nvim_server.as_deref(),
                            &session_for_editor,
                            node,
                        )
                    }
                    crate::tui::Action::Export => {
                        let drafts = DraftStore::load(&comments_path(&snapshot))?;
                        drafts.export_markdown(&snapshot, Path::new("review.md"))?;
                        Ok(())
                    }
                },
            )?;
            if !manual_scores.is_empty() {
                let ranking_path = ranking_path(&indexer::graph_path(&snapshot));
                let _lock = RankingFileLock::acquire(&ranking_path)?;
                let mut ranking =
                    RankingState::load_or_new(&ranking_path, &graph, context_digest.clone())?;
                let updates = manual_scores
                    .into_iter()
                    .map(|(node_id, score)| Assessment {
                        evidence_ids: vec![node_id.clone()],
                        node_id,
                        title: None,
                        score,
                        tags: vec!["manual".into()],
                        rationale: "Manual priority set in TUI".into(),
                        confidence: 1.0,
                        authority: crate::ranking::Authority::Manual,
                    })
                    .collect();
                ranking.apply_batch(&graph, graph.revision.as_str(), updates)?;
                ranking.save(&ranking_path)?;
            }
            Ok(json!({ "progress": ReviewProgress::load(&path)? }))
        }
        Command::Progress(args) => {
            let db = cli
                .data_dir
                .map(|p| p.join("sessions.sqlite3"))
                .map(Ok)
                .unwrap_or_else(Store::default_path)?;
            let store = Store::open(db)?;
            let (snapshot, graph) = load_graph(&store, progress_session(&args.command).to_owned())?;
            let path = progress_path(&snapshot.storage_dir);
            let mut progress = ReviewProgress::load(&path)?;
            match args.command {
                ProgressCommand::Show { .. } => {}
                ProgressCommand::Select {
                    node,
                    expected_revision,
                    ..
                } => {
                    if !graph.nodes.contains_key(&node) {
                        return Err(invalid(
                            "unknown_node",
                            format!("node {node} does not exist"),
                        ));
                    }
                    progress.selected = Some(node);
                    progress.save_checked(&path, expected_revision)?;
                }
                ProgressCommand::Status {
                    node,
                    reviewed,
                    expected_revision,
                    ..
                } => {
                    if !graph.nodes.contains_key(&node) {
                        return Err(invalid(
                            "unknown_node",
                            format!("node {node} does not exist"),
                        ));
                    }
                    progress.set_status_in_graph(
                        &graph,
                        &node,
                        if reviewed {
                            ReviewStatus::Reviewed
                        } else {
                            ReviewStatus::Unreviewed
                        },
                    );
                    progress.save_checked(&path, expected_revision)?;
                }
                ProgressCommand::Next {
                    previous,
                    expected_revision,
                    ..
                } => {
                    let context_digest = current_context_digest(&snapshot)?;
                    let ranking = RankingState::load_or_new(
                        &ranking_path(&indexer::graph_path(&snapshot)),
                        &graph,
                        context_digest.clone(),
                    )?;
                    let queue = ranking.queue(&graph, context_digest.as_deref());
                    let current = progress
                        .selected
                        .as_ref()
                        .and_then(|id| queue.items.iter().position(|item| &item.node_id == id))
                        .unwrap_or(0);
                    let next = if previous {
                        current.saturating_sub(1)
                    } else {
                        (current + 1).min(queue.items.len().saturating_sub(1))
                    };
                    progress.selected = queue.items.get(next).map(|item| item.node_id.clone());
                    progress.save_checked(&path, expected_revision)?;
                }
            }
            let derived_parent_statuses = graph
                .nodes
                .values()
                .filter(|node| node.kind == NodeKind::Hunk)
                .filter(|node| {
                    graph.edges.iter().any(|edge| {
                        edge.from == node.id
                            && edge.kind == EdgeKind::Contains
                            && graph
                                .nodes
                                .get(&edge.to)
                                .is_some_and(|child| child.kind == NodeKind::ReviewUnit)
                    })
                })
                .map(|node| (node.id.clone(), progress.status_in_graph(&graph, &node.id)))
                .collect::<std::collections::BTreeMap<_, _>>();
            Ok(json!({
                "progress": progress,
                "derived_parent_statuses": derived_parent_statuses,
            }))
        }
        Command::Editor(args) => {
            let db = cli
                .data_dir
                .map(|p| p.join("sessions.sqlite3"))
                .map(Ok)
                .unwrap_or_else(Store::default_path)?;
            let store = Store::open(db)?;
            match args.command {
                EditorCommand::Target { session, node } => {
                    let (snapshot, graph) = load_graph(&store, session)?;
                    let graph_node = graph.nodes.get(&node).ok_or_else(|| {
                        invalid("unknown_node", format!("node {node} does not exist"))
                    })?;
                    let location = graph_node.preferred_review_location().ok_or_else(|| {
                        invalid("node_has_no_source", format!("node {node} has no source"))
                    })?;
                    let side = match location.side {
                        SourceSide::Left => "left",
                        SourceSide::Right => "right",
                    };
                    Ok(json!({
                        "node_id": node,
                        "snapshot_id": snapshot.id,
                        "repository": snapshot.storage_dir.join("workspace"),
                        "before_commit": snapshot.before_commit,
                        "after_commit": snapshot.after_commit,
                        "path": location.path,
                        "side": side,
                        "line": location.range.start.line + 1,
                        "left_file": snapshot.storage_dir.join("before").join(location.path.to_path_buf()?),
                        "right_file": snapshot.storage_dir.join("after").join(location.path.to_path_buf()?),
                    }))
                }
            }
        }
        Command::Config(args) => {
            let path = Settings::default_path()?;
            let settings = match args.command {
                ConfigCommand::Init => Settings::initialize(&path)?,
                ConfigCommand::Show => Settings::load(&path)?,
            };
            Ok(json!({ "path": path, "settings": settings }))
        }
        Command::Rank {
            session,
            dry_run,
            force,
            titles_only,
        } => {
            let settings = Settings::load(&Settings::default_path()?)?;
            let data_dir = cli
                .data_dir
                .clone()
                .unwrap_or_else(|| settings.data_dir.clone());
            let db = data_dir.join("sessions.sqlite3");
            let store = Store::open(db)?;
            let session_record = store.get_session(&parse_session_id(session.clone())?)?;
            let cache_hit = if titles_only {
                current_ranking_has_all_titles(&session_record)?
            } else {
                current_ranking_is_finalized(&session_record)?
            };
            if !dry_run && !force && cache_hit {
                return Ok(json!({
                    "launched": false,
                    "cache_hit": true,
                    "invocation": null,
                }));
            }
            let ranking_evidence = if !dry_run && !titles_only {
                prepare_ranking_for_launch(&session_record)?;
                let (snapshot, graph) = load_graph(&store, session.clone())?;
                Some(bounded_ranking_evidence(&snapshot, &graph, 131_072)?)
            } else {
                None
            };
            let invocation = agent::prepare(
                &settings,
                cli.agent.as_deref(),
                cli.profile.as_deref(),
                &session,
                &session_record.repository,
                &data_dir,
                if titles_only {
                    agent::Mode::TitlesOnly { refresh: force }
                } else {
                    agent::Mode::Ranking {
                        evidence: ranking_evidence.as_ref(),
                    }
                },
            )?;
            let mut ranking_result = None;
            if !dry_run && titles_only {
                agent::launch(&invocation, &session, &data_dir)?;
            } else if !dry_run {
                let output = agent::launch_once(&invocation, &session, &data_dir)?;
                let updates = agent::parse_ranking_output(&output)?;
                let evidence = ranking_evidence.as_ref().expect("ranking evidence exists");
                let expected_revision = evidence["graph_revision"]
                    .as_str()
                    .expect("ranking evidence has a graph revision");
                let expected_context_digest = evidence["context_digest"].as_str();
                let expected_ids: BTreeSet<_> = evidence["items"]
                    .as_array()
                    .expect("ranking evidence has items")
                    .iter()
                    .filter_map(|item| item["node_id"].as_str().map(str::to_owned))
                    .collect();
                let submitted_ids: BTreeSet<_> = updates
                    .iter()
                    .map(|update| update.node_id.clone())
                    .collect();
                if updates.len() != expected_ids.len() || submitted_ids != expected_ids {
                    return Err(invalid(
                        "incomplete_agent_ranking",
                        "agent must assess every evidence item exactly once",
                    ));
                }
                let (snapshot, graph) = load_graph(&store, session.clone())?;
                if graph.revision.as_str() != expected_revision {
                    return Err(invalid(
                        "stale_graph_revision",
                        "graph changed while the ranking agent was running",
                    ));
                }
                let path = ranking_path(&indexer::graph_path(&snapshot));
                let _lock = RankingFileLock::acquire(&path)?;
                let context_digest = current_context_digest(&snapshot)?;
                if context_digest.as_deref() != expected_context_digest {
                    return Err(invalid(
                        "stale_context_digest",
                        "review context changed while the ranking agent was running",
                    ));
                }
                let mut ranking = RankingState::load_or_new(&path, &graph, context_digest.clone())?;
                if ranking.context_digest != context_digest {
                    return Err(invalid(
                        "stale_context_digest",
                        "review context changed while the ranking agent was running",
                    ));
                }
                ranking.apply_batch(&graph, expected_revision, updates)?;
                let queue = ranking.queue(&graph, context_digest.as_deref());
                if !queue.fully_ranked {
                    return Err(invalid(
                        "incomplete_agent_ranking",
                        "agent ranking does not assess every active change",
                    ));
                }
                let queue = ranking.finalize(&graph)?;
                ranking.save(&path)?;
                ranking_result = Some(ranking_ack(&queue, Some(expected_ids.len())));
            }
            let returned_invocation = if dry_run || titles_only {
                serde_json::to_value(&invocation)?
            } else {
                json!({
                    "source": invocation.source,
                    "name": invocation.name,
                    "working_directory": invocation.working_directory,
                })
            };
            Ok(json!({
                "launched": !dry_run,
                "cache_hit": false,
                "invocation": returned_invocation,
                "ranking": ranking_result,
            }))
        }
        Command::Explain(args) => {
            let settings = Settings::load(&Settings::default_path()?)?;
            let data_dir = cli
                .data_dir
                .clone()
                .unwrap_or_else(|| settings.data_dir.clone());
            let store = Store::open(data_dir.join("sessions.sqlite3"))?;
            let (snapshot, graph) = load_graph(&store, args.session.clone())?;
            let context = ContextBundle::load(&context_path(&snapshot))?;
            let test_evidence = TestEvidenceStore::load(&snapshot)?;
            let test_digest =
                (!test_evidence.digest.is_empty()).then(|| test_evidence.digest.clone());
            let path = crate::explanations::path(&snapshot);
            let mut explanations = ExplanationStore::load_or_new(
                &path,
                &snapshot,
                &graph,
                (!context.digest.is_empty()).then(|| context.digest.clone()),
                test_digest.clone(),
            )?;
            if args.show {
                return Ok(json!({
                    "stale": explanations.is_stale(
                        &snapshot,
                        &graph,
                        (!context.digest.is_empty()).then_some(context.digest.as_str()),
                        test_digest.as_deref(),
                    ),
                    "explanations": explanations,
                }));
            }
            if let Some(note) = args.note {
                explanations.set_manual_note(
                    &graph,
                    args.item.as_deref().expect("clap requires item"),
                    note,
                    args.expected_revision,
                )?;
                explanations.save(&path)?;
                return Ok(json!({ "explanations": explanations }));
            }
            if args.updates.is_some() || args.updates_file.is_some() {
                let raw = match (args.updates, args.updates_file) {
                    (Some(value), None) => value,
                    (None, Some(path)) => fs::read_to_string(path)?,
                    _ => unreachable!("clap makes updates mutually exclusive"),
                };
                let updates: Vec<ExplanationUpdate> =
                    serde_json::from_str(&raw).map_err(|error| {
                        invalid(
                            "invalid_explanation_batch",
                            format!("updates must be a JSON array: {error}"),
                        )
                    })?;
                let graph_revision = args.graph_revision.as_deref().ok_or_else(|| {
                    invalid(
                        "explanation_graph_revision_required",
                        "--graph-revision is required when applying updates",
                    )
                })?;
                explanations.apply_batch(
                    &snapshot,
                    &graph,
                    &context,
                    &test_evidence.evidence_ids(),
                    args.expected_revision,
                    graph_revision,
                    updates,
                )?;
                explanations.save(&path)?;
                return Ok(json!({ "explanations": explanations }));
            }
            if args.graph_revision.is_some() || args.item.is_some() {
                return Err(invalid(
                    "invalid_explain_options",
                    "revision/item options require --updates, --updates-file, or --note",
                ));
            }
            let stale = explanations.is_stale(
                &snapshot,
                &graph,
                (!context.digest.is_empty()).then_some(context.digest.as_str()),
                test_digest.as_deref(),
            );
            let cache_hit = !args.force && !stale && !explanations.entries.is_empty();
            if cache_hit {
                return Ok(json!({ "launched": false, "cache_hit": true, "invocation": null }));
            }
            let record = store.get_session(&parse_session_id(args.session.clone())?)?;
            let invocation = agent::prepare(
                &settings,
                cli.agent.as_deref(),
                cli.profile.as_deref(),
                &args.session,
                &record.repository,
                &data_dir,
                agent::Mode::Explain {
                    refresh: args.force,
                },
            )?;
            if !args.dry_run {
                agent::launch(&invocation, &args.session, &data_dir)?;
            }
            Ok(json!({
                "launched": !args.dry_run,
                "cache_hit": false,
                "invocation": invocation,
            }))
        }
        Command::AgentProfile(args) => {
            let path = Settings::default_path()?;
            let mut settings = Settings::load(&path)?;
            match args.command {
                AgentProfileCommand::List => Ok(json!({
                    "path": path,
                    "default_profile": settings.default_agent_profile,
                    "profiles": settings.agent_profiles,
                })),
                AgentProfileCommand::Select { name } => {
                    if !settings.agent_profiles.contains_key(&name) {
                        return Err(invalid(
                            "unknown_agent_profile",
                            format!("agent profile {name:?} is not configured"),
                        ));
                    }
                    settings.default_agent_profile = name;
                    settings.ensure_layout()?;
                    settings.save(&path)?;
                    let (name, profile) = settings.selected_agent_profile()?;
                    Ok(json!({ "path": path, "name": name, "profile": profile }))
                }
                AgentProfileCommand::Set {
                    name,
                    command,
                    args,
                    select,
                } => {
                    let mut command = vec![command];
                    command.extend(args);
                    settings
                        .agent_profiles
                        .insert(name.clone(), AgentProfile { command });
                    if select {
                        settings.default_agent_profile = name.clone();
                    }
                    settings.ensure_layout()?;
                    settings.save(&path)?;
                    Ok(json!({
                        "path": path,
                        "name": name,
                        "profile": settings.agent_profiles[&name],
                        "selected": settings.default_agent_profile == name,
                    }))
                }
                AgentProfileCommand::Resolve { name } => {
                    let (name, profile) = if let Some(name) = name.as_deref() {
                        let profile = settings.agent_profiles.get(name).ok_or_else(|| {
                            invalid(
                                "unknown_agent_profile",
                                format!("agent profile {name:?} is not configured"),
                            )
                        })?;
                        (name, profile)
                    } else {
                        settings.selected_agent_profile()?
                    };
                    Ok(json!({ "path": path, "name": name, "profile": profile }))
                }
            }
        }
        Command::GithubProfile(args) => {
            let settings_path = Settings::default_path()?;
            let mut settings = Settings::load(&settings_path)?;
            let legacy_path = cli.profile_config;
            let path = legacy_path.clone().unwrap_or(settings_path.clone());
            let mut profiles = if let Some(path) = legacy_path.as_deref() {
                ProfileStore::load(path)?
            } else {
                settings.profile_store()
            };
            match args.command {
                ProfileCommand::Set {
                    name,
                    repository_patterns,
                    github_command,
                    github_args,
                    expected_account,
                    api_host,
                } => {
                    let mut command = vec![github_command];
                    command.extend(github_args);
                    profiles.upsert(GitHubProfile {
                        name: name.clone(),
                        repository_patterns,
                        github_command: command,
                        expected_account,
                        api_host,
                    })?;
                    if legacy_path.is_some() {
                        profiles.save(&path)?;
                    } else {
                        settings.set_profiles(profiles.clone());
                        settings.ensure_layout()?;
                        settings.save(&settings_path)?;
                    }
                    let profile = profiles
                        .profiles
                        .iter()
                        .find(|profile| profile.name == name)
                        .expect("upserted profile exists");
                    Ok(json!({ "path": path, "profile": profile }))
                }
                ProfileCommand::List => Ok(json!({
                    "path": path,
                    "profiles": profiles.profiles,
                })),
                ProfileCommand::Resolve {
                    host,
                    repository,
                    name,
                } => {
                    let profile = profiles.resolve(&host, &repository, name.as_deref())?;
                    Ok(json!({
                        "target": format!("{host}/{repository}"),
                        "profile": profile,
                    }))
                }
                ProfileCommand::Exec {
                    repository_dir,
                    remote,
                    dry_run,
                    args,
                } => {
                    let (host, repository) = repository_identity(&repository_dir, &remote)?;
                    let profile = profiles
                        .resolve(&host, &repository, cli.profile.as_deref())?
                        .clone();
                    let api_host = profile.api_host.clone().unwrap_or_else(|| host.clone());
                    let mut command = profile.github_command.clone();
                    command.extend(args);
                    if dry_run {
                        Ok(json!({
                            "target": format!("{host}/{repository}"),
                            "profile": profile.name,
                            "expected_account": profile.expected_account,
                            "api_host": api_host,
                            "command": command,
                            "working_directory": repository_dir,
                        }))
                    } else {
                        verify_github_identity(&profile, &api_host)?;
                        exec_github_profile(&profile, &repository_dir, &command)
                    }
                }
            }
        }
        Command::Github(args) => {
            let profiles = if let Some(path) = cli.profile_config.as_deref() {
                ProfileStore::load(path)?
            } else {
                Settings::load(&Settings::default_path()?)?.profile_store()
            };
            let profile = profiles
                .resolve(&args.host, &args.repository, cli.profile.as_deref())?
                .clone();
            let profile_name = profile.name.clone();
            let api_host = profile.api_host.clone().unwrap_or(args.host);
            let adapter = GitHubAdapter::new(GitHubConfig {
                command: profile.command(),
                host: api_host,
                repository: args.repository,
                expected_account: profile.expected_account,
            })?;
            match args.command {
                GithubCommand::Fetch { number } => {
                    Ok(serde_json::to_value(adapter.fetch_pull(number)?)?)
                }
                GithubCommand::Create {
                    number,
                    local_repository,
                } => {
                    let pull = adapter.fetch_pull(number)?;
                    ensure_pr_objects(&local_repository, &pull)?;
                    let db = cli
                        .data_dir
                        .map(|p| p.join("sessions.sqlite3"))
                        .map(Ok)
                        .unwrap_or_else(Store::default_path)?;
                    let mut store = Store::open(db)?;
                    let snapshot = snapshot::capture(
                        &local_repository,
                        SnapshotInput::Branch {
                            base: pull.base_sha.clone(),
                            head: pull.head_sha.clone(),
                        },
                        store.root_dir(),
                    )?;
                    fs::write(
                        snapshot.storage_dir.join("github-context.json"),
                        serde_json::to_vec_pretty(&pull)?,
                    )?;
                    let mut context = ContextBundle::default();
                    capture_pull_context(&mut context, &pull, &snapshot);
                    context.save(&context_path(&snapshot))?;
                    let session = store.create_session(&local_repository)?;
                    let state = json!({
                        "snapshot_id": snapshot.id,
                        "snapshot_path": snapshot.storage_dir.join("snapshot.json"),
                        "review_units_version": 1,
                        "github": {
                            "profile": profile_name,
                            "host": pull.host,
                            "repository": pull.base_repository,
                            "number": pull.number,
                            "head_repository": pull.head_repository,
                        }
                    });
                    let session = store.update_state(&session.id, session.revision, &state)?;
                    Ok(json!({ "session": session, "snapshot": snapshot, "pull": pull }))
                }
                GithubCommand::Refresh { session } => {
                    let db = cli
                        .data_dir
                        .map(|p| p.join("sessions.sqlite3"))
                        .map(Ok)
                        .unwrap_or_else(Store::default_path)?;
                    let mut store = Store::open(db)?;
                    let id = parse_session_id(session)?;
                    let current = store.get_session(&id)?;
                    let previous = load_snapshot(&current)?;
                    let number = current
                        .state
                        .pointer("/github/number")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| {
                            invalid(
                                "session_has_no_github_pr",
                                "session is not attached to a GitHub PR",
                            )
                        })?;
                    let pull = adapter.fetch_pull(number)?;
                    ensure_pr_objects(&current.repository, &pull)?;
                    let snapshot = snapshot::capture(
                        &current.repository,
                        SnapshotInput::Branch {
                            base: pull.base_sha.clone(),
                            head: pull.head_sha.clone(),
                        },
                        store.root_dir(),
                    )?;
                    fs::write(
                        snapshot.storage_dir.join("github-context.json"),
                        serde_json::to_vec_pretty(&pull)?,
                    )?;
                    let mut context = ContextBundle::load(&context_path(&previous))?;
                    for entry in &mut context.entries {
                        if matches!(
                            entry.kind,
                            ContextSourceKind::PullRequest
                                | ContextSourceKind::ReviewSummary
                                | ContextSourceKind::ReviewThread
                        ) {
                            entry.status = CaptureStatus::Partial;
                            entry.anchor = None;
                        }
                    }
                    capture_pull_context(&mut context, &pull, &snapshot);
                    context.save(&context_path(&snapshot))?;
                    let transferred_hunks = snapshot::transferable_hunks(&previous, &snapshot);
                    let previous_drafts = DraftStore::load(&comments_path(&previous))?;
                    previous_drafts
                        .carry_to_snapshot(&snapshot)?
                        .save(&comments_path(&snapshot))?;
                    let old_progress = ReviewProgress::load(&progress_path(&previous.storage_dir))?;
                    let mut next_progress = old_progress.carry(&transferred_hunks);
                    next_progress.save_checked(&progress_path(&snapshot.storage_dir), 0)?;
                    let state = json!({
                        "snapshot_id": snapshot.id,
                        "snapshot_path": snapshot.storage_dir.join("snapshot.json"),
                        "refresh_from": previous.id,
                        "transferred_hunks": transferred_hunks,
                        "review_units_version": 1,
                        "github": {
                            "profile": profile_name,
                            "host": pull.host,
                            "repository": pull.base_repository,
                            "number": pull.number,
                            "head_repository": pull.head_repository,
                        }
                    });
                    let session = store.update_state(&id, current.revision, &state)?;
                    Ok(json!({
                        "session": session,
                        "snapshot": snapshot,
                        "pull": pull,
                        "transferred_hunks": transferred_hunks,
                    }))
                }
                GithubCommand::Preview {
                    session,
                    number,
                    comments,
                    event,
                    summary,
                } => {
                    let db = cli
                        .data_dir
                        .map(|p| p.join("sessions.sqlite3"))
                        .map(Ok)
                        .unwrap_or_else(Store::default_path)?;
                    let store = Store::open(db)?;
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let pull = adapter.fetch_pull(number)?;
                    let drafts = DraftStore::load(&comments_path(&snapshot))?;
                    let event = parse_review_event(&event)?;
                    let preview =
                        adapter.preview(&pull, &snapshot, &drafts, &comments, event, summary)?;
                    save_preview(&preview_path(&snapshot), &preview)?;
                    Ok(serde_json::to_value(preview)?)
                }
                GithubCommand::Submit { session } => {
                    let db = cli
                        .data_dir
                        .map(|p| p.join("sessions.sqlite3"))
                        .map(Ok)
                        .unwrap_or_else(Store::default_path)?;
                    let store = Store::open(db)?;
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let preview = load_preview(&preview_path(&snapshot))?;
                    let pull: PullRequestContext = serde_json::from_slice(&fs::read(
                        snapshot.storage_dir.join("github-context.json"),
                    )?)?;
                    let comments_path = comments_path(&snapshot);
                    let mut drafts = DraftStore::load(&comments_path)?;
                    let response =
                        adapter.submit(&preview, &pull, &drafts, &intent_path(&snapshot))?;
                    record_remote_comment_ids(&mut drafts, &preview, &response)?;
                    drafts.save(&comments_path)?;
                    Ok(response)
                }
                GithubCommand::Reconcile { session } => {
                    let db = cli
                        .data_dir
                        .map(|p| p.join("sessions.sqlite3"))
                        .map(Ok)
                        .unwrap_or_else(Store::default_path)?;
                    let store = Store::open(db)?;
                    let record = store.get_session(&parse_session_id(session)?)?;
                    let snapshot = load_snapshot(&record)?;
                    let preview = load_preview(&preview_path(&snapshot))?;
                    Ok(adapter.reconcile(&preview, &intent_path(&snapshot))?)
                }
            }
        }
    }
}

fn progress_session(command: &ProgressCommand) -> &str {
    match command {
        ProgressCommand::Show { session }
        | ProgressCommand::Select { session, .. }
        | ProgressCommand::Status { session, .. }
        | ProgressCommand::Next { session, .. } => session,
    }
}

fn load_snapshot(record: &SessionRecord) -> Result<snapshot::Snapshot> {
    let path = record
        .state
        .get("snapshot_path")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid(
                "session_has_no_snapshot",
                format!("session {} has no snapshot", record.id),
            )
        })?;
    snapshot::load(Path::new(path))
}

fn newest_indexed_session(store: &Store, repository: &Path) -> Result<SessionId> {
    store
        .sessions_for_repository(repository)?
        .into_iter()
        .find_map(|record| {
            load_snapshot(&record)
                .ok()
                .filter(|snapshot| indexer::graph_path(snapshot).is_file())
                .map(|_| record.id)
        })
        .ok_or_else(|| {
            invalid(
                "indexed_session_not_found",
                "no indexed review session exists for this repository; launch a review first",
            )
        })
}

fn session_summary(record: &SessionRecord) -> Value {
    let result = (|| -> Result<Value> {
        let snapshot = load_snapshot(record)?;
        let graph_path = indexer::graph_path(&snapshot);
        let file_count = snapshot.files.len();
        let change_count = snapshot
            .files
            .iter()
            .map(|file| file.hunks.len().max(usize::from(file.hunks.is_empty())))
            .sum::<usize>();
        if !graph_path.is_file() {
            return Ok(json!({
                "id": record.id,
                "repository": record.repository,
                "updated_at": record.updated_at,
                "input": snapshot.input,
                "stage": "captured",
                "files": file_count,
                "changes": change_count,
                "assessed": 0,
                "reviewed": 0,
            }));
        }
        let graph = ChangeGraph::load(&graph_path)?;
        let context_digest = current_context_digest(&snapshot)?;
        let ranking =
            RankingState::load_or_new(&ranking_path(&graph_path), &graph, context_digest.clone())?;
        let queue = ranking.queue(&graph, context_digest.as_deref());
        let progress = ReviewProgress::load(&progress_path(&snapshot.storage_dir))?;
        let reviewed = queue
            .items
            .iter()
            .filter(|item| progress.status(&item.node_id) == ReviewStatus::Reviewed)
            .count();
        let stage = if queue.stale {
            "stale"
        } else if queue.finalized {
            "ready"
        } else if queue.assessed_changes > 0 {
            "ranking"
        } else {
            "indexed"
        };
        Ok(json!({
            "id": record.id,
            "repository": record.repository,
            "updated_at": record.updated_at,
            "input": snapshot.input,
            "stage": stage,
            "files": file_count,
            "changes": queue.total_changes,
            "assessed": queue.assessed_changes,
            "reviewed": reviewed,
            "finalized": queue.finalized,
            "stale": queue.stale,
            "graph_revision": queue.graph_revision,
        }))
    })();
    result.unwrap_or_else(|error| {
        json!({
            "id": record.id,
            "repository": record.repository,
            "updated_at": record.updated_at,
            "stage": "error",
            "error": { "code": error.code(), "message": error.to_string() },
        })
    })
}

fn reusable_session(
    store: &Store,
    repository: &Path,
    input: &SnapshotInput,
) -> Result<Option<(SessionRecord, snapshot::Snapshot)>> {
    let fingerprint = snapshot::source_fingerprint(repository, input)?;
    for record in store.sessions_for_repository(repository)? {
        let Ok(candidate) = load_snapshot(&record) else {
            continue;
        };
        if &candidate.input == input && candidate.source_fingerprint == fingerprint {
            let confirmed = snapshot::source_fingerprint(repository, input)?;
            if confirmed == fingerprint {
                return Ok(Some((record, candidate)));
            }
            return Ok(None);
        }
    }
    Ok(None)
}

fn current_ranking_is_finalized(record: &SessionRecord) -> Result<bool> {
    let Ok(snapshot) = load_snapshot(record) else {
        return Ok(false);
    };
    let graph_path = indexer::graph_path(&snapshot);
    let Ok(graph) = active_graph_for_snapshot(&snapshot) else {
        return Ok(false);
    };
    let path = ranking_path(&graph_path);
    if !path.is_file() {
        return Ok(false);
    }
    let ranking: RankingState = serde_json::from_slice(&fs::read(path)?)?;
    let context_digest = current_context_digest(&snapshot)?;
    let queue = ranking.queue(&graph, context_digest.as_deref());
    Ok(ranking.finalized && !queue.stale)
}

fn current_ranking_has_all_titles(record: &SessionRecord) -> Result<bool> {
    let Ok(snapshot) = load_snapshot(record) else {
        return Ok(false);
    };
    let graph_path = indexer::graph_path(&snapshot);
    let Ok(graph) = active_graph_for_snapshot(&snapshot) else {
        return Ok(false);
    };
    let path = ranking_path(&graph_path);
    if !path.is_file() {
        return Ok(false);
    }
    let ranking: RankingState = serde_json::from_slice(&fs::read(path)?)?;
    let context_digest = current_context_digest(&snapshot)?;
    let queue = ranking.queue(&graph, context_digest.as_deref());
    Ok(!queue.stale
        && queue
            .items
            .iter()
            .filter(|item| matches!(item.kind, NodeKind::Hunk | NodeKind::ReviewUnit))
            .all(|item| {
                matches!(
                    item.title_source,
                    crate::ranking::TitleSource::Model | crate::ranking::TitleSource::Manual
                )
            }))
}

fn prepare_ranking_for_launch(record: &SessionRecord) -> Result<()> {
    let Ok(snapshot) = load_snapshot(record) else {
        return Ok(());
    };
    let graph_path = indexer::graph_path(&snapshot);
    let Ok(graph) = active_graph_for_snapshot(&snapshot) else {
        return Ok(());
    };
    let path = ranking_path(&graph_path);
    if !path.is_file() {
        return Ok(());
    }
    let _lock = RankingFileLock::acquire(&path)?;
    let ranking: RankingState = serde_json::from_slice(&fs::read(&path)?)?;
    let context_digest = current_context_digest(&snapshot)?;
    if !ranking.queue(&graph, context_digest.as_deref()).stale {
        return Ok(());
    }
    let context_key = ranking
        .context_digest
        .as_deref()
        .unwrap_or("none")
        .chars()
        .take(16)
        .collect::<String>();
    let backup = path.with_file_name(format!(
        "ranking.stale-{}-{context_key}.json",
        ranking.graph_revision
    ));
    fs::copy(&path, backup)?;
    ranking.restarted(&graph, context_digest).save(&path)
}

fn load_graph(store: &Store, session: String) -> Result<(snapshot::Snapshot, ChangeGraph)> {
    let record = store.get_session(&parse_session_id(session)?)?;
    let snapshot = load_snapshot(&record)?;
    let path = indexer::graph_path(&snapshot);
    if !path.exists() {
        return Err(invalid(
            "graph_not_built",
            "run `lgr graph build <session>` first",
        ));
    }
    let graph = active_graph_for_snapshot(&snapshot)?;
    Ok((snapshot, graph))
}

fn active_graph_for_snapshot(snapshot: &snapshot::Snapshot) -> Result<ChangeGraph> {
    let graph = ChangeGraph::load(&indexer::graph_path(snapshot))?;
    let projection = review_units::load(snapshot)?;
    let graph = review_units::project_graph(graph, projection.as_ref())?;
    let evidence = TestEvidenceStore::load(snapshot)?;
    Ok(test_evidence::project_runtime(graph, snapshot, &evidence))
}

fn partition_preview_summary(preview: &review_units::PartitionPreview) -> Value {
    json!({
        "split_hunks": preview.split_hunks,
        "raw_hunks": preview.raw_hunks,
        "leaf_units": preview.leaf_units,
        "projection": review_unit_projection_summary(&preview.review_units),
        "units": preview.review_units.units.iter().map(review_unit_summary).collect::<Vec<_>>(),
    })
}

fn review_unit_projection_summary(projection: &ReviewUnits) -> Value {
    json!({
        "schema_version": projection.schema_version,
        "graph_revision": projection.graph_revision,
        "projection_revision": projection.projection_revision,
        "algorithm": projection.algorithm,
        "config_digest": projection.config_digest,
        "active": projection.active,
        "unit_count": projection.units.len(),
    })
}

fn review_unit_summary(unit: &review_units::ReviewUnit) -> Value {
    json!({
        "id": unit.id,
        "parent_hunk_id": unit.parent_hunk_id,
        "part": unit.part,
        "parts": unit.parts,
        "title": unit.title,
        "changed_lines": unit.changed_lines,
        "old_range": unit.old_range,
        "new_range": unit.new_range,
    })
}

fn ensure_review_units(
    snapshot: &snapshot::Snapshot,
    graph: &ChangeGraph,
    config: &crate::settings::ReviewUnitSettings,
) -> Result<()> {
    let current = review_units::load(snapshot)?;
    let candidate = review_units::build(snapshot, graph, config);
    if current.as_ref().is_some_and(|current| {
        current.active
            && current.graph_revision == candidate.graph_revision
            && current.config_digest == candidate.config_digest
            && current.units == candidate.units
    }) {
        return Ok(());
    }
    let expected = current
        .as_ref()
        .map_or(0, |current| current.projection_revision);
    review_units::save_checked(snapshot, candidate, expected)
}

fn partition_backup_dir(snapshot: &snapshot::Snapshot, revision: u64) -> PathBuf {
    snapshot
        .storage_dir
        .join(format!("partition-backup-{revision}"))
}

fn backup_partition_state(snapshot: &snapshot::Snapshot, revision: u64) -> Result<()> {
    let destination = partition_backup_dir(snapshot, revision);
    if destination.is_dir() {
        fs::remove_dir_all(&destination)?;
    }
    fs::create_dir_all(&destination)?;
    let names = [
        "progress.json",
        "ranking.json",
        "comments.json",
        "review-units.json",
        "review-units.backup.json",
    ];
    let mut present = Vec::new();
    for name in names {
        let source = snapshot.storage_dir.join(name);
        if source.is_file() {
            fs::copy(source, destination.join(name))?;
            present.push(name);
        }
    }
    fs::write(
        destination.join("manifest.json"),
        serde_json::to_vec_pretty(&present)?,
    )?;
    Ok(())
}

fn restore_partition_state(
    snapshot: &snapshot::Snapshot,
    revision: u64,
    restore_comments: bool,
) -> Result<()> {
    let source = partition_backup_dir(snapshot, revision);
    if !source.is_dir() {
        return Ok(());
    }
    let present: Vec<String> = serde_json::from_slice(&fs::read(source.join("manifest.json"))?)?;
    for name in [
        "progress.json",
        "ranking.json",
        "review-units.json",
        "review-units.backup.json",
    ] {
        let backup = source.join(name);
        let destination = snapshot.storage_dir.join(name);
        if present.iter().any(|present| present == name) {
            fs::copy(backup, snapshot.storage_dir.join(name))?;
        } else if destination.is_file() {
            fs::remove_file(destination)?;
        }
    }
    if restore_comments {
        let destination = snapshot.storage_dir.join("comments.json");
        if present.iter().any(|present| present == "comments.json") {
            fs::copy(source.join("comments.json"), destination)?;
        } else if destination.is_file() {
            fs::remove_file(destination)?;
        }
    }
    Ok(())
}

fn migrate_partition_ranking(
    snapshot: &snapshot::Snapshot,
    raw_graph: &ChangeGraph,
    projection: &ReviewUnits,
) -> Result<()> {
    let path = ranking_path(&indexer::graph_path(snapshot));
    if !path.is_file() {
        return Ok(());
    }
    let graph = review_units::project_graph(raw_graph.clone(), Some(projection))?;
    let mut ranking: RankingState = serde_json::from_slice(&fs::read(&path)?)?;
    ranking
        .assessments
        .retain(|node_id, _| graph.nodes.contains_key(node_id));
    ranking
        .labels
        .retain(|node_id, _| graph.nodes.contains_key(node_id));
    ranking.graph_revision = graph.revision.to_string();
    ranking.finalized = false;
    ranking.finalized_at = None;
    ranking.save(&path)
}

fn migrate_partition_progress(
    snapshot: &snapshot::Snapshot,
    projection: &ReviewUnits,
) -> Result<()> {
    let path = progress_path(&snapshot.storage_dir);
    let mut progress = ReviewProgress::load(&path)?;
    let previous_revision = progress.revision;
    let mut changed = false;
    for unit in &projection.units {
        if progress.status(&unit.parent_hunk_id) == ReviewStatus::Reviewed {
            progress.set_status(unit.id.clone(), ReviewStatus::Reviewed);
            changed = true;
        }
    }
    if let Some(selected) = progress.selected.clone()
        && let Some(first) = projection
            .units
            .iter()
            .find(|unit| unit.parent_hunk_id == selected)
    {
        progress.selected = Some(first.id.clone());
        changed = true;
    }
    if changed {
        progress.save_checked(&path, previous_revision)?;
    }
    Ok(())
}

fn context_path(snapshot: &snapshot::Snapshot) -> PathBuf {
    snapshot.storage_dir.join("context.json")
}

fn capture_pull_context(
    bundle: &mut ContextBundle,
    pull: &PullRequestContext,
    snapshot: &snapshot::Snapshot,
) {
    let pull_url = format!(
        "https://{}/{}/pull/{}",
        pull.host, pull.base_repository, pull.number
    );
    bundle.add_markdown_with(
        "github:pull-request",
        format!("# {}\n\n{}", pull.title, pull.body),
        262_144,
        ContextMetadata {
            source_key: format!("github:pr:{}:{}", pull.base_repository, pull.number),
            kind: ContextSourceKind::PullRequest,
            external_id: Some(pull.number.to_string()),
            external_url: Some(pull_url.clone()),
            source_updated_at: Some(pull.captured_at),
            ..ContextMetadata::default()
        },
    );
    for review in &pull.reviews {
        let Some(body) = review.get("body").and_then(Value::as_str) else {
            continue;
        };
        let id = json_identity(review, "id");
        bundle.add_markdown_with(
            "github:review-summary",
            body.into(),
            65_536,
            ContextMetadata {
                source_key: format!("github:review:{}", id),
                kind: ContextSourceKind::ReviewSummary,
                external_id: Some(id),
                external_url: review
                    .get("html_url")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                author: review
                    .pointer("/user/login")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                source_updated_at: json_datetime(review, "submitted_at"),
                ..ContextMetadata::default()
            },
        );
    }
    for comment in &pull.issue_comments {
        let Some(body) = comment.get("body").and_then(Value::as_str) else {
            continue;
        };
        let id = json_identity(comment, "id");
        bundle.add_markdown_with(
            "github:issue-comment",
            body.into(),
            65_536,
            ContextMetadata {
                source_key: format!("github:issue-comment:{id}"),
                kind: ContextSourceKind::ReviewThread,
                external_id: Some(id.clone()),
                thread_id: Some(id),
                external_url: comment
                    .get("html_url")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                author: comment
                    .pointer("/user/login")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                source_updated_at: json_datetime(comment, "updated_at"),
                ..ContextMetadata::default()
            },
        );
    }
    for comment in &pull.review_comments {
        let Some(body) = comment.get("body").and_then(Value::as_str) else {
            continue;
        };
        let id = json_identity(comment, "id");
        let reply_to = comment.get("in_reply_to_id").and_then(Value::as_u64);
        let thread_id = reply_to.map_or_else(|| id.clone(), |id| id.to_string());
        let (anchor, status) = github_comment_anchor(comment, snapshot);
        bundle.add_markdown_with(
            "github:review-thread",
            body.into(),
            65_536,
            ContextMetadata {
                source_key: format!("github:review-comment:{id}"),
                kind: ContextSourceKind::ReviewThread,
                external_id: Some(id),
                thread_id: Some(thread_id),
                reply_to: reply_to.map(|id| id.to_string()),
                external_url: comment
                    .get("html_url")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                author: comment
                    .pointer("/user/login")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                source_updated_at: json_datetime(comment, "updated_at"),
                status,
                anchor,
            },
        );
    }
}

fn json_identity(value: &Value, key: &str) -> String {
    value
        .get(key)
        .map(|value| match value {
            Value::String(value) => value.clone(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| {
            let bytes = serde_json::to_vec(value).unwrap_or_default();
            format!("sha256:{}", &hex::encode(Sha256::digest(bytes))[..20])
        })
}

fn json_datetime(value: &Value, key: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&chrono::Utc))
}

fn github_comment_anchor(
    comment: &Value,
    snapshot: &snapshot::Snapshot,
) -> (Option<ContextAnchor>, CaptureStatus) {
    let side = match comment.get("side").and_then(Value::as_str) {
        Some("LEFT") => SourceSide::Left,
        Some("RIGHT") => SourceSide::Right,
        _ => return (None, CaptureStatus::Outdated),
    };
    let Some(line) = comment.get("line").and_then(Value::as_u64) else {
        return (None, CaptureStatus::Outdated);
    };
    if line == 0 || line > u32::MAX as u64 {
        return (None, CaptureStatus::Unmatched);
    }
    let Some(path) = comment.get("path").and_then(Value::as_str) else {
        return (None, CaptureStatus::Unmatched);
    };
    let path = snapshot
        .files
        .iter()
        .find_map(|file| {
            match side {
                SourceSide::Left => file.old_path.as_ref(),
                SourceSide::Right => file.new_path.as_ref(),
            }
            .filter(|candidate| candidate.display == path)
        })
        .cloned();
    let Some(path) = path else {
        return (None, CaptureStatus::Unmatched);
    };
    let commit_matches = comment
        .get("commit_id")
        .and_then(Value::as_str)
        .is_none_or(|commit| commit == snapshot.after_commit);
    if !commit_matches {
        return (None, CaptureStatus::Outdated);
    }
    (
        Some(ContextAnchor {
            snapshot_id: snapshot.id.to_string(),
            side,
            path,
            start_line: comment
                .get("start_line")
                .and_then(Value::as_u64)
                .unwrap_or(line) as u32,
            end_line: line as u32,
        }),
        CaptureStatus::Complete,
    )
}

fn current_context_digest(snapshot: &snapshot::Snapshot) -> Result<Option<String>> {
    let bundle = ContextBundle::load(&context_path(snapshot))?;
    Ok((!bundle.digest.is_empty()).then_some(bundle.digest))
}

fn parse_edge_kind(value: &str) -> Result<EdgeKind> {
    match value {
        "contains" => Ok(EdgeKind::Contains),
        "overlaps" => Ok(EdgeKind::Overlaps),
        "references" => Ok(EdgeKind::References),
        "calls" => Ok(EdgeKind::Calls),
        "test_reference" => Ok(EdgeKind::TestReference),
        "counterpart" => Ok(EdgeKind::Counterpart),
        "definition" => Ok(EdgeKind::Definition),
        "runtime_test" => Ok(EdgeKind::RuntimeTest),
        _ => Err(invalid(
            "unknown_edge_kind",
            format!("unknown edge kind {value}"),
        )),
    }
}

fn parse_source_side(value: &str) -> Result<SourceSide> {
    match value {
        "left" => Ok(SourceSide::Left),
        "right" => Ok(SourceSide::Right),
        _ => Err(invalid("invalid_source_side", "side must be left or right")),
    }
}

fn parse_run_status(value: &str) -> Result<RunStatus> {
    match value {
        "passed" => Ok(RunStatus::Passed),
        "failed" => Ok(RunStatus::Failed),
        "cancelled" => Ok(RunStatus::Cancelled),
        "timed_out" | "timed-out" => Ok(RunStatus::TimedOut),
        "partial" => Ok(RunStatus::Partial),
        "skipped" => Ok(RunStatus::Skipped),
        "error" => Ok(RunStatus::Error),
        _ => Err(invalid(
            "invalid_test_status",
            "status must be passed, failed, cancelled, timed-out, partial, skipped, or error",
        )),
    }
}

fn parse_review_event(value: &str) -> Result<ReviewEvent> {
    match value {
        "comment" => Ok(ReviewEvent::Comment),
        "approve" => Ok(ReviewEvent::Approve),
        "request-changes" | "request_changes" => Ok(ReviewEvent::RequestChanges),
        _ => Err(invalid(
            "invalid_review_event",
            "event must be comment, approve, or request-changes",
        )),
    }
}

fn parse_revision_spec(value: &str) -> Result<SnapshotInput> {
    let (base, head, merge_base) = if let Some((base, head)) = value.split_once("...") {
        (base, head, true)
    } else if let Some((base, head)) = value.split_once("..") {
        (base, head, false)
    } else {
        return Err(invalid(
            "invalid_revision_spec",
            "revision comparison must use A..B or A...B",
        ));
    };
    if base.is_empty() || head.is_empty() || base.contains("..") || head.contains("..") {
        return Err(invalid(
            "invalid_revision_spec",
            "revision comparison needs exactly two non-empty revisions",
        ));
    }
    if merge_base {
        Ok(SnapshotInput::Branch {
            base: base.into(),
            head: head.into(),
        })
    } else {
        Ok(SnapshotInput::Revisions {
            base: base.into(),
            head: head.into(),
        })
    }
}

fn repository_identity(repository: &Path, remote: &str) -> Result<(String, String)> {
    let url = crate::git::text(
        repository,
        &[
            crate::git::os("remote"),
            crate::git::os("get-url"),
            crate::git::os(remote),
        ],
    )?;
    parse_remote_identity(&url).ok_or_else(|| {
        invalid(
            "unsupported_remote_url",
            format!("remote {remote:?} URL {url:?} does not identify a host/owner/repository"),
        )
    })
}

fn parse_remote_identity(url: &str) -> Option<(String, String)> {
    let (host, path) = if let Some((_, rest)) = url.split_once("://") {
        let (authority, path) = rest.split_once('/')?;
        (authority.rsplit('@').next()?, path)
    } else {
        let (authority, path) = url.split_once(':')?;
        if !authority.contains('@') {
            return None;
        }
        (authority.rsplit('@').next()?, path)
    };
    let repository = path.trim_matches('/').strip_suffix(".git").unwrap_or(path);
    if host.is_empty() || repository.split('/').count() < 2 {
        return None;
    }
    Some((host.into(), repository.trim_matches('/').into()))
}

fn verify_github_identity(profile: &GitHubProfile, host: &str) -> Result<()> {
    let (executable, fixed_args) = profile.github_command.split_first().ok_or_else(|| {
        invalid(
            "invalid_profile",
            format!("GitHub profile {:?} has no command", profile.name),
        )
    })?;
    let output = std::process::Command::new(executable)
        .args(fixed_args)
        .args(["api", "--hostname", host, "user", "--jq", ".login"])
        .output()
        .map_err(|error| {
            invalid(
                "github_identity_check_failed",
                format!("could not run GitHub profile {:?}: {error}", profile.name),
            )
        })?;
    if !output.status.success() {
        return Err(invalid(
            "github_identity_check_failed",
            format!(
                "GitHub profile {:?} identity check failed: {}",
                profile.name,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    let account = String::from_utf8_lossy(&output.stdout);
    if account.trim() != profile.expected_account {
        return Err(invalid(
            "github_identity_mismatch",
            format!(
                "GitHub profile {:?} authenticated as {}, expected {}",
                profile.name,
                account.trim(),
                profile.expected_account
            ),
        ));
    }
    Ok(())
}

fn exec_github_profile(
    profile: &GitHubProfile,
    repository: &Path,
    command: &[String],
) -> Result<Value> {
    let (executable, args) = command.split_first().ok_or_else(|| {
        invalid(
            "invalid_profile",
            format!("GitHub profile {:?} has no command", profile.name),
        )
    })?;
    let error = std::process::Command::new(executable)
        .args(args)
        .current_dir(repository)
        .exec();
    Err(invalid(
        "github_profile_exec_failed",
        format!(
            "could not launch GitHub profile {:?}: {error}",
            profile.name
        ),
    ))
}

fn ensure_pr_objects(repository: &Path, pull: &PullRequestContext) -> Result<()> {
    if crate::git::text(
        repository,
        &[
            crate::git::os("cat-file"),
            crate::git::os("-t"),
            std::ffi::OsStr::new(&pull.base_sha),
        ],
    )
    .is_ok()
        && crate::git::text(
            repository,
            &[
                crate::git::os("cat-file"),
                crate::git::os("-t"),
                std::ffi::OsStr::new(&pull.head_sha),
            ],
        )
        .is_ok()
    {
        return Ok(());
    }
    crate::git::run(
        repository,
        &[
            crate::git::os("fetch"),
            crate::git::os("--no-write-fetch-head"),
            crate::git::os("--no-tags"),
            crate::git::os("origin"),
            std::ffi::OsStr::new(&pull.base_sha),
            std::ffi::OsStr::new(&format!("refs/pull/{}/head", pull.number)),
        ],
    )?;
    Ok(())
}

fn project_node(node: &crate::graph::GraphNode, fields: &[String]) -> Value {
    let full = serde_json::to_value(node).expect("graph node is serializable");
    if fields.is_empty() {
        return full;
    }
    let mut projected = serde_json::Map::new();
    let object = full.as_object().expect("graph node serializes as object");
    for field in fields {
        if let Some(value) = object.get(field) {
            projected.insert(field.clone(), value.clone());
        }
    }
    Value::Object(projected)
}

fn walk_query_key(seeds: &[String], kinds: &BTreeSet<EdgeKind>, depth: usize) -> String {
    let mut digest = Sha256::new();
    for seed in seeds {
        digest.update(seed.as_bytes());
    }
    for kind in kinds {
        digest.update(format!("{kind:?}").as_bytes());
    }
    digest.update(depth.to_le_bytes());
    hex::encode(digest.finalize())
}

fn encode_continuation(revision: &str, query_key: &str, offset: usize) -> String {
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        serde_json::to_vec(&json!({
            "graph_revision": revision,
            "query": query_key,
            "offset": offset,
        }))
        .expect("continuation is serializable"),
    )
}

fn decode_continuation(value: &str, revision: &str, query_key: &str) -> Result<usize> {
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, value)
        .map_err(|error| invalid("invalid_continuation", error.to_string()))?;
    let payload: Value = serde_json::from_slice(&bytes)
        .map_err(|error| invalid("invalid_continuation", error.to_string()))?;
    if payload.get("graph_revision").and_then(Value::as_str) != Some(revision)
        || payload.get("query").and_then(Value::as_str) != Some(query_key)
    {
        return Err(invalid(
            "stale_continuation",
            "continuation belongs to another graph revision or traversal",
        ));
    }
    payload
        .get("offset")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .ok_or_else(|| invalid("invalid_continuation", "continuation has no valid offset"))
}

fn source_item(
    snapshot: &snapshot::Snapshot,
    graph: &ChangeGraph,
    node: &str,
    offset: usize,
    max_bytes: usize,
) -> Value {
    let result = (|| -> Result<Value> {
        let graph_node = graph
            .nodes
            .get(node)
            .ok_or_else(|| invalid("unknown_node", format!("node {node} does not exist")))?;
        let location = graph_node.preferred_review_location().ok_or_else(|| {
            invalid(
                "node_has_no_source",
                format!("node {node} has no source location"),
            )
        })?;
        let side = match location.side {
            SourceSide::Left => "before",
            SourceSide::Right => "after",
        };
        let path = snapshot
            .storage_dir
            .join(side)
            .join(location.path.to_path_buf()?);
        let bytes = fs::read(path)?;
        let start = offset.min(bytes.len());
        let end = start.saturating_add(max_bytes).min(bytes.len());
        Ok(json!({
            "node_id": node,
            "side": side,
            "path": location.path,
            "range": location.range,
            "offset": start,
            "next_offset": (end < bytes.len()).then_some(end),
            "total_bytes": bytes.len(),
            "content_base64": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                &bytes[start..end],
            ),
        }))
    })();
    match result {
        Ok(value) => value,
        Err(error) => json!({
            "node_id": node,
            "error": { "code": error.code(), "message": error.to_string() }
        }),
    }
}

fn hunk_item(
    snapshot: &snapshot::Snapshot,
    projection: Option<&ReviewUnits>,
    id: &str,
    offset: usize,
    max_bytes: usize,
) -> Value {
    let raw_id = review_units::find(projection, id).map_or(id, |unit| unit.parent_hunk_id.as_str());
    let hunk = snapshot
        .files
        .iter()
        .flat_map(|file| &file.hunks)
        .find(|hunk| hunk.id == raw_id);
    let Some(hunk) = hunk else {
        return json!({
            "hunk_id": id,
            "error": { "code": "unknown_hunk", "message": "hunk does not exist" }
        });
    };
    let unit_patch =
        projection.and_then(|projection| review_units::unit_patch(snapshot, projection, id));
    let bytes = unit_patch.as_deref().unwrap_or(&hunk.patch).as_bytes();
    let start = offset.min(bytes.len());
    let end = start.saturating_add(max_bytes).min(bytes.len());
    json!({
        "hunk_id": id,
        "parent_hunk_id": raw_id,
        "header": hunk.header,
        "offset": start,
        "next_offset": (end < bytes.len()).then_some(end),
        "total_bytes": bytes.len(),
        "content_base64": base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &bytes[start..end],
        ),
    })
}

fn ranking_evidence_item(
    snapshot: &snapshot::Snapshot,
    projection: Option<&ReviewUnits>,
    graph: &ChangeGraph,
    id: &str,
    max_bytes: usize,
) -> Value {
    let node = &graph.nodes[id];
    let mut item = if let Some(raw_id) = review_units::parent_hunk_id(graph, id) {
        let hunk = snapshot
            .files
            .iter()
            .flat_map(|file| &file.hunks)
            .find(|hunk| hunk.id == raw_id);
        match hunk {
            Some(hunk) => {
                let unit_patch =
                    projection.and_then(|units| review_units::unit_patch(snapshot, units, id));
                let patch = unit_patch.as_deref().unwrap_or(&hunk.patch);
                let mut end = max_bytes.min(patch.len());
                while !patch.is_char_boundary(end) {
                    end -= 1;
                }
                json!({
                    "hunk_id": id,
                    "parent_hunk_id": raw_id,
                    "header": hunk.header,
                    "next_offset": (end < patch.len()).then_some(end),
                    "total_bytes": patch.len(),
                    "patch": &patch[..end],
                })
            }
            None => json!({
                "hunk_id": id,
                "error": { "code": "unknown_hunk", "message": "hunk does not exist" }
            }),
        }
    } else {
        json!({ "total_bytes": 0, "next_offset": null })
    };
    let object = item.as_object_mut().expect("evidence item is an object");
    object.insert("node_id".into(), json!(id));
    object.insert("kind".into(), json!(node.kind));
    object.insert("name".into(), json!(node.name));
    if let Some(location) = node.preferred_review_location() {
        object.insert("location".into(), json!(location));
    }
    item
}

fn bounded_ranking_evidence(
    snapshot: &snapshot::Snapshot,
    graph: &ChangeGraph,
    max_bytes: usize,
) -> Result<Value> {
    if max_bytes == 0 || max_bytes > 192 * 1024 {
        return Err(invalid(
            "invalid_evidence_limit",
            "max-bytes must be 1..196608",
        ));
    }
    let context = ContextBundle::load(&context_path(snapshot))?;
    let projection = review_units::load(snapshot)?;
    let ids: Vec<_> = review_units::active_leaf_ids(graph).into_iter().collect();
    let patch_sizes: Vec<_> = ids
        .iter()
        .map(|id| {
            (
                id.clone(),
                ranking_evidence_item(snapshot, projection.as_ref(), graph, id, 0)["total_bytes"]
                    .as_u64()
                    .unwrap_or(0) as usize,
            )
        })
        .collect();
    let total_patch_bytes: usize = patch_sizes.iter().map(|(_, size)| size).sum();
    let mut lower = 0;
    let mut upper = total_patch_bytes.min(max_bytes);
    let mut response = ranking_evidence_response(
        snapshot,
        projection.as_ref(),
        graph,
        &context,
        &patch_sizes,
        0,
    );
    if serialized_response_bytes(&response)? > max_bytes {
        return Err(invalid(
            "evidence_metadata_exceeds_limit",
            "context and change metadata exceed max-bytes",
        ));
    }
    while lower < upper {
        let candidate_bytes = lower + (upper - lower).div_ceil(2);
        let candidate = ranking_evidence_response(
            snapshot,
            projection.as_ref(),
            graph,
            &context,
            &patch_sizes,
            candidate_bytes,
        );
        if serialized_response_bytes(&candidate)? <= max_bytes {
            lower = candidate_bytes;
            response = candidate;
        } else {
            upper = candidate_bytes - 1;
        }
    }
    Ok(response)
}

fn ranking_evidence_response(
    snapshot: &snapshot::Snapshot,
    projection: Option<&ReviewUnits>,
    graph: &ChangeGraph,
    context: &ContextBundle,
    patch_sizes: &[(String, usize)],
    patch_budget: usize,
) -> Value {
    let total_patch_bytes: usize = patch_sizes.iter().map(|(_, size)| size).sum();
    let items: Vec<_> = patch_sizes
        .iter()
        .map(|(id, total_bytes)| {
            let allowance = if total_patch_bytes <= patch_budget {
                *total_bytes
            } else {
                patch_budget.saturating_mul(*total_bytes) / total_patch_bytes
            };
            ranking_evidence_item(snapshot, projection, graph, id, allowance)
        })
        .collect();
    let evidence_complete = context.entries.iter().all(|entry| !entry.truncated)
        && items.iter().all(|item| {
            item.get("error").is_none() && item.get("next_offset").is_none_or(Value::is_null)
        });
    json!({
        "graph_revision": graph.revision,
        "context_digest": (!context.digest.is_empty()).then_some(&context.digest),
        "context": context,
        "coverage": graph.coverage,
        "unfinished_frontier": graph.unfinished_frontier,
        "evidence_complete": evidence_complete,
        "items": items,
    })
}

fn serialized_response_bytes(response: &Value) -> Result<usize> {
    Ok(serde_json::to_vec(&Envelope::success(response))?.len())
}

fn ranking_ack(queue: &crate::ranking::Queue, submitted: Option<usize>) -> Value {
    json!({
        "graph_revision": queue.graph_revision,
        "context_digest": queue.context_digest,
        "submitted": submitted,
        "assessed_changes": queue.assessed_changes,
        "total_changes": queue.total_changes,
        "fully_ranked": queue.fully_ranked,
        "finalized": queue.finalized,
        "stale": queue.stale,
        "metrics": queue.metrics,
    })
}

fn response_content_bytes(response: &Value) -> u64 {
    response
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.get("patch")
                .and_then(Value::as_str)
                .map(|patch| patch.len() as u64)
                .or_else(|| {
                    item.get("content_base64")
                        .and_then(Value::as_str)
                        .and_then(|content| {
                            base64::Engine::decode(
                                &base64::engine::general_purpose::STANDARD,
                                content,
                            )
                            .ok()
                        })
                        .map(|bytes| bytes.len() as u64)
                })
        })
        .sum()
}

fn record_query_metrics(
    snapshot: &snapshot::Snapshot,
    graph: &ChangeGraph,
    response: &Value,
    source_bytes: u64,
    elapsed: std::time::Duration,
) -> Result<()> {
    let path = ranking_path(&indexer::graph_path(snapshot));
    let _lock = RankingFileLock::acquire(&path)?;
    let context_digest = current_context_digest(snapshot)?;
    let mut ranking = RankingState::load_or_new(&path, graph, context_digest)?;
    if ranking.graph_revision != graph.revision.as_str() {
        return Ok(());
    }
    ranking.metrics.query_count += 1;
    ranking.metrics.returned_bytes += serde_json::to_vec(response)?.len() as u64;
    ranking.metrics.requested_source_bytes += source_bytes;
    ranking.metrics.elapsed_ms += elapsed.as_millis() as u64;
    ranking.save(&path)
}

fn open_neovim(command: &Path, server: Option<&str>, session: &str, node: &str) -> Result<()> {
    if !node
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(invalid(
            "invalid_editor_node",
            "node ID contains unsafe characters",
        ));
    }
    let status = if let Some(server) = server {
        std::process::Command::new(command)
            .args([
                "--server",
                server,
                "--remote-send",
                &format!("<Cmd>lua require('lazy-git-review').open('{node}')<CR>"),
            ])
            .status()?
    } else {
        std::process::Command::new(command)
            .args([
                "-c",
                &format!("lua require('lazy-git-review').attach('{session}')"),
                "-c",
                &format!("lua require('lazy-git-review').open('{node}')"),
            ])
            .status()?
    };
    if status.success() {
        Ok(())
    } else {
        Err(invalid(
            "editor_open_failed",
            format!("Neovim exited with {status}; queue selection and drafts were retained"),
        ))
    }
}

fn invalid(code: &'static str, message: impl Into<String>) -> AppError {
    AppError::InvalidInput {
        code,
        message: message.into(),
    }
}

fn parse_session_id(id: String) -> Result<SessionId> {
    SessionId::parse(id).map_err(|message| AppError::InvalidInput {
        code: "invalid_session_id",
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{FileChange, GitPath, Snapshot};
    use chrono::Utc;

    #[test]
    fn pull_context_keeps_stable_threads_and_only_exact_current_anchors() {
        let snapshot = Snapshot {
            id: crate::model::SnapshotId::new(),
            repository: ".".into(),
            input: SnapshotInput::Uncommitted,
            original_base: "base".into(),
            original_head: "head".into(),
            comparison_base: "base".into(),
            before_commit: "base".into(),
            after_commit: "head".into(),
            captured_at: Utc::now(),
            source_fingerprint: "source".into(),
            files: vec![FileChange {
                status: "M".into(),
                old_path: Some(GitPath::from_bytes(b"a.ts".to_vec())),
                new_path: Some(GitPath::from_bytes(b"a.ts".to_vec())),
                old_mode: "100644".into(),
                new_mode: "100644".into(),
                old_object: "old".into(),
                new_object: "new".into(),
                before_blob: None,
                after_blob: None,
                binary: false,
                submodule: false,
                hunks: vec![],
            }],
            storage_dir: ".".into(),
        };
        let pull = PullRequestContext {
            number: 7,
            host: "github.com".into(),
            base_repository: "example/repository".into(),
            head_repository: "fork/repository".into(),
            title: "Change behavior".into(),
            body: "Intent".into(),
            head_ref: "feature/JT-1-change".into(),
            base_sha: "base".into(),
            head_sha: "head".into(),
            issue_comments: vec![json!({
                "id": 10, "body": "discussion", "html_url": "https://example/10",
                "user": {"login": "alice"}, "updated_at": "2026-09-09T10:00:00Z"
            })],
            reviews: vec![json!({
                "id": 11, "body": "summary", "html_url": "https://example/11",
                "user": {"login": "bob"}, "submitted_at": "2026-09-09T10:01:00Z"
            })],
            review_comments: vec![
                json!({
                    "id": 12, "body": "exact", "path": "a.ts", "side": "RIGHT",
                    "line": 1, "commit_id": "head", "user": {"login": "carol"}
                }),
                json!({
                    "id": 13, "body": "old", "path": "a.ts", "side": "RIGHT",
                    "line": 1, "commit_id": "older", "in_reply_to_id": 12
                }),
                json!({
                    "id": 14, "body": "missing", "path": "gone.ts", "side": "LEFT",
                    "line": 2
                }),
            ],
            captured_at: Utc::now(),
        };
        let mut bundle = ContextBundle::default();
        capture_pull_context(&mut bundle, &pull, &snapshot);
        let ids = bundle
            .entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        capture_pull_context(&mut bundle, &pull, &snapshot);
        assert_eq!(bundle.entries.len(), 6);
        assert_eq!(
            bundle
                .entries
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        let exact = bundle
            .entries
            .iter()
            .find(|entry| entry.external_id.as_deref() == Some("12"))
            .unwrap();
        assert_eq!(exact.status, CaptureStatus::Complete);
        assert_eq!(
            exact.anchor.as_ref().unwrap().snapshot_id,
            snapshot.id.as_str()
        );
        let old = bundle
            .entries
            .iter()
            .find(|entry| entry.external_id.as_deref() == Some("13"))
            .unwrap();
        assert_eq!(old.thread_id.as_deref(), Some("12"));
        assert_eq!(old.reply_to.as_deref(), Some("12"));
        assert_eq!(old.status, CaptureStatus::Outdated);
        assert!(old.anchor.is_none());
        let missing = bundle
            .entries
            .iter()
            .find(|entry| entry.external_id.as_deref() == Some("14"))
            .unwrap();
        assert_eq!(missing.status, CaptureStatus::Unmatched);
    }
}

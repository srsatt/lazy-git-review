## Purpose

Let an external ranking model efficiently inspect change evidence and produce an explainable hunk-level review order for a later reviewer.

## ADDED Requirements

### Requirement: Provide compact machine-readable graph access

The CLI SHALL provide versioned JSON operations for inventory, batch node lookup, edge-filtered traversal from multiple seeds, bounded recursive traversal, and retrieval of selected hunks or source ranges. Traversal SHALL deduplicate cycles and enforce node, edge, depth, and response-size limits. Responses SHALL expose continuation state, omitted content, graph revision, and structured per-item errors. Logs SHALL NOT contaminate JSON stdout.

#### Scenario: Batch traverses shared callers and tests
- **WHEN** the agent traverses multiple related symbols with a test-reference filter
- **THEN** shared results appear once, limits apply to the entire request, and pagination can resume without losing the remaining frontier

#### Scenario: One requested node does not exist
- **WHEN** a read batch contains valid and unknown node identifiers
- **THEN** valid results are returned with an explicit error for the unknown item

### Requirement: Separate graph expansion from read traversal

Reading a graph SHALL NOT trigger unbounded indexing. Explicit expansion SHALL respect indexing budgets, produce a new graph revision, and expose changed coverage. Finalized rankings based on an older graph revision SHALL be marked stale.

#### Scenario: Agent needs an omitted dependency frontier
- **WHEN** it explicitly requests bounded graph expansion
- **THEN** the updated graph can be queried, and subsequent ranking updates must identify the new revision

### Requirement: Store evidence-backed weights and tags

The CLI SHALL accept atomic batch score updates for hunks and symbols. Each update SHALL contain a bounded numeric importance score, tags, a concise rationale, confidence, and valid evidence identifiers. Tags SHALL support security, non-trivial logic, behavior, API, tests, configuration, mechanical changes, and user-defined labels. Manual values SHALL take precedence over later model updates.

#### Scenario: Invalid entry in a score batch
- **WHEN** a write batch contains an unknown node, invalid score, invalid evidence reference, or stale revision
- **THEN** the entire batch is rejected with actionable errors and no partial score changes

#### Scenario: User raises a hunk's importance
- **WHEN** the agent reranks the same snapshot
- **THEN** the manual override remains effective until the user removes it

### Requirement: Derive a deterministic complete review queue

The default queue SHALL order active review leaves by importance, emphasize consequential logic and security through recorded scores/tags, and use stable tie-breaks. Raw hunks SHALL be the leaves when no partition is active; bounded review-unit children SHALL replace a split parent in completion accounting when a projection is active. A leaf-specific score SHALL override inherited parent-hunk or symbol scores; otherwise the highest applicable inherited score SHALL apply. Symbols and aggregate parents SHALL remain inspectable without duplicating their leaves in queue completion accounting. Every captured change SHALL remain discoverable, including unranked and semantically unsupported changes.

#### Scenario: File contains one risky hunk and many mechanical hunks
- **WHEN** these hunks receive different scores
- **THEN** the risky hunk can appear earlier independently of its file's other hunks

#### Scenario: Model stops before ranking all changes
- **WHEN** ranking reaches its time or invocation budget
- **THEN** the queue is labeled partially ranked and identifies all unassessed changes; no change is silently dropped

### Requirement: Supply an efficient ranking-only agent skill

The release SHALL include a skill for an external agent to read the inventory and context, batch related queries, follow bounded neighborhoods, retrieve source selectively, submit validated score batches, and finalize with a coverage/budget summary. It SHALL instruct the agent to prioritize importance, non-trivial logic, and security, account for every change, and distinguish uncertainty from low importance. Ranking SHALL NOT require a built-in model client, generate automated bug findings, modify source, or post reviews.

#### Scenario: Independent models rank and review
- **WHEN** the ranking agent finishes
- **THEN** a separate reviewer can consume the persisted queue, evidence, context, and hunks without the ranking conversation or model credentials

### Requirement: Configure shared storage and external agent profiles in JSON

The system SHALL use a versioned `~/.lgr/settings.json` file for the default data directory, user scripts directory, named external agent-profile argv, selected agent profile, and GitHub profiles. The default layout SHALL use `~/.lgr/data` and `~/.lgr/scripts`; explicit CLI data/profile flags SHALL remain temporary overrides. Fresh settings SHALL provide generic `codex` and `opencode` agent profiles, select `codex`, and contain no developer or account identity. Version-one harness fields SHALL remain readable. Configuration SHALL NOT execute an agent, store model credentials, or make the binary a model provider client.

#### Scenario: Review commands omit the data directory
- **WHEN** a session is created and later opened without `--data-dir`
- **THEN** both commands use the data directory from `~/.lgr/settings.json`

#### Scenario: User selects a Codex agent profile
- **WHEN** the user selects `codex` or `opencode`
- **THEN** settings persist the corresponding structured argv and subsequent resolution reports it without launching Codex

#### Scenario: Installer runs with existing settings
- **WHEN** installation initializes the configuration layout and `settings.json` already exists
- **THEN** the existing valid settings and user scripts remain unchanged

### Requirement: Launch ranking with injected skill instructions

The CLI SHALL launch ranking with either an ad-hoc `--agent/-a` executable or a named `--profile/-p`; these options SHALL be mutually exclusive and omission SHALL use the configured default agent profile. Bare ad-hoc names SHALL resolve from the configured scripts directory before `PATH`. The process SHALL receive the session and data directory, reviewed-repository working directory, and bundled ranking skill without requiring harness-specific skill setup. Arguments SHALL NOT pass through a shell. A dry-run SHALL expose the resolved invocation without launching it.

#### Scenario: Ad-hoc OpenCode ranking
- **WHEN** the user runs `lgr rank SESSION --agent opencode`
- **THEN** the tool launches `opencode run` with an injected ranking-only prompt bound to that session

#### Scenario: Named profile ranking
- **WHEN** the user runs `lgr rank SESSION --profile codex`
- **THEN** the tool launches the configured structured argv and appends the same embedded skill prompt

### Requirement: Measure traversal efficiency and resume

Ranking runs SHALL expose query count, returned bytes, requested source volume, elapsed time, and assessed-change count. Resumption SHALL retain completed assessments for the same graph/context revisions. Budgets SHALL be configurable and their exhaustion explicit.

The rank launcher SHALL reuse a finalized ranking while its graph revision and context digest remain current, without starting an agent process. A changed graph or context SHALL invalidate that result. An explicit force option SHALL launch the agent even when the result is current.

#### Scenario: Agent restarts midway through ranking
- **WHEN** it resumes an unchanged session
- **THEN** the inventory identifies unassessed items and prior evidence so the skill can avoid repeating completed work

#### Scenario: Finalized ranking is requested again
- **WHEN** ranking is launched for an unchanged graph and context with a finalized result
- **THEN** the CLI reports a cache hit and does not start the configured agent

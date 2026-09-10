## Purpose

Guide reviewers through the ranked queue with persistent progress while reusing Neovim and Diffview for detailed source inspection.

## ADDED Requirements

### Requirement: Distribute a licensed native command-line application

The first public release SHALL be version `0.1.0` and provide Rust-built binaries for macOS Apple Silicon and Linux x86_64, the agent skill, and a Neovim bridge. Project source and release bundles SHALL be licensed under EUPL-1.2 and include the canonical licence text plus third-party notices. A dependency diagnostic SHALL identify missing Git, language servers and runtimes, Neovim/plugins, and GitHub adapter requirements separately. Local inventory and queue operations SHALL NOT require a model API key or running web service.

#### Scenario: Language server is missing
- **WHEN** the user checks dependencies or starts a review
- **THEN** the tool reports the exact missing capability and an actionable setup path while retaining raw Git review

#### Scenario: User inspects a release bundle
- **WHEN** the user downloads a supported `0.1.0` release archive
- **THEN** the archive identifies the release version and contains the EUPL-1.2 text, third-party notices, binary, helper commands, agent skill, Neovim bridge, and setup documentation

### Requirement: Document complete setup and recovery

The release SHALL provide one indexed documentation path covering archive and source installation, external dependencies, initial configuration, agent profiles, Neovim/Diffview/code-review.nvim integration, gh-dash and GitHub profiles, verification with `lgr doctor`, upgrades, uninstall, data location and backup, and troubleshooting. Commands SHALL distinguish required steps from optional integrations and use portable placeholders instead of developer-specific paths or identities. Documented setup SHALL be verified from a clean isolated configuration without depending on the developer's existing settings.

#### Scenario: New user configures the Neovim workflow
- **WHEN** a user follows the documented setup on a supported platform
- **THEN** they can install `lgr`, configure a ranking agent and required plugins, pass the documented diagnostics, launch a review, and reopen its ranked queue

#### Scenario: Optional GitHub integration is unavailable
- **WHEN** a user configures local review without gh-dash or a GitHub profile
- **THEN** the documentation identifies that integration as optional and local capture, ranking, navigation, drafts, and Markdown export remain usable

### Requirement: Keep the TUI ranked-queue-first with graph drill-down

The TUI SHALL display active review leaves—raw hunks or projected review units—with symbol/path, importance, tags, assessment state, and review progress. Queue rows SHALL preserve the distinguishing end of long paths and leave room for the hunk, unit, or symbol name. Aggregate raw parents SHALL remain inspectable without entering completion totals twice. It SHALL support keyboard navigation, tag/status filtering, manual priority changes, marking reviewed/unreviewed, and resuming the session. Filtered-out items SHALL remain included in total completion accounting. The TUI SHALL expose source identity and incomplete-ranking/analysis state when relevant.

From the selected queue item, the TUI SHALL provide a one-key graph drill-down over the persisted graph. It SHALL show typed and directed immediate relations, prioritize test references, usages/references, imports/definitions, and calls, permit recursive neighbor traversal with a back path, open any sourced relation in Neovim, and return to the unchanged queue selection. Graph browsing SHALL build an in-memory adjacency index once and SHALL NOT invoke an LSP server, ranking agent, network request, or database query per navigation step.

#### Scenario: Reviewer filters to security changes
- **WHEN** all visible security-tagged hunks are reviewed
- **THEN** the queue still reports remaining unreviewed changes outside the filter

#### Scenario: Reviewer follows a changed hunk to tests
- **WHEN** the reviewer opens graph drill-down for a ranked hunk and follows a `test_reference` relation
- **THEN** the TUI identifies relation direction and evidence, permits further traversal or source opening, and returns to the same ranked hunk without recomputing the graph

### Requirement: Open the exact source and hunk in Neovim

Selecting a queue item SHALL open its snapshot comparison, file, hunk, and chosen side in Neovim using Diffview when compatible. The bridge SHALL support committed, staged, and unstaged snapshots without substituting changed working-tree content. It SHALL support an attached Neovim instance and a suspend/open/resume terminal workflow. External processes SHALL receive paths and arguments without shell interpolation.

#### Scenario: Navigate to a deletion
- **WHEN** the queue opens a deleted hunk
- **THEN** Neovim displays the before-side source at the deletion location

#### Scenario: Working tree changed after ranking
- **WHEN** the user opens a captured unstaged hunk
- **THEN** the editor displays captured content, indicates its source identity, and does not silently display the newer file

### Requirement: Share progress without replacing editor bindings

The TUI and Neovim bridge SHALL operate on one session's progress and comments. Queue next/previous actions from either surface SHALL follow the persisted ranking. Existing comment keybindings SHALL remain usable, including the user's `<leader>rc` mapping; adapter mappings SHALL be configurable.

#### Scenario: Add a comment and advance from Neovim
- **WHEN** the reviewer comments using the existing plugin and chooses next ranked item
- **THEN** the comment is stored for the current item and both surfaces select the next queue item

### Requirement: Launch common review inputs from Neovim

The Neovim bridge SHALL expose asynchronous actions for all uncommitted changes, staged changes, unstaged changes, the current branch against a configurable development base, and the current branch against a prompted base. A launch SHALL reuse an unchanged session, graph, and finalized current ranking when available; otherwise it SHALL create the snapshot, build the semantic graph, and run the configured ranking agent. It SHALL then attach the completed session and open the ranked TUI in a floating terminal connected to the same Neovim RPC server. Selecting an item SHALL hide the popup, open the full captured comparison in Diffview, retain every changed file in its sidebar, and focus the selected file and line. The popup SHALL be resumable without restarting the TUI process. Only one launch SHALL run at a time, and failures SHALL leave Neovim responsive with an actionable error.

The bridge SHALL keep a compact animated status visible throughout the asynchronous launch, identifying snapshot, graph, ranking, and opening stages. It SHALL expose the same current status for statusline integrations and clear active state on success or failure.

#### Scenario: Launch all uncommitted changes
- **WHEN** the reviewer chooses the uncommitted action from the editor mapping group
- **THEN** the bridge captures staged, unstaged, and untracked changes, ranks the session, attaches it, and shows the ranked selector

#### Scenario: Launch current branch against development
- **WHEN** the reviewer chooses the development action
- **THEN** the bridge reviews the merge base of the configured development ref through `HEAD` without fetching or changing refs

#### Scenario: Repeat an unchanged editor launch
- **WHEN** the reviewer launches the same input after its ranking finalized and no selected Git or context content changed
- **THEN** the bridge reattaches the cached session without recapturing, reindexing, or starting an agent

#### Scenario: Ranking takes several minutes
- **WHEN** the ranking agent is still running
- **THEN** Neovim remains responsive and shows an animated ranking-stage indicator until the process completes or fails

#### Scenario: Reviewer chooses one ranked hunk
- **WHEN** the reviewer selects a ranked hunk from a comparison containing multiple files
- **THEN** Diffview shows all captured changed files while focusing the selected hunk rather than filtering the comparison to one path

#### Scenario: Reviewer returns to the ranked queue
- **WHEN** Diffview is showing a selected hunk and the reviewer invokes the ranked-review shortcut
- **THEN** the existing TUI terminal resumes in a popup with its selection and filters intact

### Requirement: Recover from editor failures

Missing, disconnected, or incompatible editor integrations SHALL produce an actionable error without marking the item reviewed or losing drafts. The CLI SHALL still allow source retrieval and Markdown feedback.

#### Scenario: Neovim connection drops
- **WHEN** a queue navigation request cannot reach the editor
- **THEN** the selection and drafts remain recoverable and the reviewer can reconnect or use the terminal workflow

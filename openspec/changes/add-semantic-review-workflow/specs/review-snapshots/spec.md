## Purpose

Capture reproducible review inputs so graph evidence, navigation, ranking, and comments refer to the same source content throughout a session.

## ADDED Requirements

### Requirement: Review all supported Git inputs

The system SHALL accept GitHub PR identifiers or URLs, local branch comparisons, all uncommitted changes, staged changes, and unstaged changes. PR and default branch reviews SHALL compare the merge base to the selected head; explicit two-revision comparisons SHALL compare those revisions directly. An uncommitted review SHALL compare HEAD to the working tree and include staged, unstaged, and untracked content. Staged review SHALL compare HEAD to the index, and unstaged review SHALL compare the index to tracked working-tree content. Including untracked files in an unstaged-only review SHALL be an explicit option.

The CLI SHALL also accept Diffview-style `A..B` as a direct two-revision comparison and `A...B` as a merge-base-to-`B` comparison. Both endpoints SHALL be non-empty and resolved to recorded commit IDs.

#### Scenario: Branch contains unrelated base-branch updates
- **WHEN** a branch review starts after the target branch has advanced
- **THEN** its changes are computed from the merge base and resolved commit IDs are recorded

#### Scenario: File contains both staged and unstaged changes
- **WHEN** the user creates separate staged and unstaged sessions
- **THEN** each session contains only its selected comparison and retains its own before/after content

#### Scenario: Reviewer selects all uncommitted changes
- **WHEN** tracked content has staged and unstaged edits and the checkout has an untracked file
- **THEN** one uncommitted snapshot contains all three without changing the source checkout or index

#### Scenario: Reviewer pastes a Diffview comparison
- **WHEN** review creation receives `feature-A..origin/develop`
- **THEN** it directly compares the resolved `feature-A` and `origin/develop` commits without changing either ref

### Requirement: Preserve immutable source identity and coordinates

Each snapshot SHALL identify the repository, comparison mode, resolved revisions or content digests, paths on both sides, and exact before/after content. Each textual hunk SHALL preserve old/new line ranges and a stable identifier within that snapshot. Renames, deleted files, binary changes, mode changes, and submodule changes SHALL remain represented even when textual or semantic analysis is unavailable.

#### Scenario: Deleted or renamed file
- **WHEN** a snapshot includes a deletion or rename
- **THEN** the old path and source remain available for graph references, display, and left-side comments

#### Scenario: Content changes during capture
- **WHEN** the index or working-tree source changes while a local snapshot is being captured
- **THEN** capture retries within a bounded limit or fails explicitly without publishing an inconsistent snapshot

### Requirement: Leave the source checkout intact

Snapshot capture and indexing SHALL NOT stage, commit, stash, switch branches, edit tracked files, run project builds or dependency-install scripts, or execute Git hooks in the source checkout. Required isolated source material SHALL be created in tool-owned storage. Executable setup hooks SHALL follow the separate explicit configuration contract.

#### Scenario: Dirty Inker checkout
- **WHEN** a review is created for pinned Inker commits while unrelated local edits exist
- **THEN** the edits and index remain byte-for-byte unchanged and the review uses the pinned content

### Requirement: Resume and refresh without false carry-over

Sessions SHALL persist progress, ranking, context identity, and draft comments across process restarts. Refresh SHALL create a new snapshot, mark obsolete rankings stale, and preserve the old session. Review status and anchors SHALL transfer only for unambiguously unchanged content; changed or ambiguous content SHALL require renewed review or anchor resolution.

Review creation SHALL support explicit reuse of the newest session for the same canonical repository, comparison mode, resolved revisions, and exact selected index/worktree/untracked content. Reuse SHALL preserve that session's graph, ranking, progress, and drafts. Content fingerprints SHALL include bytes, not only Git status categories, and SHALL be confirmed before reuse. Normal review creation without reuse SHALL continue to create an independent immutable session.

#### Scenario: PR receives a new commit
- **WHEN** the user refreshes a previously reviewed PR
- **THEN** changed hunks are unreviewed in the new snapshot, and unresolved old comments remain available without being silently assigned to new lines

#### Scenario: Reviewer launches the same unchanged input again
- **WHEN** reusable review creation receives the same input while its selected source bytes and revisions are unchanged
- **THEN** it returns the existing session and reports a cache hit without recapturing source

#### Scenario: Modified file changes while remaining modified
- **WHEN** a tracked working-tree file is edited again without changing its Git status category
- **THEN** reusable review creation detects different content and captures a new session

### Requirement: Isolate sessions and reject conflicting updates

All machine operations SHALL identify their session and applicable revision. Mutations SHALL either commit atomically or return a conflict with no partial write. Different repositories or PRs SHALL NOT share mutable session data accidentally.

The CLI SHALL list recent sessions for the current or selected repository with input, lifecycle stage, file/change counts, ranked count, reviewed count, staleness, and graph identity. The TUI SHALL accept an explicit session or select the newest indexed session for the current repository when omitted.

#### Scenario: Two clients update the same queue revision
- **WHEN** one client saves a manual override after another has advanced that revision
- **THEN** the stale update is rejected and the newer override remains intact

#### Scenario: Reviewer resumes from a repository terminal
- **WHEN** the reviewer runs the TUI without a session ID in a repository with indexed sessions
- **THEN** it opens that repository's newest indexed session without considering sessions from another repository

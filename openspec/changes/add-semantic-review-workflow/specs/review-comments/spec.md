## Purpose

Reuse the existing Neovim commenting workflow while keeping feedback portable in Markdown and precise enough for GitHub inline reviews.

## ADDED Requirements

### Requirement: Reuse the installed comment experience

The Neovim integration SHALL reuse `choplin/code-review.nvim` for adding and editing comments, including normal-mode line comments and visual selections. Integration SHALL persist comments and their source identity beyond the plugin's default in-memory session. Enabling integration SHALL NOT replace unrelated plugin configuration or silently reinterpret existing comments as current-snapshot drafts.

#### Scenario: Existing add-comment mapping
- **WHEN** the reviewer uses `<leader>rc` on a selected review line or range
- **THEN** the existing plugin's comment editor opens and the saved comment becomes a persistent session draft

### Requirement: Preserve complete comment anchors

Each inline draft SHALL identify a stable comment ID, session/snapshot, source side, repository-relative path, line range, and source-content fingerprint. The adapter SHALL translate Diffview buffers to source identities and SHALL NOT store a `diffview://` URI as a GitHub path. Unresolvable anchors SHALL remain explicit orphaned drafts.

#### Scenario: Comment on the left side of a renamed file
- **WHEN** the reviewer comments in the before-side diff buffer
- **THEN** the draft preserves the old path, LEFT side, selected range, and captured source fingerprint

### Requirement: Exchange editable readable Markdown

The system SHALL export a simple `review.md` containing ordered feedback and enough source context to hand to another LLM. It SHALL import supported body edits and new anchored comment sections without losing identity. Machine anchor metadata SHALL remain separate from the readable comment body. Unsupported or damaged metadata SHALL produce a precise conflict rather than attaching a comment by guesswork.

#### Scenario: Edit feedback outside Neovim
- **WHEN** the user edits a known comment body in `review.md` and imports it
- **THEN** the existing draft is updated once with its original anchor and no duplicate is created

#### Scenario: Add freeform feedback without an anchor
- **WHEN** the user adds an unanchored section
- **THEN** it remains review-level feedback and is not fabricated into an inline comment

### Requirement: Detect concurrent and ambiguous edits

Markdown import and plugin updates SHALL detect changes since the last export/read. Divergent edits SHALL require explicit resolution and retain both versions. Reimporting unchanged data SHALL be idempotent. Removing a Markdown section SHALL NOT silently delete a local or published comment.

#### Scenario: TUI session and Markdown both change a draft
- **WHEN** an older Markdown export is imported with a conflicting body edit
- **THEN** neither version is overwritten silently and the conflict identifies the comment

### Requirement: Keep publication and refresh explicit

Saving comments or exporting Markdown SHALL NOT post to GitHub. Comments carried to another snapshot SHALL be marked stale unless their anchors are unambiguously revalidated. Published comment IDs and local drafts SHALL remain distinguishable.

#### Scenario: Old feedback is imported after a PR update
- **WHEN** its original source no longer matches the new snapshot
- **THEN** the draft remains visible as stale and cannot be published inline until resolved

## Purpose

Present attributable explanations, comments, test evidence, and captured review intent beside each change without losing the reviewer’s place.

## ADDED Requirements

### Requirement: Toggle code and context without leaving the selected change

Pressing `i` SHALL toggle the focused item's lower pane between Code and Context in queue and related views. Each mode SHALL retain independent scroll state through toggling, following/backtracking, and Neovim popup hide/resume. Context SHALL expose available explanations, local comments, linked test evidence, and cited source excerpts. `I` SHALL expose session-level context, including material not associated with a particular hunk. Existing navigation, comment, theme, tag, and Diffview shortcuts SHALL remain available and contextual help SHALL document the new keys.

#### Scenario: Read intent and return to the code
- **WHEN** the reviewer scrolls a patch, presses i, reads its context, and presses i again
- **THEN** the same selected item and code position are restored without launching an agent or editor or changing reviewed status

#### Scenario: No hunk-specific context exists
- **WHEN** the selected hunk has no explanation or anchored discussion
- **THEN** the view offers the available add/enrich action and session context without fabricating an association

### Requirement: Store bounded source-backed model explanations

The system SHALL accept explicit batch enrichment with concise explanations, cited evidence, and optional annotations anchored to captured side/path/ranges. Writes SHALL validate item identities, ranges, evidence references, revisions, and text bounds atomically. Model output SHALL be labeled as interpretation and kept separate from manual notes, ranking rationale, and publishable drafts. Explanation-only enrichment SHALL NOT change scores, tags, ranking finalization, progress, or existing comments. Missing or changed inputs SHALL make dependent explanations visibly stale.

#### Scenario: Model highlights a boundary check
- **WHEN** an enrichment batch provides a valid captured range and explanation citing the PR requirement
- **THEN** code preview shows a discoverable emphasis marker and Context shows the concise explanation and captured citation

#### Scenario: Batch contains an invalid or stale annotation
- **WHEN** any annotation refers to another snapshot, an out-of-range line, or missing evidence
- **THEN** the entire batch is rejected with an actionable structured error and prior manual/model content remains intact

### Requirement: Capture and normalize review context with provenance

The system SHALL support PR title/body, PR review summaries and comment threads, local Markdown, and explicitly supplied Folio Markdown exports. Entries SHALL preserve source identity, author when available, capture/update time, digest, truncation/partial state, and original location or URL. Reimporting unchanged content SHALL be idempotent. Explicit refresh failures SHALL retain previous usable captures. Reading local sessions SHALL not require GitHub or Folio availability.

#### Scenario: Review offline using imported documents
- **WHEN** a session contains a PR conversation and an imported Folio Markdown export
- **THEN** Context displays their captured content and provenance offline, with links to the original sources where supplied

#### Scenario: A remote discussion is edited or only partly fetched
- **WHEN** the reviewer explicitly refreshes context
- **THEN** changed content receives a new digest, partial results are identified, and affected explanations or intent-dependent rankings follow their respective stale-state rules

### Requirement: Associate context without guessing anchors or executing content

Exact snapshot-compatible inline anchors SHALL associate with intersecting review items. Ambiguous, outdated, or unmatched anchors SHALL remain inspectable session context with their limitation. Manual or model associations SHALL expose their provenance. Markdown and external text SHALL be inert evidence and SHALL NOT enable commands, hooks, tests, model calls, or publishing. Only explicit reviewer actions SHALL create or publish drafts.

#### Scenario: Old PR comment refers to moved code
- **WHEN** a comment's revision cannot be mapped exactly to the captured change
- **THEN** the original discussion remains visible as outdated context and is not silently attached to a similarly named line

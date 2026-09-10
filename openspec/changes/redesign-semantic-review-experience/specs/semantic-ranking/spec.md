## ADDED Requirements

### Requirement: Name individual changes by semantic meaning

The ranking workflow SHALL request a concise action-oriented title describing each hunk's observable change, distinct from its importance rationale. Titles SHALL be stored against stable hunk identity and graph revision and exposed to queue consumers without replacing existing name, score, rationale, or identity fields. Supplied titles SHALL contain 1–120 Unicode characters after surrounding whitespace is removed, remain single-line, and contain no terminal control characters. Invalid label or score batches SHALL fail atomically. Titles SHALL describe supported evidence without inventing review findings.

#### Scenario: Several changes share one file and symbol
- **WHEN** separate hunks change error handling, tests, and imports
- **THEN** the ranker can assign separate titles describing those changes and the queue retains separate stable hunk identities and completion accounting

#### Scenario: Title contains invalid content
- **WHEN** a batch supplies a blank, overlong, multiline, or terminal-control-containing title
- **THEN** the entire batch is rejected with a structured actionable error and all previous labels and assessments remain intact

### Requirement: Preserve usability of existing rankings without titles

Existing serialized rankings and clients omitting titles SHALL remain accepted. A missing hunk title SHALL use a concise existing rationale when available, otherwise a deterministic symbol/change-kind and file/range label. Queue consumers SHALL be able to distinguish a direct semantic title from a fallback. An inherited symbol title SHALL NOT be represented as a direct hunk title. Missing titles SHALL NOT make a finalized ranking stale or trigger an agent when opening or resuming review.

#### Scenario: Open a previously finalized session
- **WHEN** its assessments have rationales but no titles
- **THEN** meaningful fallback labels appear immediately, all scores and review progress remain unchanged, and no ranking process starts

#### Scenario: Ranking is partial or inherited
- **WHEN** a hunk lacks its own title or direct assessment
- **THEN** it remains discoverable with a fallback label and its actual assessment state, without claiming a model produced a hunk-specific title

### Requirement: Enrich titles independently of review priority

The CLI SHALL provide an explicit title-only ranking operation using the configured external agent and batch label writes checked against current graph revision. Enrichment SHALL preserve existing scores, tags, rationales, manual authority, finalized ranking state, drafts, and review status. It SHALL skip existing direct titles by default and support explicit refresh. A graph revision mismatch SHALL reject stale label writes. No built-in model provider SHALL be required.

#### Scenario: Add titles to a manually adjusted queue
- **WHEN** the reviewer explicitly requests title enrichment on a current finalized ranking
- **THEN** missing hunk titles are filled without changing manual priorities or completion and the next TUI load displays them

#### Scenario: Enrichment is interrupted
- **WHEN** the external agent fails after committing a valid batch
- **THEN** committed titles survive, missing titles still have fallbacks, and the command reports failure without losing the existing review

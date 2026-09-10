## Purpose

Relate changed hunks to source symbols, affected references, and tests while exposing the limits of language-server evidence.

## ADDED Requirements

### Requirement: Represent hunks and symbols without losing changes

The graph SHALL expose changed hunks and source symbols as separately addressable nodes with snapshot, source side, path, and range. It SHALL relate hunks to all overlapping symbols, preserve before/after symbol identities, and retain hunks without a symbol. The immutable raw hunk SHALL remain addressable in every mode. Without an active review-unit projection it remains one independently reviewable change; an active projection MAY present bounded child review units as the completion leaves without deleting or rewriting the parent. Unchanged symbols reached as context SHALL be distinguishable from changed review items.

#### Scenario: One hunk spans several functions
- **WHEN** a hunk overlaps several symbol ranges
- **THEN** all applicable relationships are exposed, the raw hunk remains addressable, and it remains one independently reviewable change unless an explicit active projection presents its bounded children as review leaves

#### Scenario: Non-code or binary change
- **WHEN** a file cannot provide source symbols or textual hunks
- **THEN** its change remains in the inventory with its kind and limitation rather than disappearing

### Requirement: Qualify initial language servers and multiple projects

The first release SHALL support configured LSP servers for TypeScript/JavaScript including TSX/JSX, HTML, and CSS. It SHALL discover or accept project roots and settings independently, negotiate server capabilities, and index the correct before/after snapshot content. Missing methods SHALL be recorded as unsupported, not interpreted as successful empty results.

#### Scenario: Inker frontend and backend have different TypeScript settings
- **WHEN** a snapshot changes both projects
- **THEN** each uses its applicable project configuration, and returned locations identify the correct project and source side

#### Scenario: HTML or CSS server lacks call hierarchy
- **WHEN** the server supports document symbols but not call hierarchy
- **THEN** supported structure is available and call relationships are explicitly unavailable

### Requirement: Preserve relationship types and evidence

Relationships SHALL identify their type, direction, source location, producing mechanism, and confidence. The graph SHALL distinguish containment, hunk overlap, references, calls when supported, and test references. Inferred relationships SHALL NOT be presented as LSP-confirmed facts. The system SHALL NOT claim complete cross-language HTML/CSS/TSX binding or runtime dependency analysis from LSP alone.

#### Scenario: Function changes before and after a rename
- **WHEN** before-side and after-side language servers report different references
- **THEN** both sets remain associated with their own source revisions, and any identity match exposes its confidence

### Requirement: Treat tests as a special reference type

References from test code to a symbol SHALL be queryable as `test_reference` edges, separately from production references. The graph SHALL distinguish directly resolved test references from filename or directory association heuristics. Tests excluded by normal application configuration SHALL still be considered through supported server handling or labeled fallback discovery. A test reference SHALL NOT imply that the test passed or covers a particular changed behavior.

#### Scenario: Adjacent test imports a changed helper
- **WHEN** a test file imports or invokes a changed helper
- **THEN** its resolved reference is available through the test-reference filter even when the application's build excludes tests

#### Scenario: Similar test filename without a resolved reference
- **WHEN** only a naming convention links a test to a changed symbol
- **THEN** the association is labeled heuristic and is distinguishable from a directly resolved reference

### Requirement: Make incomplete analysis visible and bounded

Indexing SHALL expose per-project, per-side, and per-relation coverage, timeouts, unresolved references, and excluded or truncated frontiers. It SHALL enforce configurable time and graph-size budgets, permit cancellation, and retain a reviewable raw-change inventory after partial failure. Coverage gaps SHALL NOT automatically lower importance scores.

Graph building SHALL reuse a persisted graph only when its source, server profile, configuration, and dependency fingerprint remains current, and SHALL report whether it reused the cache. An explicit force option SHALL bypass reuse and publish a new graph revision.

#### Scenario: Server times out during reference discovery
- **WHEN** the configured indexing budget is exhausted
- **THEN** the session remains usable, identifies unfinished work, and supports explicit bounded continuation

#### Scenario: Graph build repeats without relevant changes
- **WHEN** graph building runs again for an unchanged snapshot and analysis configuration
- **THEN** it returns the existing graph revision as a cache hit without relaunching language servers

### Requirement: Match coordinates across tools

Graph ranges SHALL convert negotiated LSP position encodings to exact snapshot text and Git line coordinates. UTF-16 positions, multibyte characters, CRLF content, and empty ranges SHALL NOT move references or anchors to the wrong source location.

#### Scenario: Non-ASCII text precedes a changed symbol
- **WHEN** an LSP range uses UTF-16 offsets in a line containing an emoji
- **THEN** source retrieval and editor navigation select the actual symbol and correct Git lines

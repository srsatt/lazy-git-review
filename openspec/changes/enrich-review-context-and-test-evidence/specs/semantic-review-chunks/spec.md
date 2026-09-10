## Purpose

Divide large captured Git hunks into manageable review units while retaining complete diff content, stable identity, and exact comment anchors.

## ADDED Requirements

### Requirement: Partition large textual hunks deterministically

New sessions SHALL partition hunks above a configurable changed-line threshold into independently addressable review units, preferring symbol/block boundaries and using bounded contiguous fallback segments where necessary. Default threshold and hard ceiling SHALL be 120 owned changed lines with an 80-line target. Every added/deleted patch row SHALL belong to exactly one leaf unit; surrounding context MAY overlap. Units SHALL expose parent identity, part order, exact old/new ranges, and access to the full original hunk. Identical captured inputs and partition settings SHALL produce identical IDs. Binary/metadata changes SHALL retain existing inventory behavior.

#### Scenario: A new file adds three hundred lines
- **WHEN** the file contains several functions or test cases within one Git hunk
- **THEN** the queue contains multiple bounded, meaningfully named review units that collectively own all three hundred added lines and preserve access to the whole file

#### Scenario: One function exceeds the chunk ceiling
- **WHEN** no semantic boundary can satisfy the configured maximum
- **THEN** deterministic fallback chunks remain bounded, identify continuation, and lose no changed rows

### Requirement: Rank and complete leaf units consistently

Each active leaf unit SHALL support its own title, assessment, reviewed status, related links, and context. Parent assessments SHALL remain available as explicitly inherited values until overridden. Queue counts SHALL include active leaves once and exclude aggregate parents and unchanged source context. Parent completion SHALL reflect all children, independently of filtering. Writes SHALL validate the active projection revision.

#### Scenario: Only one of four chunks is reviewed
- **WHEN** a reviewer marks one chunk reviewed and filters out the other three
- **THEN** total completion remains one of four and the parent is not marked fully reviewed

### Requirement: Preserve existing sessions and exact source anchors

Legacy sessions SHALL retain their saved hunk queue until explicit partition migration. Migration SHALL preview mappings, preserve raw hunk IDs/patches and original comments, back up durable state, and reject stale concurrent writes. Exact inherited progress SHALL be carried with provenance; ambiguous repartitioned work SHALL remain unreviewed. Editor opening, source retrieval, Markdown export, and GitHub anchor validation SHALL resolve chunk locations against the immutable parent snapshot rather than fabricated diff offsets.

#### Scenario: Partition a finalized review with existing comments
- **WHEN** the reviewer applies the previewed partition migration
- **THEN** existing comments retain their original side/path/ranges, child assessments expose inheritance, and the recorded parent reviewed state maps deterministically to children

#### Scenario: Comment or open a renamed deletion chunk
- **WHEN** the reviewer selects lines from a deletion on the old side of a renamed file
- **THEN** editor and comment operations resolve the exact captured old path and lines without shifting to the new file's coordinates

## ADDED Requirements

### Requirement: Present semantic hunk names as primary review content

Queue and related-change rows SHALL prioritize the semantic title with visible review state, importance, and compact filename/line context. Paths SHALL appear once per row and preserve their distinguishing suffix when shortened. Distinct hunks in one file SHALL remain distinguishable by title or range. Tags, relation type, and incomplete assessment state SHALL remain accessible without repeating raw node metadata in every row. Long lists SHALL scroll to keep keyboard selection visible, including after resize and filtering.

#### Scenario: Review many hunks in a deeply nested file
- **WHEN** a queue contains 32 hunks with the same long path prefix
- **THEN** titles and line locations distinguish the rows at 80-column and 120-column widths without requiring horizontal navigation through paths

#### Scenario: Move beyond the visible list
- **WHEN** the reviewer selects an item below the current viewport or resizes the terminal
- **THEN** the selected row stays visible and keyboard focus, identity, and completion state remain correct

### Requirement: Navigate directly between related changed hunks

From either the ranked queue or a related-change list, `l` or Right SHALL show related targets for the selected review leaf. Related rows SHALL represent deduplicated changed hunks or active review units with semantic titles and previews, plus explicitly labeled unchanged test/source context when backed by static or compatible runtime evidence. Targets SHALL be grouped into tests, usages/imports, definitions, calls, or explicitly labeled symbol context. Each relation SHALL retain inspectable direction and supporting graph evidence. Multiple static/runtime routes to one target SHALL produce one target row with aggregated evidence. Same-file membership alone SHALL NOT establish a semantic relation.

Changing list selection SHALL preview that changed or unchanged target without opening an editor. Repeating `l` SHALL follow the selected target; `h`, Left, or Escape SHALL restore the previous center and selection, and at the root return to the queue. Returning SHALL restore filters and preview/list offsets. Reading or following a relation SHALL NOT mark content reviewed. Raw graph evidence and related unchanged source SHALL remain inspectable and distinct from changed-leaf completion.

#### Scenario: Follow implementation to a changed test and back
- **WHEN** an indexed reference connects the selected implementation hunk to a changed test hunk
- **THEN** one press of `l` reveals that test with its semantic title, selecting it shows the captured test patch, another `l` follows its relations, and `h` restores the prior review position

#### Scenario: Many references map to one test hunk
- **WHEN** several reference locations and before/after evidence paths resolve to the same captured test change
- **THEN** it appears once in related changes with its supporting routes inspectable

#### Scenario: A relation points outside changed hunks
- **WHEN** a usage or definition exists only in unchanged source
- **THEN** it is available as source context with a symbol or basename, side, line, and excerpt, without being presented as a changed or reviewable hunk

#### Scenario: Analysis has incomplete coverage
- **WHEN** no related changes are indexed and graph coverage is partial
- **THEN** the view explains the indexed-result limitation and permits return to the queue without implying there are no tests or dependencies

### Requirement: Preview captured changes inside the TUI

Queue and related views SHALL include a scrollable diff preview of the selected hunk from the immutable session snapshot. At terminal sizes of at least 100 columns by 30 rows the preview SHALL show at least ten code lines in addition to metadata; at 80 by 24 it SHALL retain at least six code lines and a usable list. The preview SHALL show side-aware line coordinates, added/deleted/context markers, and a discoverable scroll position. Tab SHALL switch list/preview focus; j/k and paging SHALL act on the focused pane; `[` and `]` SHALL scroll preview without moving list selection.

Deletion-only, added, renamed, non-text, oversized, and unavailable-source cases SHALL have accurate source identity and appropriate patch or metadata presentation. Long/control-character-containing content SHALL not corrupt terminal rendering. Large previews SHALL indicate bounded content and permit continuation. Errors SHALL retain selection and allow navigation or explicit editor opening.

#### Scenario: Review without opening Diffview
- **WHEN** a reviewer selects an implementation hunk, scrolls its patch, then selects a related test
- **THEN** both captured patches can be inspected and marked reviewed entirely in the TUI without launching an editor or model

#### Scenario: Source changes after capture
- **WHEN** working-tree content differs from the selected review snapshot
- **THEN** the preview still shows the captured bytes and original old/new coordinates

#### Scenario: Preview contains a deletion or binary change
- **WHEN** the selection is deletion-only or non-text
- **THEN** the preview shows the correct old-side diff or captured non-text change metadata instead of an unrelated right-side file or blank pane

### Requirement: Choose tags explicitly

Pressing `t` SHALL open a searchable multi-select picker containing available built-in and user-defined tags and counts. Space SHALL toggle the highlighted tag, Enter SHALL apply, Escape SHALL cancel without changing the active filter, and an explicit All tags choice SHALL clear it. Multiple selected tags SHALL combine with OR; tag and status filters SHALL combine with AND. Counts SHALL reflect the current status-filtered collection before tag filtering. Modal keystrokes SHALL not invoke background review actions.

Applying filters SHALL retain the selected hunk when visible, otherwise select the first result. Empty results SHALL provide a clear-filter action. Filtered items SHALL remain included in total review completion; hiding/resuming the popup SHALL preserve active filters.

#### Scenario: Select tests and non-trivial changes
- **WHEN** the reviewer checks tests and non-trivial-logic and applies the picker
- **THEN** hunks carrying either tag are visible subject to the status filter and overall completion counts remain unchanged

#### Scenario: Cancel a filter edit
- **WHEN** the reviewer changes pending selections and presses Escape
- **THEN** the prior filter and selected hunk remain unchanged and no hunk is marked reviewed or opened

### Requirement: Provide readable themed and monochrome review

The default review theme SHALL use a Night Owl-inspired palette with distinct focus, importance, relation, addition, deletion, and error roles. Essential meaning SHALL also be expressed by text, markers, grouping, and focus treatment. `T` SHALL select among bundled presets and named custom palettes declared as semantic color roles in the existing JSON settings; the configured `tui.theme` SHALL choose the startup palette and `terminal` SHALL remain available. Theme selection SHALL not require network access. `--no-color` or nonempty `NO_COLOR` SHALL disable custom colors; limited-color terminals SHALL receive a supported fallback. Unicode display width SHALL govern alignment and clipping.

#### Scenario: Review with color disabled
- **WHEN** the user starts the TUI with NO_COLOR set or --no-color
- **THEN** selection, added/deleted code, relation categories, and review state remain distinguishable without custom foreground/background colors

#### Scenario: Select a configured custom theme
- **WHEN** settings declare a named semantic palette and the reviewer chooses it with `T`
- **THEN** the review redraws with that palette without restarting, downloading data, or changing review/navigation state

#### Scenario: Run with redirected input or a dumb terminal
- **WHEN** TUI input/output is not an interactive terminal or TERM is dumb
- **THEN** it fails promptly before entering raw mode with an actionable structured error pointing to noninteractive queue/hunk commands

### Requirement: Resume the ranked queue directly from Diffview

The shipped Diffview integration SHALL offer configurable `gq` mappings in diff buffers and the file panel, described in Diffview's `g?` help. Mapping installation SHALL be scoped to Diffview and preserve unrelated or explicitly customized mappings. Invoking `gq` SHALL reopen the existing session popup, preserve review mode/selection/filters/preview position, and restore terminal input focus without starting another ranking process. The existing leader shortcut and queue command SHALL remain valid.

#### Scenario: Return from either comparison pane
- **WHEN** a reviewer presses gq in either diff pane or the file panel
- **THEN** the same review popup resumes in the same Neovim instance and j/k immediately controls its focused pane

#### Scenario: Existing user mapping conflicts
- **WHEN** an explicit custom Diffview gq mapping exists
- **THEN** default integration does not silently overwrite it and a configurable mapping or the queue command remains available

### Requirement: Keep review navigation responsive and recoverable

With a prebuilt graph containing at least 10,000 nodes, 10,000 edges, and 100 active review leaves (raw hunks or projected units) on the recorded acceptance host, warm startup SHALL reach a useful first frame within one second, the first captured preview SHALL display within 100 ms, and p95 cached selection/follow/back-to-render latency SHALL be at most 50 ms across at least 200 actions. Acceptance SHALL report wall-clock timings, fixture size, and host rather than claim latency from aggregate CLI query time.

Ordinary navigation SHALL not launch LSP servers, agents, network requests, or repeated database queries. Bounded preview loading SHALL keep key handling responsive and expose errors without losing review work. Presentation-only actions SHALL not rewrite durable progress. Normal exit, Ctrl+C, and supported termination signals SHALL restore terminal state and preserve already saved progress. Contextual help SHALL expose navigation, filtering, preview, editor, and exit keys.

#### Scenario: Repeated cached review traversal
- **WHEN** the reviewer performs 200 selections, relation follows, and back operations in the acceptance fixture
- **THEN** measured p95 meets the latency bound and process/IO evidence shows no model, LSP, network, or per-step database work

#### Scenario: Exit during preview loading
- **WHEN** the reviewer cancels while a bounded preview load is pending
- **THEN** the TUI exits promptly, restores the terminal, and retains previously saved review status and drafts

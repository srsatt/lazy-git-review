## Purpose

Make captured code readable inside review previews while preserving exact diff coordinates, theme compatibility, and fast navigation.

## ADDED Requirements

### Requirement: Highlight captured source on the correct side

The preview SHALL provide syntax highlighting for TypeScript, TSX, JavaScript, JSX, HTML, and CSS. Deleted text SHALL use the captured left source and added text the captured right source, including multiline syntax context. Styling SHALL preserve old/new line coordinates, changed-line markers, and source bytes independently from display sanitization. Unchanged source previews SHALL receive the same treatment.

#### Scenario: Multiline syntax crosses a hunk boundary
- **WHEN** a selected change starts inside a captured multiline comment or string
- **THEN** syntax classification reflects that file's captured state rather than treating the patch fragment as a new file

#### Scenario: Working tree differs from captured code
- **WHEN** the working tree changes after capture
- **THEN** highlighted text and navigation still use the captured side and coordinates, including CRLF and multibyte characters

### Requirement: Compose syntax with review themes and annotations

Syntax colors SHALL respect the selected built-in or custom theme, retain addition/deletion and focus meaning, and remain distinct from optional model annotation emphasis. Existing palettes without syntax roles SHALL continue to load. Color opt-out SHALL remove custom colors while retaining all textual diff and annotation markers. Users SHALL be able to disable syntax highlighting independently.

#### Scenario: Switch theme with an annotated deletion visible
- **WHEN** the reviewer selects another theme or disables color
- **THEN** deleted content, annotation presence, and focus remain identifiable and selection and scroll positions remain unchanged

### Requirement: Fall back without blocking review

Unknown languages, unavailable grammars, malformed source, oversized files, and highlight failures SHALL retain a usable plain captured preview. Highlighting SHALL be bounded and cancellable, and ordinary cached navigation SHALL meet the existing 50 ms p95 target. A useful plain preview SHALL remain available within the existing 100 ms target; supported selected files up to 256 KiB SHALL highlight within 250 ms after their source is available on the recorded acceptance host.

#### Scenario: Highlight an oversized or unsupported file
- **WHEN** syntax work exceeds the supported source budget or language coverage
- **THEN** the reviewer can immediately scroll, follow relations, or open the full captured file without waiting for highlighting

## Purpose

Connect local review drafts to GitHub PR discussions and explicit review submission without losing source anchors or duplicating publication.

## ADDED Requirements

### Requirement: Read PR review context through existing authentication

The GitHub integration SHALL use a configured GitHub CLI adapter and its authentication to resolve PR identity, base/head revisions, existing review comments, and review summaries. It SHALL preserve remote IDs, authors, source coordinates, and outdated state. Pagination SHALL not omit available discussion silently. It SHALL support repository forks and explicit host/repository identity.

#### Scenario: Import comments from a fork PR
- **WHEN** the selected PR has existing inline comments across multiple pages
- **THEN** comments are associated with the correct base repository, PR, remote IDs, and available source revisions

### Requirement: Preview and explicitly submit reviews

The user SHALL be able to preview and explicitly submit a COMMENT, APPROVE, or REQUEST_CHANGES review with a summary and supported inline drafts. Before mutation the integration SHALL verify the authenticated account against the configured expected identity. Account commands and identities SHALL be supplied only through user-owned configuration, without switching global authentication. Ranking, navigation, and comment editing SHALL never submit a review automatically.

#### Scenario: Submit selected drafts
- **WHEN** the user explicitly submits a previewed review
- **THEN** only the selected drafts and summary are sent, and the returned review/comment IDs are recorded

#### Scenario: Authentication identity differs
- **WHEN** the adapter's authenticated identity differs from the expected account
- **THEN** submission fails before mutation and explains the mismatch

### Requirement: Resolve account-bound GitHub profiles without global switching

The system SHALL support versioned user-owned settings mapping repository identity globs to structured GitHub CLI commands, expected accounts, and optional canonical API hosts for SSH aliases. It SHALL automatically select exactly one matching profile or accept an explicit named override. Profiles SHALL store no authentication token and profile resolution SHALL NOT switch global GitHub authentication or modify global Git configuration. No match, an ambiguous match, an invalid profile, or an identity mismatch SHALL fail before mutation.

#### Scenario: Repositories use different account commands
- **WHEN** repository identities match distinct configured profiles
- **THEN** each GitHub operation invokes its matched command and verifies its expected account without changing another profile's authentication

#### Scenario: Repository profiles overlap
- **WHEN** two configured patterns match the same base repository and no explicit profile is supplied
- **THEN** the operation reports the matching profile names and performs no GitHub request

### Requirement: Launch GitHub CLI extensions through directory-matched profiles

The system SHALL derive a host/repository identity from an explicit local Git remote and run arbitrary GitHub CLI extension arguments through the uniquely matched structured profile command. It SHALL verify `expected_account` before replacing itself with the interactive command. Missing remotes, unsupported URLs, unmatched or ambiguous profiles, failed identity checks, and identity mismatches SHALL stop before the extension starts. It SHALL NOT switch global authentication or fall back to bare `gh`.

#### Scenario: Neovim opens Git Dash from a matched repository
- **WHEN** the configured Git Dash bridge starts from a repository whose remote matches a profile
- **THEN** it verifies and runs that profile's command without reading or changing another profile's authentication

#### Scenario: Neovim changes repository while dashboard is hidden
- **WHEN** the reviewer invokes Git Dash after moving from one Git root to another
- **THEN** the bridge closes the old dashboard process and starts a newly resolved profile for the new root

### Requirement: Validate anchors and PR freshness

Submission SHALL re-fetch the PR head and validate each inline anchor against the reviewed comparison and GitHub-supported diff coordinates. LEFT/RIGHT side and supported single/multi-line ranges SHALL be preserved. A stale head or invalid anchor SHALL block submission before creating the review. The user SHALL be able to explicitly convert an unsuitable inline draft into summary feedback; the tool SHALL NOT do this silently.

#### Scenario: PR head advances after preview
- **WHEN** submission detects a different head commit
- **THEN** it preserves the drafts and requires refresh and a new preview

#### Scenario: Inline draft targets unavailable diff context
- **WHEN** a draft cannot be represented as a valid GitHub inline location
- **THEN** the preview identifies that draft and offers explicit repair or conversion to review-level feedback

### Requirement: Reconcile uncertain publication without duplicates

The system SHALL persist submission intent before mutation and record acknowledged remote results. After a timeout or interrupted response it SHALL reconcile remote state before retrying. If the outcome cannot be determined uniquely, it SHALL report an uncertain submission and require user resolution rather than issuing another mutation. Local drafts SHALL survive authentication, network, permission, and API failures.

#### Scenario: GitHub accepts review but response is lost
- **WHEN** the user resumes the interrupted submission
- **THEN** the integration queries the recorded target and reconciles an identifiable remote review instead of blindly publishing it again

### Requirement: Separate local and remote review capabilities

Offline local review and Markdown export SHALL remain usable when GitHub is unavailable. Remote editing, replying, and resolving threads SHALL not be implied by importing them; the first release's publication contract SHALL cover new reviews and drafts only.

#### Scenario: GitHub access is unavailable during local review
- **WHEN** the reviewer continues inspecting local snapshot content
- **THEN** navigation, ranking retrieval, draft editing, and Markdown export continue, while remote refresh/submission report their unavailable state

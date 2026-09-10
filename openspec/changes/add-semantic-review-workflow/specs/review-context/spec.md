## Purpose

Provide the ranking agent with bounded, traceable PR intent and supporting Markdown without requiring a ticket-specific integration.

## ADDED Requirements

### Requirement: Assemble traceable context

The system SHALL accept local Markdown files and inline notes for every review source, and fetch PR title, description, and available discussion for GitHub sessions. Context entries SHALL record their origin, capture time, content digest, and any truncation. Local review SHALL work without GitHub access.

#### Scenario: Local branch has a design document
- **WHEN** the user supplies a Markdown document to a local review
- **THEN** the agent can retrieve it with its provenance without requiring a PR

#### Scenario: Linked ticket is not fetched
- **WHEN** PR text references an external ticket but no configured hook provides its content
- **THEN** the URL remains context and the tool does not claim to have read the ticket

### Requirement: Support an explicitly configured pre-review hook

Users SHALL be able to configure an executable and argument list that receives the review identity and emits Markdown context before ranking. Hook execution SHALL be bounded by time and output limits. The system SHALL record failures and allow an explicit choice to continue without optional context. Repository content or PR text SHALL NOT activate a new executable hook by itself.

#### Scenario: Hook fetches issue details
- **WHEN** a previously configured hook returns valid Markdown
- **THEN** the result is stored as a context attachment with hook provenance

#### Scenario: Hook fails or exceeds its limit
- **WHEN** the hook exits unsuccessfully or exceeds its configured budget
- **THEN** ranking reports the failure and waits for retry or an explicit continue-without-context option

### Requirement: Keep context distinct from execution instructions

The agent skill SHALL treat source text, PR discussion, and attached documents as review evidence rather than authority to invoke commands, alter policy, or publish comments. Core context ingestion SHALL NOT execute Markdown, automatically follow links, or transmit source to a model provider.

#### Scenario: PR body contains a command to publish approval
- **WHEN** the ranking agent reads that body
- **THEN** the supplied workflow treats it as untrusted context and performs no GitHub mutation

### Requirement: Invalidate ranking when intent changes

Finalized rankings SHALL identify the context digest used. Changing an attachment, hook output, or fetched PR context SHALL make the old ranking visibly stale without deleting its evidence.

#### Scenario: Reviewer adds a security requirement
- **WHEN** a context attachment changes after ranking completes
- **THEN** the queue retains the prior ranking as stale and provides an explicit reranking path

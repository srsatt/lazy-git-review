## Purpose

Connect review changes to observed test execution with honest attribution, snapshot validity, and navigable unchanged test context.

## ADDED Requirements

### Requirement: Run configured tests explicitly with bounded lifecycle

The system SHALL support explicit test execution using a configured executable and arguments, selected snapshot side, project, test selection, timeout, and output/report limits. A dry run SHALL show the resolved command and scope. Runs SHALL use disposable captured content, preserve immutable snapshots and the user's source/index, report progress, and terminate child processes on cancellation. Missing dependencies SHALL produce a recoverable error; normal browsing or repository context SHALL NOT launch tests or install dependencies.

#### Scenario: Cancel a slow selected test run
- **WHEN** the reviewer cancels before completion
- **THEN** the child process group terminates, the outcome is cancelled/partial, bounded output remains inspectable, and existing review progress and evidence are retained

### Requirement: Import coverage with explicit attribution precision

Coverage import SHALL accept normalized attributed manifests and Istanbul JSON/LCOV reports. Evidence SHALL identify snapshot side, source hashes, runner/config/dependency identity, outcome, selected scope, covered ranges, and attribution granularity. Case-level, file-level, and suite-level attribution SHALL be distinguishable. Combined coverage lacking test identity SHALL NOT generate individual-test or test-file links. Execution hits SHALL NOT imply assertion coverage, passing outcome, or correctness; unknown and incomplete evidence SHALL remain distinct from observed zero hits.

#### Scenario: Import a combined coverage report
- **WHEN** a report contains line counts for a whole suite without attributed tests
- **THEN** coverage is visible as suite evidence and no specific test is invented

#### Scenario: Collect coverage from isolated test-file runs
- **WHEN** each report is associated with exactly one executed test file and a matching snapshot
- **THEN** links identify that test file, preserve run outcomes, and do not claim which individual test case produced the hits

### Requirement: Require matching captured coordinates and freshness

Runtime links SHALL require original-source ranges and source identities matching the selected snapshot side. Unmapped transpiled positions, mismatched revisions, stale reports, and right-side evidence for deleted left-side lines SHALL remain diagnostics rather than current links. Completed results SHALL be reused only when snapshot, test selection, runner, configuration, dependency, and source fingerprints match; force SHALL rerun explicitly. Static graph/index coverage and runtime coverage SHALL retain separate identities and validity.

#### Scenario: Coverage was collected before a source edit
- **WHEN** imported source hashes differ from the selected captured source
- **THEN** the report is marked incompatible/stale and contributes no current runtime edges

### Requirement: Navigate both changed and unchanged linked tests

Observed runtime ranges SHALL link intersecting changed hunks or chunks to their attributed test targets. Related navigation SHALL distinguish runtime execution, static references, and naming heuristics while aggregating duplicate targets and retaining provenance. An unchanged test SHALL be navigable with captured source and available test location/name, without becoming a completion item. A changed test SHALL preview its applicable changed units. Runtime links SHALL be traversable in both directions.

#### Scenario: Production code changes and its test file does not
- **WHEN** current attributed coverage connects the production chunk to the unchanged test file
- **THEN** l exposes that test as context, selection previews its captured source, and Back restores the production chunk without changing completion totals

#### Scenario: Failed tests still executed the changed code
- **WHEN** a matching run failed after recording hits
- **THEN** the observed link remains inspectable with its failed outcome and is not presented as a successful verification

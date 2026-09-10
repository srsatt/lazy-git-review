use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::graph::{ChangeGraph, EdgeKind, GraphNode, NodeKind, SourceLocation};
use crate::position::TextRange;
use crate::ranking::Queue;
use crate::snapshot::Snapshot;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Direction {
    Incoming,
    Outgoing,
}

impl Direction {
    pub(super) fn marker(self) -> &'static str {
        match self {
            Self::Incoming => "←",
            Self::Outgoing => "→",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct RelatedHunk {
    pub(super) node_id: String,
    pub(super) title: String,
    pub(super) path: Option<String>,
    pub(super) line: Option<u32>,
    pub(super) score: Option<u8>,
    pub(super) tags: Vec<String>,
    pub(super) categories: BTreeSet<EdgeKind>,
    pub(super) directions: BTreeSet<Direction>,
    pub(super) evidence_ids: BTreeSet<String>,
    pub(super) evidence_kinds: BTreeSet<String>,
    pub(super) via_symbols: BTreeSet<String>,
    pub(super) confidence: u8,
    pub(super) symbol_context: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum LinkedTestKind {
    Changed,
    Before,
    After,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LinkedTest {
    pub(super) node_id: String,
    pub(super) title: String,
    pub(super) path: Option<String>,
    pub(super) line: Option<u32>,
    pub(super) kind: LinkedTestKind,
    pub(super) granularity: Option<String>,
    pub(super) status: Option<String>,
}

impl LinkedTest {
    pub(super) fn kind_label(&self) -> &'static str {
        match self.kind {
            LinkedTestKind::Changed => "changed",
            LinkedTestKind::Before => "before",
            LinkedTestKind::After => "after",
        }
    }
}

impl RelatedHunk {
    pub(super) fn primary_label(&self) -> &'static str {
        if self.symbol_context {
            return "symbol ctx";
        }
        self.categories
            .iter()
            .min_by_key(|kind| relation_rank(kind))
            .map(relation_label)
            .unwrap_or("related")
    }

    pub(super) fn direction_marker(&self) -> &'static str {
        if self.directions.len() > 1 {
            "↔"
        } else {
            self.directions
                .iter()
                .next()
                .copied()
                .unwrap_or(Direction::Outgoing)
                .marker()
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct SourceContext {
    pub(super) node_id: String,
    pub(super) relation: EdgeKind,
    pub(super) direction: Direction,
    pub(super) symbol: String,
    pub(super) path: String,
    pub(super) side: String,
    pub(super) line: u32,
    pub(super) evidence_id: String,
    pub(super) confidence: u8,
    pub(super) source_file: Option<PathBuf>,
}

pub(super) struct Index {
    nodes: BTreeMap<String, GraphNode>,
    related: BTreeMap<String, Vec<RelatedHunk>>,
    source_context: BTreeMap<String, Vec<SourceContext>>,
    linked_tests: BTreeMap<String, Vec<LinkedTest>>,
}

impl Index {
    pub(super) fn new(graph: &ChangeGraph, queue: &Queue, snapshot: &Snapshot) -> Self {
        let queue_items: BTreeMap<_, _> = queue
            .items
            .iter()
            .map(|item| (item.node_id.as_str(), item))
            .collect();
        let review_nodes: Vec<_> = graph
            .nodes
            .values()
            .filter(|node| queue_items.contains_key(node.id.as_str()))
            .collect();
        let mut parent_leaves: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for node in &review_nodes {
            for parent in &node.hunk_ids {
                parent_leaves
                    .entry(parent)
                    .or_default()
                    .push(node.id.clone());
            }
        }
        let mut mapped_hunks: BTreeMap<String, (Vec<String>, bool)> = BTreeMap::new();
        for node in graph.nodes.values() {
            let mut ids: BTreeSet<String> = match node.kind {
                NodeKind::Hunk | NodeKind::ReviewUnit
                    if queue_items.contains_key(node.id.as_str()) =>
                {
                    [node.id.clone()].into_iter().collect()
                }
                NodeKind::Test => [node.id.clone()].into_iter().collect(),
                NodeKind::Symbol => node
                    .hunk_ids
                    .iter()
                    .flat_map(|parent| {
                        parent_leaves
                            .get(parent.as_str())
                            .cloned()
                            .unwrap_or_else(|| vec![parent.clone()])
                    })
                    .collect(),
                _ => BTreeSet::new(),
            };
            let mut contextual = false;
            if ids.is_empty() {
                for location in &node.locations {
                    ids.extend(hunks_at_location(&review_nodes, location));
                    if ids.is_empty() {
                        ids.extend(hunks_via_smallest_symbol(graph, location));
                        contextual = !ids.is_empty();
                    }
                }
            }
            mapped_hunks.insert(node.id.clone(), (ids.into_iter().collect(), contextual));
        }

        let mut aggregates: BTreeMap<(String, String), RelatedHunk> = BTreeMap::new();
        let mut source_context: BTreeMap<String, Vec<SourceContext>> = BTreeMap::new();
        for edge in graph.edges.iter().filter(|edge| is_semantic(&edge.kind)) {
            let (from_hunks, from_contextual) =
                mapped_hunks.get(&edge.from).cloned().unwrap_or_default();
            let (to_hunks, to_contextual) = mapped_hunks.get(&edge.to).cloned().unwrap_or_default();
            for source in &from_hunks {
                for target in &to_hunks {
                    add_related(
                        &mut aggregates,
                        source,
                        target,
                        RelationRoute {
                            kind: &edge.kind,
                            direction: Direction::Outgoing,
                            edge,
                            graph,
                            queue: &queue_items,
                            contextual: from_contextual || to_contextual,
                        },
                    );
                }
            }
            for source in &to_hunks {
                for target in &from_hunks {
                    add_related(
                        &mut aggregates,
                        source,
                        target,
                        RelationRoute {
                            kind: &edge.kind,
                            direction: Direction::Incoming,
                            edge,
                            graph,
                            queue: &queue_items,
                            contextual: from_contextual || to_contextual,
                        },
                    );
                }
            }
            if from_hunks.is_empty() {
                add_source_context(
                    &mut source_context,
                    &to_hunks,
                    graph.nodes.get(&edge.from),
                    &edge.kind,
                    Direction::Incoming,
                    edge,
                    snapshot,
                );
            }
            if to_hunks.is_empty() {
                add_source_context(
                    &mut source_context,
                    &from_hunks,
                    graph.nodes.get(&edge.to),
                    &edge.kind,
                    Direction::Outgoing,
                    edge,
                    snapshot,
                );
            }
        }

        let mut related: BTreeMap<String, Vec<RelatedHunk>> = BTreeMap::new();
        for ((source, _), item) in aggregates {
            related.entry(source).or_default().push(item);
        }
        for items in related.values_mut() {
            items.sort_by(|left, right| {
                let left_relation = left
                    .symbol_context
                    .then_some(4)
                    .or_else(|| left.categories.iter().map(relation_rank).min())
                    .unwrap_or(255);
                let right_relation = right
                    .symbol_context
                    .then_some(4)
                    .or_else(|| right.categories.iter().map(relation_rank).min())
                    .unwrap_or(255);
                left_relation
                    .cmp(&right_relation)
                    .then_with(|| right.score.unwrap_or(0).cmp(&left.score.unwrap_or(0)))
                    .then_with(|| left.node_id.cmp(&right.node_id))
            });
        }
        for items in source_context.values_mut() {
            items.sort_by(|left, right| {
                relation_rank(&left.relation)
                    .cmp(&relation_rank(&right.relation))
                    .then_with(|| left.path.cmp(&right.path))
                    .then_with(|| left.line.cmp(&right.line))
                    .then_with(|| left.node_id.cmp(&right.node_id))
            });
            items.dedup_by(|left, right| {
                left.node_id == right.node_id
                    && left.relation == right.relation
                    && left.direction == right.direction
            });
        }

        let linked_tests = build_linked_tests(graph, queue, &related);
        Self {
            nodes: graph.nodes.clone(),
            related,
            source_context,
            linked_tests,
        }
    }

    pub(super) fn node(&self, id: &str) -> Option<&GraphNode> {
        self.nodes.get(id)
    }

    pub(super) fn related(&self, id: &str) -> &[RelatedHunk] {
        self.related.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub(super) fn source_context(&self, id: &str) -> &[SourceContext] {
        self.source_context
            .get(id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub(super) fn linked_tests(&self, id: &str) -> &[LinkedTest] {
        self.linked_tests.get(id).map(Vec::as_slice).unwrap_or(&[])
    }
}

fn build_linked_tests(
    graph: &ChangeGraph,
    queue: &Queue,
    related: &BTreeMap<String, Vec<RelatedHunk>>,
) -> BTreeMap<String, Vec<LinkedTest>> {
    let mut result = BTreeMap::new();
    for (source, items) in related {
        let mut links = Vec::new();
        let mut seen = BTreeSet::new();
        let runtime_paths: BTreeSet<_> = items
            .iter()
            .filter(|item| item.categories.contains(&EdgeKind::RuntimeTest))
            .filter_map(|item| item.path.clone())
            .collect();

        for item in items {
            if item.categories.contains(&EdgeKind::RuntimeTest) {
                let location = graph
                    .nodes
                    .get(&item.node_id)
                    .and_then(GraphNode::preferred_review_location);
                let kind = match location.map(|location| location.side) {
                    Some(crate::graph::SourceSide::Left) => LinkedTestKind::Before,
                    _ => LinkedTestKind::After,
                };
                push_link(
                    &mut links,
                    &mut seen,
                    LinkedTest {
                        node_id: item.node_id.clone(),
                        title: item.title.clone(),
                        path: item.path.clone(),
                        line: item.line,
                        kind,
                        granularity: graph
                            .nodes
                            .get(&item.node_id)
                            .and_then(|node| node.symbol_kind.clone()),
                        status: item.evidence_kinds.iter().find_map(|value| {
                            value
                                .rsplit_once(':')
                                .map(|(_, status)| status.to_ascii_lowercase())
                                .filter(|status| {
                                    matches!(
                                        status.as_str(),
                                        "passed"
                                            | "failed"
                                            | "cancelled"
                                            | "timedout"
                                            | "partial"
                                            | "skipped"
                                            | "error"
                                    )
                                })
                        }),
                    },
                );
            }

            let changed_test = item.path.as_deref().is_some_and(crate::graph::is_test_path)
                && graph
                    .nodes
                    .get(&item.node_id)
                    .is_some_and(|node| matches!(node.kind, NodeKind::Hunk | NodeKind::ReviewUnit))
                && item.categories.contains(&EdgeKind::TestReference);
            if changed_test {
                push_changed_link(&mut links, &mut seen, item);
            }
        }

        for item in &queue.items {
            let Some(path) = item.path.as_deref() else {
                continue;
            };
            if item.node_id != *source
                && crate::graph::is_test_path(path)
                && runtime_paths.contains(path)
            {
                let node = graph.nodes.get(&item.node_id);
                push_link(
                    &mut links,
                    &mut seen,
                    LinkedTest {
                        node_id: item.node_id.clone(),
                        title: item.title.clone(),
                        path: Some(path.to_owned()),
                        line: node
                            .and_then(GraphNode::preferred_review_location)
                            .map(|location| location.range.start.line + 1),
                        kind: LinkedTestKind::Changed,
                        granularity: None,
                        status: None,
                    },
                );
            }
        }

        links.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.title.cmp(&right.title))
                .then_with(|| left.node_id.cmp(&right.node_id))
        });
        if !links.is_empty() {
            result.insert(source.clone(), links);
        }
    }
    result
}

fn push_changed_link(
    links: &mut Vec<LinkedTest>,
    seen: &mut BTreeSet<(LinkedTestKind, String)>,
    item: &RelatedHunk,
) {
    push_link(
        links,
        seen,
        LinkedTest {
            node_id: item.node_id.clone(),
            title: item.title.clone(),
            path: item.path.clone(),
            line: item.line,
            kind: LinkedTestKind::Changed,
            granularity: None,
            status: None,
        },
    );
}

fn push_link(
    links: &mut Vec<LinkedTest>,
    seen: &mut BTreeSet<(LinkedTestKind, String)>,
    link: LinkedTest,
) {
    if seen.insert((link.kind, link.node_id.clone())) {
        links.push(link);
    }
}

struct RelationRoute<'a> {
    kind: &'a EdgeKind,
    direction: Direction,
    edge: &'a crate::graph::GraphEdge,
    graph: &'a ChangeGraph,
    queue: &'a BTreeMap<&'a str, &'a crate::ranking::QueueItem>,
    contextual: bool,
}

fn add_related(
    aggregates: &mut BTreeMap<(String, String), RelatedHunk>,
    source: &str,
    target: &str,
    route: RelationRoute<'_>,
) {
    if source == target {
        return;
    }
    let Some(target_node) = route.graph.nodes.get(target) else {
        return;
    };
    let queue_item = route.queue.get(target).copied();
    let location = target_node.preferred_review_location();
    let item = aggregates
        .entry((source.to_owned(), target.to_owned()))
        .or_insert_with(|| RelatedHunk {
            node_id: target.to_owned(),
            title: queue_item
                .map(|item| item.title.clone())
                .unwrap_or_else(|| target_node.name.clone()),
            path: location.map(|location| location.path.display.clone()),
            line: location.map(|location| location.range.start.line + 1),
            score: queue_item.and_then(|item| item.score),
            tags: queue_item.map(|item| item.tags.clone()).unwrap_or_default(),
            categories: BTreeSet::new(),
            directions: BTreeSet::new(),
            evidence_ids: BTreeSet::new(),
            evidence_kinds: BTreeSet::new(),
            via_symbols: BTreeSet::new(),
            confidence: 0,
            symbol_context: route.contextual,
        });
    item.symbol_context &= route.contextual;
    item.categories.insert(route.kind.clone());
    item.directions.insert(route.direction);
    item.evidence_ids.insert(route.edge.id.clone());
    item.evidence_kinds.insert(route.edge.evidence_kind.clone());
    for node_id in [&route.edge.from, &route.edge.to] {
        if let Some(node) = route.graph.nodes.get(node_id)
            && node.kind == NodeKind::Symbol
        {
            item.via_symbols.insert(node.name.clone());
        }
    }
    item.confidence = item
        .confidence
        .max((route.edge.confidence * 100.0).round() as u8);
}

fn add_source_context(
    contexts: &mut BTreeMap<String, Vec<SourceContext>>,
    source_hunks: &[String],
    target: Option<&GraphNode>,
    kind: &EdgeKind,
    direction: Direction,
    edge: &crate::graph::GraphEdge,
    snapshot: &Snapshot,
) {
    let Some(target) = target else { return };
    let Some(location) = target.preferred_review_location() else {
        return;
    };
    for source in source_hunks {
        let source_file = location.path.to_path_buf().ok().map(|path| {
            snapshot
                .storage_dir
                .join(match location.side {
                    crate::graph::SourceSide::Left => "before",
                    crate::graph::SourceSide::Right => "after",
                })
                .join(path)
        });
        contexts
            .entry(source.clone())
            .or_default()
            .push(SourceContext {
                node_id: target.id.clone(),
                relation: kind.clone(),
                direction,
                symbol: target.name.clone(),
                path: location.path.display.clone(),
                side: format!("{:?}", location.side).to_lowercase(),
                line: location.range.start.line + 1,
                evidence_id: edge.id.clone(),
                confidence: (edge.confidence * 100.0).round() as u8,
                source_file,
            });
    }
}

fn hunks_at_location(hunks: &[&GraphNode], location: &SourceLocation) -> Vec<String> {
    hunks
        .iter()
        .filter(|hunk| {
            hunk.locations.iter().any(|candidate| {
                candidate.side == location.side
                    && candidate.path.bytes_base64 == location.path.bytes_base64
                    && ranges_touch(candidate.range, location.range)
            })
        })
        .map(|hunk| hunk.id.clone())
        .collect()
}

fn hunks_via_smallest_symbol(graph: &ChangeGraph, location: &SourceLocation) -> Vec<String> {
    graph
        .nodes
        .values()
        .filter(|node| node.kind == NodeKind::Symbol && !node.hunk_ids.is_empty())
        .filter_map(|node| {
            let candidate = node.locations.iter().find(|candidate| {
                candidate.side == location.side
                    && candidate.path.bytes_base64 == location.path.bytes_base64
                    && encloses(candidate.range, location.range)
            })?;
            Some((range_span(candidate.range), &node.hunk_ids))
        })
        .min_by_key(|(span, _)| *span)
        .map(|(_, ids)| ids.clone())
        .unwrap_or_default()
}

fn ranges_touch(left: TextRange, right: TextRange) -> bool {
    if right.start == right.end {
        return left.start.line <= right.start.line && right.start.line < left.end.line;
    }
    left.start.line < right.end.line && right.start.line < left.end.line
}

fn encloses(outer: TextRange, inner: TextRange) -> bool {
    outer.start.line <= inner.start.line && outer.end.line >= inner.end.line
}

fn range_span(range: TextRange) -> u32 {
    range.end.line.saturating_sub(range.start.line)
}

fn is_semantic(kind: &EdgeKind) -> bool {
    matches!(
        kind,
        EdgeKind::RuntimeTest
            | EdgeKind::TestReference
            | EdgeKind::References
            | EdgeKind::Definition
            | EdgeKind::Calls
    )
}

pub(super) fn relation_label(kind: &EdgeKind) -> &'static str {
    match kind {
        EdgeKind::RuntimeTest => "executed test",
        EdgeKind::TestReference => "static test",
        EdgeKind::References => "usage",
        EdgeKind::Definition => "definition",
        EdgeKind::Calls => "call",
        EdgeKind::Overlaps => "symbol",
        EdgeKind::Contains => "contains",
        EdgeKind::Counterpart => "counterpart",
    }
}

fn relation_rank(kind: &EdgeKind) -> u8 {
    match kind {
        EdgeKind::RuntimeTest => 0,
        EdgeKind::TestReference => 1,
        EdgeKind::References => 2,
        EdgeKind::Definition => 3,
        EdgeKind::Calls => 4,
        EdgeKind::Overlaps => 5,
        EdgeKind::Contains => 6,
        EdgeKind::Counterpart => 7,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{GraphEdge, SourceSide};
    use crate::model::GraphRevisionId;
    use crate::position::Position;
    use crate::ranking::{QueueItem, RankingMetrics, TitleSource};
    use crate::snapshot::GitPath;

    fn location(path: &str, side: SourceSide, start: u32, end: u32) -> SourceLocation {
        SourceLocation {
            side,
            path: GitPath::from_bytes(path.as_bytes().to_vec()),
            range: TextRange {
                start: Position {
                    line: start,
                    character: 0,
                },
                end: Position {
                    line: end,
                    character: 0,
                },
            },
        }
    }

    fn node(
        id: &str,
        kind: NodeKind,
        name: &str,
        location: SourceLocation,
        hunk_ids: &[&str],
    ) -> GraphNode {
        GraphNode {
            id: id.into(),
            kind,
            name: name.into(),
            symbol_kind: None,
            changed: !hunk_ids.is_empty(),
            locations: vec![location],
            selection_range: None,
            hunk_ids: hunk_ids.iter().map(|id| (*id).into()).collect(),
        }
    }

    fn queue_item(id: &str, title: &str, path: &str, score: u8) -> QueueItem {
        QueueItem {
            node_id: id.into(),
            kind: NodeKind::Hunk,
            name: title.into(),
            title: title.into(),
            title_source: TitleSource::Model,
            path: Some(path.into()),
            score: Some(score),
            tags: Vec::new(),
            rationale: None,
            confidence: Some(1.0),
            inherited_from: None,
            assessed: true,
        }
    }

    #[test]
    fn projects_and_deduplicates_changed_test_hunks_in_both_directions() {
        let h1 = node(
            "h1",
            NodeKind::Hunk,
            "implementation",
            location("src/auth.ts", SourceSide::Right, 10, 20),
            &["h1"],
        );
        let symbol = node(
            "symbol",
            NodeKind::Symbol,
            "authenticate",
            location("src/auth.ts", SourceSide::Right, 5, 30),
            &["h1"],
        );
        let h2 = node(
            "h2",
            NodeKind::Hunk,
            "test",
            location("tests/auth.test.ts", SourceSide::Right, 40, 55),
            &["h2"],
        );
        let reference = node(
            "reference",
            NodeKind::Reference,
            "authenticate",
            location("tests/auth.test.ts", SourceSide::Right, 45, 45),
            &[],
        );
        let nodes = [h1, symbol, h2, reference]
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect();
        let edges = ["edge-a", "edge-b"]
            .into_iter()
            .map(|id| GraphEdge {
                id: id.into(),
                from: "symbol".into(),
                to: "reference".into(),
                kind: EdgeKind::TestReference,
                producer: "lsp".into(),
                evidence_kind: "resolved".into(),
                confidence: 1.0,
                location: None,
            })
            .collect();
        let graph = ChangeGraph {
            revision: GraphRevisionId::parse("grf_test").unwrap(),
            snapshot_id: "snp_test".into(),
            fingerprint: "fixture".into(),
            nodes,
            edges,
            coverage: Vec::new(),
            unfinished_frontier: Vec::new(),
        };
        let queue = Queue {
            graph_revision: "grf_test".into(),
            stale: false,
            fully_ranked: true,
            finalized: true,
            context_digest: None,
            metrics: RankingMetrics::default(),
            assessed_changes: 2,
            total_changes: 2,
            items: vec![
                queue_item("h1", "Validate authentication", "src/auth.ts", 90),
                queue_item("h2", "Cover expired token", "tests/auth.test.ts", 80),
            ],
        };

        let index = Index::new(&graph, &queue, &snapshot());
        assert_eq!(index.related("h1").len(), 1);
        assert_eq!(index.related("h1")[0].node_id, "h2");
        assert_eq!(index.related("h1")[0].evidence_ids.len(), 2);
        assert_eq!(index.related("h2")[0].node_id, "h1");
        assert_eq!(index.linked_tests("h1").len(), 1);
        assert_eq!(index.linked_tests("h1")[0].node_id, "h2");
        assert_eq!(index.linked_tests("h1")[0].kind, LinkedTestKind::Changed);
    }

    #[test]
    fn keeps_unchanged_targets_separate_from_review_hunks() {
        let hunk = node(
            "h1",
            NodeKind::Hunk,
            "implementation",
            location("src/auth.ts", SourceSide::Left, 10, 20),
            &["h1"],
        );
        let symbol = node(
            "symbol",
            NodeKind::Symbol,
            "authenticate",
            location("src/auth.ts", SourceSide::Left, 5, 30),
            &["h1"],
        );
        let usage = node(
            "usage",
            NodeKind::Reference,
            "authenticate",
            location("src/server.ts", SourceSide::Left, 80, 80),
            &[],
        );
        let nodes = [hunk, symbol, usage]
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect();
        let graph = ChangeGraph {
            revision: GraphRevisionId::parse("grf_test").unwrap(),
            snapshot_id: "snp_test".into(),
            fingerprint: "fixture".into(),
            nodes,
            edges: vec![GraphEdge {
                id: "usage-edge".into(),
                from: "symbol".into(),
                to: "usage".into(),
                kind: EdgeKind::References,
                producer: "lsp".into(),
                evidence_kind: "resolved".into(),
                confidence: 1.0,
                location: None,
            }],
            coverage: Vec::new(),
            unfinished_frontier: Vec::new(),
        };
        let queue = Queue {
            graph_revision: "grf_test".into(),
            stale: false,
            fully_ranked: true,
            finalized: true,
            context_digest: None,
            metrics: RankingMetrics::default(),
            assessed_changes: 1,
            total_changes: 1,
            items: vec![queue_item(
                "h1",
                "Validate authentication",
                "src/auth.ts",
                90,
            )],
        };

        let index = Index::new(&graph, &queue, &snapshot());
        assert!(index.related("h1").is_empty());
        assert_eq!(index.source_context("h1").len(), 1);
        assert_eq!(index.source_context("h1")[0].path, "src/server.ts");
    }

    #[test]
    fn runtime_test_is_a_fast_navigable_target_but_not_a_queue_item() {
        let hunk = node(
            "h1",
            NodeKind::Hunk,
            "implementation",
            location("src/auth.ts", SourceSide::Right, 10, 20),
            &["h1"],
        );
        let mut test = node(
            "test1",
            NodeKind::Test,
            "tests/auth.test.ts",
            location("tests/auth.test.ts", SourceSide::Right, 4, 5),
            &[],
        );
        test.changed = false;
        test.symbol_kind = Some("file".into());
        let graph = ChangeGraph {
            revision: GraphRevisionId::parse("grf_test").unwrap(),
            snapshot_id: "snp_test".into(),
            fingerprint: "fixture".into(),
            nodes: [hunk, test]
                .into_iter()
                .map(|node| (node.id.clone(), node))
                .collect(),
            edges: vec![GraphEdge {
                id: "runtime-edge".into(),
                from: "h1".into(),
                to: "test1".into(),
                kind: EdgeKind::RuntimeTest,
                producer: "test-evidence".into(),
                evidence_kind: "tev_one:Passed".into(),
                confidence: 1.0,
                location: None,
            }],
            coverage: Vec::new(),
            unfinished_frontier: Vec::new(),
        };
        let queue = Queue {
            graph_revision: "grf_test".into(),
            stale: false,
            fully_ranked: true,
            finalized: true,
            context_digest: None,
            metrics: RankingMetrics::default(),
            assessed_changes: 1,
            total_changes: 1,
            items: vec![queue_item(
                "h1",
                "Validate authentication",
                "src/auth.ts",
                90,
            )],
        };
        let index = Index::new(&graph, &queue, &snapshot());
        assert_eq!(index.related("h1")[0].node_id, "test1");
        assert_eq!(index.related("h1")[0].primary_label(), "executed test");
        assert_eq!(index.related("test1")[0].node_id, "h1");
        assert_eq!(index.linked_tests("h1").len(), 1);
        assert_eq!(index.linked_tests("h1")[0].kind, LinkedTestKind::After);
        assert_eq!(
            index.linked_tests("h1")[0].granularity.as_deref(),
            Some("file")
        );
        assert_eq!(
            index.linked_tests("h1")[0].status.as_deref(),
            Some("passed")
        );
        assert_eq!(queue.total_changes, 1);
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            id: crate::model::SnapshotId::parse("snp_test").unwrap(),
            repository: PathBuf::from("/repo"),
            input: crate::snapshot::SnapshotInput::Uncommitted,
            original_base: "base".into(),
            original_head: "head".into(),
            comparison_base: "base".into(),
            before_commit: "base".into(),
            after_commit: "head".into(),
            captured_at: chrono::Utc::now(),
            source_fingerprint: "fixture".into(),
            files: Vec::new(),
            storage_dir: PathBuf::from("/snapshot"),
        }
    }
}

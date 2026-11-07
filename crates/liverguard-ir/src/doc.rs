//! The script document and its structural validation.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::node::{Edge, LongPressData, Node, NodeKind, SwipeData, TapData, MAX_SPREAD, MIN_SPREAD};
use crate::target::Target;

pub const IR_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_engine: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Flow {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IrDoc {
    #[serde(default = "ir_version")]
    pub ir_version: u32,
    pub meta: Meta,
    #[serde(default)]
    pub targets: BTreeMap<String, Target>,
    pub flow: Flow,
}

fn ir_version() -> u32 {
    IR_VERSION
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Problem {
    pub severity: Severity,
    pub message: String,
    pub node: Option<String>,
}

impl Problem {
    fn error(message: String, node: Option<&str>) -> Self {
        Self { severity: Severity::Error, message, node: node.map(str::to_string) }
    }

    fn warning(message: String, node: Option<&str>) -> Self {
        Self { severity: Severity::Warning, message, node: node.map(str::to_string) }
    }

    fn global(message: String) -> Self {
        Self { severity: Severity::Error, message, node: None }
    }
}

impl Flow {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn start(&self) -> Option<&Node> {
        self.nodes.iter().find(|node| matches!(node.kind, NodeKind::Start(_)))
    }

    /// The node an output handle points at, if wired.
    pub fn next(&self, source: &str, handle: &str) -> Option<&str> {
        self.edges
            .iter()
            .find(|edge| edge.source == source && edge.source_handle == handle)
            .map(|edge| edge.target.as_str())
    }
}

impl IrDoc {
    /// A minimal runnable document: `start` only.
    pub fn empty(name: impl Into<String>) -> Self {
        let flow = Flow {
            nodes: vec![Node {
                id: "start".to_string(),
                pos: Default::default(),
                label: None,
                max_visits: None,
                kind: NodeKind::Start(Default::default()),
            }],
            edges: vec![],
        };
        Self {
            ir_version: IR_VERSION,
            meta: Meta { name: name.into(), game: None, min_engine: None },
            targets: BTreeMap::new(),
            flow,
        }
    }

    pub fn target(&self, name: &str) -> Option<&Target> {
        self.targets.get(name)
    }

    /// SHA-256 hashes of every template asset the script needs.
    pub fn asset_hashes(&self) -> Vec<String> {
        self.targets
            .values()
            .filter_map(|target| match target {
                Target::Template(t) => Some(t.asset.clone()),
                Target::Point(_) => None,
            })
            .collect()
    }

    pub fn errors(problems: &[Problem]) -> Vec<&Problem> {
        problems.iter().filter(|p| p.severity == Severity::Error).collect()
    }

    /// Structural checks the type system cannot express.
    pub fn validate(&self) -> Vec<Problem> {
        let mut problems = Vec::new();

        for (name, target) in &self.targets {
            if let Target::Template(t) = target {
                if t.asset.is_empty() {
                    problems.push(Problem::global(format!("target \"{name}\" has no asset")));
                }
            }
        }

        self.validate_flow(&mut problems);

        problems
    }

    fn validate_flow(&self, problems: &mut Vec<Problem>) {
        let flow = &self.flow;
        let mut ids: HashSet<&str> = HashSet::new();
        for node in &flow.nodes {
            if !ids.insert(node.id.as_str()) {
                problems.push(Problem::error(format!("duplicate node id \"{}\"", node.id), Some(&node.id)));
            }
        }

        let starts: Vec<&Node> = flow.nodes.iter().filter(|n| matches!(n.kind, NodeKind::Start(_))).collect();
        match starts.len() {
            1 => {}
            0 => problems.push(Problem::error("flow has no start node".to_string(), None)),
            n => problems.push(Problem::error(format!("flow has {n} start nodes, expected 1"), None)),
        }

        let by_id: HashMap<&str, &Node> = flow.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let mut used_handles: BTreeSet<(&str, &str)> = BTreeSet::new();

        for edge in &flow.edges {
            let Some(source) = by_id.get(edge.source.as_str()) else {
                problems.push(Problem::error(format!("edge \"{}\" starts at unknown node \"{}\"", edge.id, edge.source), None));
                continue;
            };
            if !by_id.contains_key(edge.target.as_str()) {
                problems.push(Problem::error(format!("edge \"{}\" ends at unknown node \"{}\"", edge.id, edge.target), None));
            }
            if matches!(by_id.get(edge.target.as_str()).map(|n| &n.kind), Some(NodeKind::Start(_))) {
                problems.push(Problem::error("a start node cannot have incoming edges".to_string(), Some(&edge.target)));
            }
            if !source.kind.output_handles().iter().any(|h| h == &edge.source_handle) {
                problems.push(Problem::error(
                    format!("node \"{}\" ({}) has no handle \"{}\"", source.id, source.kind.type_name(), edge.source_handle),
                    Some(&source.id),
                ));
            }
            if !used_handles.insert((edge.source.as_str(), edge.source_handle.as_str())) {
                problems.push(Problem::error(
                    format!("handle \"{}\" of node \"{}\" has more than one outgoing edge", edge.source_handle, edge.source),
                    Some(&edge.source),
                ));
            }
        }

        for node in &flow.nodes {
            for target in node.kind.target_refs() {
                if !self.targets.contains_key(target) {
                    problems.push(Problem::error(
                        format!("target \"{target}\" is referenced but not defined"),
                        Some(&node.id),
                    ));
                }
            }
            match &node.kind {
                NodeKind::Loop(data) => {
                    if data.mode == crate::node::LoopMode::While && data.when.is_none() {
                        problems.push(Problem::error("a while loop needs a condition".to_string(), Some(&node.id)));
                    }
                }
                NodeKind::Sleep(data) if data.min_ms > data.max_ms => {
                    problems.push(Problem::error(
                        format!("sleep minMs ({}) is above maxMs ({})", data.min_ms, data.max_ms),
                        Some(&node.id),
                    ));
                }
                NodeKind::Tap(TapData { spread, .. })
                | NodeKind::LongPress(LongPressData { spread, .. })
                | NodeKind::Swipe(SwipeData { spread, .. })
                    if !(MIN_SPREAD..=MAX_SPREAD).contains(spread) =>
                {
                    problems.push(Problem::error(
                        format!("spread ({spread}) must be between {MIN_SPREAD} and {MAX_SPREAD}"),
                        Some(&node.id),
                    ));
                }
                NodeKind::Switch(data) if data.cases.is_empty() => {
                    problems.push(Problem::error("a switch needs at least one case".to_string(), Some(&node.id)));
                }
                _ => {}
            }
        }

        // Reachability: everything is reached from start, whichever handle it hangs off.
        if let Some(start) = starts.first() {
            let mut seen: HashSet<&str> = HashSet::new();
            let mut stack = vec![start.id.as_str()];
            while let Some(current) = stack.pop() {
                if !seen.insert(current) {
                    continue;
                }
                for edge in flow.edges.iter().filter(|e| e.source == current) {
                    stack.push(edge.target.as_str());
                }
            }
            for node in &flow.nodes {
                if !seen.contains(node.id.as_str()) {
                    problems.push(Problem::warning(format!("node \"{}\" is not reachable from start", node.id), Some(&node.id)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::*;
    use crate::target::*;

    fn node(id: &str, kind: NodeKind) -> Node {
        Node { id: id.to_string(), pos: Pos::default(), label: None, max_visits: None, kind }
    }

    fn edge(id: &str, source: &str, handle: &str, target: &str) -> Edge {
        Edge { id: id.into(), source: source.into(), source_handle: handle.into(), target: target.into(), target_handle: None }
    }

    fn tap(target: &str) -> NodeKind {
        NodeKind::Tap(TapData { target: target.into(), duration_ms: 60, human: false, spread: 1.0 })
    }

    fn doc_with(nodes: Vec<Node>, edges: Vec<Edge>) -> IrDoc {
        let mut doc = IrDoc::empty("t");
        doc.flow = Flow { nodes, edges };
        doc
    }

    fn template() -> Target {
        Target::Template(TemplateTarget::new("a".repeat(64), AssetBox { w: 10.0, h: 10.0 }, [0.0, 0.0, 1.0, 1.0]))
    }

    #[test]
    fn empty_document_is_valid() {
        assert!(IrDoc::errors(&IrDoc::empty("x").validate()).is_empty());
    }

    #[test]
    fn wire_format_matches_react_flow_nodes() {
        let json = r#"{
          "irVersion": 3,
          "meta": {"name": "demo"},
          "targets": {},
          "flow": {
            "nodes": [
              {"id": "s", "type": "start", "pos": {"x": 0, "y": 0}, "data": {}},
              {"id": "t", "type": "sleep", "pos": {"x": 10, "y": 20}, "data": {"minMs": 50, "maxMs": 80}, "label": "wait"}
            ],
            "edges": [{"id": "e", "source": "s", "sourceHandle": "next", "target": "t"}]
          }
        }"#;
        let doc: IrDoc = serde_json::from_str(json).expect("parses");
        let sleep = doc.flow.node("t").unwrap();
        assert_eq!(sleep.kind, NodeKind::Sleep(SleepData { min_ms: 50, max_ms: 80 }));
        assert_eq!(sleep.label.as_deref(), Some("wait"));

        let again: IrDoc = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(doc, again);
        assert!(IrDoc::errors(&doc.validate()).is_empty());
    }

    #[test]
    fn a_legacy_sleep_node_with_only_ms_loads_as_one_second() {
        let data: SleepData = serde_json::from_str(r#"{"ms": 50}"#).unwrap();
        assert_eq!(data, SleepData { min_ms: 1000, max_ms: 1000 });
    }

    #[test]
    fn a_sleep_range_with_min_above_max_is_an_error() {
        let doc = doc_with(
            vec![node("s", NodeKind::Start(Default::default())), node("n", NodeKind::Sleep(SleepData { min_ms: 2000, max_ms: 1000 }))],
            vec![edge("e", "s", "next", "n")],
        );
        let problems = doc.validate();
        assert!(IrDoc::errors(&problems).iter().any(|p| p.message.contains("above maxMs")));
    }

    #[test]
    fn human_defaults_off_and_the_old_random_point_flag_still_reads_as_it() {
        for (json, expected) in [
            (r#"{"target":"b","human":true}"#, true),
            (r#"{"target":"b","randomPoint":true}"#, true),
            (r#"{"target":"b"}"#, false),
        ] {
            let json = format!(r#"{{"id":"t","type":"tap","pos":{{"x":0,"y":0}},"data":{json}}}"#);
            let node: Node = serde_json::from_str(&json).unwrap();
            match node.kind {
                NodeKind::Tap(data) => {
                    assert_eq!(data.duration_ms, 60);
                    assert_eq!(data.human, expected, "{json}");
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[test]
    fn swipe_human_defaults_off() {
        let json = r#"{"id":"s","type":"swipe","pos":{"x":0,"y":0},"data":{"from":"a"}}"#;
        let node: Node = serde_json::from_str(json).unwrap();
        match node.kind {
            NodeKind::Swipe(data) => {
                assert!(!data.human);
                assert_eq!(data.spread, 1.0);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn spread_defaults_to_the_standard_value_and_reads_back() {
        for (kind, data, expected) in [
            ("tap", r#"{"target":"b"}"#, 1.0),
            ("tap", r#"{"target":"b","spread":1.4}"#, 1.4),
            ("longPress", r#"{"target":"b"}"#, 1.0),
            ("longPress", r#"{"target":"b","spread":0.5}"#, 0.5),
        ] {
            let json = format!(r#"{{"id":"t","type":"{kind}","pos":{{"x":0,"y":0}},"data":{data}}}"#);
            let node: Node = serde_json::from_str(&json).unwrap();
            let got = match node.kind {
                NodeKind::Tap(d) => d.spread,
                NodeKind::LongPress(d) => d.spread,
                other => panic!("unexpected {other:?}"),
            };
            assert_eq!(got, expected, "{json}");
        }
    }

    #[test]
    fn rejects_a_spread_outside_the_allowed_range() {
        for (spread, ok) in [(0.3, true), (1.0, true), (2.0, true), (0.1, false), (2.5, false), (f64::NAN, false)] {
            let mut doc = doc_with(
                vec![
                    node("s", NodeKind::Start(Default::default())),
                    node("t", NodeKind::Tap(TapData { target: "b".into(), duration_ms: 60, human: true, spread })),
                ],
                vec![edge("e", "s", "next", "t")],
            );
            doc.targets.insert("b".into(), template());
            let bad = IrDoc::errors(&doc.validate()).iter().any(|p| p.message.contains("spread"));
            assert_eq!(!bad, ok, "spread {spread}");
        }
    }

    #[test]
    fn rejects_missing_start_and_unknown_target() {
        let doc = doc_with(vec![node("t", tap("ghost"))], vec![]);
        let messages: Vec<String> = IrDoc::errors(&doc.validate()).iter().map(|p| p.message.clone()).collect();
        assert!(messages.iter().any(|m| m.contains("no start node")));
        assert!(messages.iter().any(|m| m.contains("\"ghost\" is referenced but not defined")));
    }

    #[test]
    fn rejects_bad_handles_and_double_wiring() {
        let mut doc = doc_with(
            vec![node("s", NodeKind::Start(Default::default())), node("a", tap("b")), node("c", tap("b"))],
            vec![
                edge("e1", "s", "next", "a"),
                edge("e2", "s", "next", "c"),
                edge("e3", "a", "found", "c"),
            ],
        );
        doc.targets.insert("b".into(), template());
        let messages: Vec<String> = IrDoc::errors(&doc.validate()).iter().map(|p| p.message.clone()).collect();
        assert!(messages.iter().any(|m| m.contains("more than one outgoing edge")));
        assert!(messages.iter().any(|m| m.contains("has no handle \"found\"")));
    }

    #[test]
    fn warns_about_unreachable_nodes_but_stays_valid() {
        let mut doc = doc_with(vec![node("s", NodeKind::Start(Default::default())), node("x", NodeKind::Sleep(SleepData { min_ms: 1, max_ms: 1 }))], vec![]);
        doc.targets.clear();
        let problems = doc.validate();
        assert!(IrDoc::errors(&problems).is_empty());
        assert!(problems.iter().any(|p| p.severity == Severity::Warning && p.node.as_deref() == Some("x")));
    }

    #[test]
    fn cycles_are_allowed() {
        let doc = doc_with(
            vec![node("s", NodeKind::Start(Default::default())), node("a", NodeKind::Sleep(SleepData { min_ms: 1, max_ms: 1 }))],
            vec![edge("e1", "s", "next", "a"), edge("e2", "a", "next", "a")],
        );
        assert!(IrDoc::errors(&doc.validate()).is_empty());
    }

    #[test]
    fn switch_exposes_one_handle_per_case_plus_default() {
        let kind = NodeKind::Switch(SwitchData { cases: vec!["a".into(), "b".into()] });
        assert_eq!(kind.output_handles(), vec!["case:a", "case:b", "default"]);
    }

    #[test]
    fn schema_generation_covers_the_document() {
        let schema = schemars::schema_for!(IrDoc);
        let text = serde_json::to_string(&schema).unwrap();
        assert!(text.contains("waitFor"));
        assert!(text.contains("sourceHandle"));
    }
}

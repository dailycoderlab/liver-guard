//! The script document and its structural validation.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::node::{Edge, Node, NodeKind, VarValue};
use crate::target::Target;

pub const IR_VERSION: u32 = 2;
pub const DEFAULT_MAX_DURATION_MS: u64 = 1_800_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_engine: Option<String>,
    #[serde(default = "default_max_duration")]
    pub max_duration_ms: u64,
}

fn default_max_duration() -> u64 {
    DEFAULT_MAX_DURATION_MS
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
    pub vars: BTreeMap<String, VarValue>,
    #[serde(default)]
    pub targets: BTreeMap<String, Target>,
    #[serde(default = "main_flow")]
    pub entry: String,
    pub flows: BTreeMap<String, Flow>,
}

fn ir_version() -> u32 {
    IR_VERSION
}

fn main_flow() -> String {
    "main".to_string()
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
    pub flow: Option<String>,
    pub node: Option<String>,
}

impl Problem {
    fn error(message: String, flow: &str, node: Option<&str>) -> Self {
        Self { severity: Severity::Error, message, flow: Some(flow.to_string()), node: node.map(str::to_string) }
    }

    fn warning(message: String, flow: &str, node: Option<&str>) -> Self {
        Self { severity: Severity::Warning, message, flow: Some(flow.to_string()), node: node.map(str::to_string) }
    }

    fn global(message: String) -> Self {
        Self { severity: Severity::Error, message, flow: None, node: None }
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
        let mut flows = BTreeMap::new();
        flows.insert(
            "main".to_string(),
            Flow {
                nodes: vec![Node {
                    id: "start".to_string(),
                    pos: Default::default(),
                    label: None,
                    max_visits: None,
                    kind: NodeKind::Start(Default::default()),
                }],
                edges: vec![],
            },
        );
        Self {
            ir_version: IR_VERSION,
            meta: Meta { name: name.into(), game: None, min_engine: None, max_duration_ms: DEFAULT_MAX_DURATION_MS },
            vars: BTreeMap::new(),
            targets: BTreeMap::new(),
            entry: "main".to_string(),
            flows,
        }
    }

    pub fn flow(&self, name: &str) -> Option<&Flow> {
        self.flows.get(name)
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

        if !self.flows.contains_key(&self.entry) {
            problems.push(Problem::global(format!("entry flow \"{}\" does not exist", self.entry)));
        }

        for (name, target) in &self.targets {
            if let Target::Template(t) = target {
                if t.asset.is_empty() {
                    problems.push(Problem::global(format!("target \"{name}\" has no asset")));
                }
            }
        }

        for (flow_name, flow) in &self.flows {
            self.validate_flow(flow_name, flow, &mut problems);
        }

        problems
    }

    fn validate_flow(&self, flow_name: &str, flow: &Flow, problems: &mut Vec<Problem>) {
        let mut ids: HashSet<&str> = HashSet::new();
        for node in &flow.nodes {
            if !ids.insert(node.id.as_str()) {
                problems.push(Problem::error(format!("duplicate node id \"{}\"", node.id), flow_name, Some(&node.id)));
            }
        }

        let starts: Vec<&Node> = flow.nodes.iter().filter(|n| matches!(n.kind, NodeKind::Start(_))).collect();
        match starts.len() {
            1 => {}
            0 => problems.push(Problem::error("flow has no start node".to_string(), flow_name, None)),
            n => problems.push(Problem::error(format!("flow has {n} start nodes, expected 1"), flow_name, None)),
        }

        let by_id: HashMap<&str, &Node> = flow.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let mut used_handles: BTreeSet<(&str, &str)> = BTreeSet::new();

        for edge in &flow.edges {
            let Some(source) = by_id.get(edge.source.as_str()) else {
                problems.push(Problem::error(format!("edge \"{}\" starts at unknown node \"{}\"", edge.id, edge.source), flow_name, None));
                continue;
            };
            if !by_id.contains_key(edge.target.as_str()) {
                problems.push(Problem::error(format!("edge \"{}\" ends at unknown node \"{}\"", edge.id, edge.target), flow_name, None));
            }
            if matches!(by_id.get(edge.target.as_str()).map(|n| &n.kind), Some(NodeKind::Start(_))) {
                problems.push(Problem::error("a start node cannot have incoming edges".to_string(), flow_name, Some(&edge.target)));
            }
            if !source.kind.output_handles().iter().any(|h| h == &edge.source_handle) {
                problems.push(Problem::error(
                    format!("node \"{}\" ({}) has no handle \"{}\"", source.id, source.kind.type_name(), edge.source_handle),
                    flow_name,
                    Some(&source.id),
                ));
            }
            if !used_handles.insert((edge.source.as_str(), edge.source_handle.as_str())) {
                problems.push(Problem::error(
                    format!("handle \"{}\" of node \"{}\" has more than one outgoing edge", edge.source_handle, edge.source),
                    flow_name,
                    Some(&edge.source),
                ));
            }
        }

        for node in &flow.nodes {
            for target in node.kind.target_refs() {
                if !self.targets.contains_key(target) {
                    problems.push(Problem::error(
                        format!("target \"{target}\" is referenced but not defined"),
                        flow_name,
                        Some(&node.id),
                    ));
                }
            }
            match &node.kind {
                NodeKind::Call(data) if !self.flows.contains_key(&data.flow) => {
                    problems.push(Problem::error(format!("flow \"{}\" is called but not defined", data.flow), flow_name, Some(&node.id)));
                }
                NodeKind::Loop(data) => {
                    if data.mode == crate::node::LoopMode::While && data.when.is_none() {
                        problems.push(Problem::error("a while loop needs a condition".to_string(), flow_name, Some(&node.id)));
                    }
                }
                NodeKind::Switch(data) if data.cases.is_empty() => {
                    problems.push(Problem::error("a switch needs at least one case".to_string(), flow_name, Some(&node.id)));
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
                    problems.push(Problem::warning(format!("node \"{}\" is not reachable from start", node.id), flow_name, Some(&node.id)));
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
        NodeKind::Tap(TapData { target: target.into(), duration_ms: 60, jitter: Jitter::default() })
    }

    fn doc_with(nodes: Vec<Node>, edges: Vec<Edge>) -> IrDoc {
        let mut doc = IrDoc::empty("t");
        doc.flows.insert("main".into(), Flow { nodes, edges });
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
          "irVersion": 2,
          "meta": {"name": "demo"},
          "entry": "main",
          "targets": {},
          "flows": {"main": {
            "nodes": [
              {"id": "s", "type": "start", "pos": {"x": 0, "y": 0}, "data": {}},
              {"id": "t", "type": "sleep", "pos": {"x": 10, "y": 20}, "data": {"ms": 50}, "label": "wait"}
            ],
            "edges": [{"id": "e", "source": "s", "sourceHandle": "next", "target": "t"}]
          }}
        }"#;
        let doc: IrDoc = serde_json::from_str(json).expect("parses");
        let sleep = doc.flow("main").unwrap().node("t").unwrap();
        assert_eq!(sleep.kind, NodeKind::Sleep(SleepData { ms: 50 }));
        assert_eq!(sleep.label.as_deref(), Some("wait"));
        assert_eq!(doc.meta.max_duration_ms, DEFAULT_MAX_DURATION_MS);

        let again: IrDoc = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(doc, again);
        assert!(IrDoc::errors(&doc.validate()).is_empty());
    }

    #[test]
    fn tap_data_flattens_jitter_fields() {
        let json = r#"{"id":"t","type":"tap","pos":{"x":0,"y":0},"data":{"target":"b","delayMs":100,"offsetRatio":0.2}}"#;
        let node: Node = serde_json::from_str(json).unwrap();
        match node.kind {
            NodeKind::Tap(data) => {
                assert_eq!(data.duration_ms, 60);
                assert_eq!(data.jitter.delay_ms, 100.0);
                assert_eq!(data.jitter.offset_ratio, 0.2);
            }
            other => panic!("unexpected {other:?}"),
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
        let mut doc = doc_with(vec![node("s", NodeKind::Start(Default::default())), node("x", NodeKind::Sleep(SleepData { ms: 1 }))], vec![]);
        doc.targets.clear();
        let problems = doc.validate();
        assert!(IrDoc::errors(&problems).is_empty());
        assert!(problems.iter().any(|p| p.severity == Severity::Warning && p.node.as_deref() == Some("x")));
    }

    #[test]
    fn cycles_are_allowed() {
        let doc = doc_with(
            vec![node("s", NodeKind::Start(Default::default())), node("a", NodeKind::Sleep(SleepData { ms: 1 }))],
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
    fn flow_calls_must_exist() {
        let doc = doc_with(
            vec![node("s", NodeKind::Start(Default::default())), node("c", NodeKind::Call(CallData { flow: "nope".into() }))],
            vec![edge("e", "s", "next", "c")],
        );
        assert!(IrDoc::errors(&doc.validate()).iter().any(|p| p.message.contains("flow \"nope\"")));
    }

    #[test]
    fn schema_generation_covers_the_document() {
        let schema = schemars::schema_for!(IrDoc);
        let text = serde_json::to_string(&schema).unwrap();
        assert!(text.contains("waitFor"));
        assert!(text.contains("sourceHandle"));
    }
}

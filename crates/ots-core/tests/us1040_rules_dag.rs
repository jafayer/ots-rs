use std::path::PathBuf;

use ots_core::dag::{load_execution_dag_from_file, DagEdgeKind};

#[test]
fn builds_dag_from_us1040_rules_config() {
    let rules_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/2025/us1040.rules.toml");

    let dag = load_execution_dag_from_file(rules_path).expect("DAG should build from rules config");

    assert!(dag.nodes.contains_key("field:L1a"));
    assert!(dag.nodes.contains_key("rule:rule_L1"));
    assert!(dag.nodes.contains_key("rule:rule_refund_or_due"));

    assert!(dag
        .edges
        .iter()
        .any(|edge| edge.from == "field:L1a" && edge.to == "rule:rule_L1" && edge.kind == DagEdgeKind::Data));

    assert!(dag
        .edges
        .iter()
        .any(|edge| edge.from == "rule:rule_L1" && edge.to == "rule:rule_L9" && edge.kind == DagEdgeKind::Data));

    assert!(dag.edges.iter().any(|edge| {
        edge.from == "rule:rule_refund_or_due"
            && edge.to == "rule:rule_refund_L34"
            && edge.kind == DagEdgeKind::ControlThen
    }));

    assert!(dag.edges.iter().any(|edge| {
        edge.from == "rule:rule_refund_or_due"
            && edge.to == "rule:rule_due_L37"
            && edge.kind == DagEdgeKind::ControlElse
    }));

    let rule_apply_to_2025 = dag
        .nodes
        .get("rule:rule_ApplyTo2025_Validated")
        .expect("rule_ApplyTo2025_Validated node should exist");
    assert!(rule_apply_to_2025
        .external_inputs
        .iter()
        .any(|input| input == "refund.apply_to_next_year_min_pct"));

    let ordered = dag
        .topological_order()
        .expect("DAG should be acyclic and topologically sortable");
    assert_eq!(ordered.len(), dag.nodes.len());
}

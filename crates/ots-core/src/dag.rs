use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionDag {
    pub nodes: BTreeMap<String, DagNode>,
    pub edges: Vec<DagEdge>,
}

impl ExecutionDag {
    pub fn topological_order(&self) -> Result<Vec<String>, DagBuildError> {
        let mut in_degree: BTreeMap<String, usize> = self
            .nodes
            .keys()
            .map(|id| (id.clone(), 0_usize))
            .collect();

        for edge in &self.edges {
            if let Some(value) = in_degree.get_mut(&edge.to) {
                *value += 1;
            }
        }

        let mut queue: VecDeque<String> = in_degree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(id, _)| id.clone())
            .collect();
        let mut ordered = Vec::with_capacity(self.nodes.len());

        while let Some(node_id) = queue.pop_front() {
            ordered.push(node_id.clone());

            for edge in self.edges.iter().filter(|edge| edge.from == node_id) {
                if let Some(value) = in_degree.get_mut(&edge.to) {
                    *value -= 1;
                    if *value == 0 {
                        queue.push_back(edge.to.clone());
                    }
                }
            }
        }

        if ordered.len() != self.nodes.len() {
            return Err(DagBuildError::CycleDetected);
        }

        Ok(ordered)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagNode {
    pub id: String,
    pub kind: DagNodeKind,
    pub target: Option<String>,
    pub operator: Option<String>,
    pub dependencies: Vec<String>,
    pub external_inputs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DagNodeKind {
    Field,
    Rule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagEdge {
    pub from: String,
    pub to: String,
    pub kind: DagEdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DagEdgeKind {
    Data,
    ControlThen,
    ControlElse,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RuleSchema {
    pub meta: Option<RuleMeta>,
    pub fields: Vec<FieldSpec>,
    pub rules: Vec<RuleSpec>,
    #[serde(default)]
    pub hooks: BTreeMap<String, HookSpec>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RuleMeta {
    pub form: String,
    pub year: u16,
    pub constants_file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FieldSpec {
    pub id: String,
    pub source: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub default: Option<f64>,
    pub enum_ref: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HookSpec {
    pub module: String,
    #[serde(rename = "fn")]
    pub function: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RuleSpec {
    pub id: String,
    pub target: Option<String>,
    pub kind: String,
    pub args: Option<Vec<String>>,
    pub hook: Option<String>,
    pub constants: Option<Vec<String>>,
    #[serde(rename = "if")]
    pub if_expr: Option<String>,
    #[serde(rename = "then")]
    pub then_rules: Option<Vec<String>>,
    #[serde(rename = "else")]
    pub else_rules: Option<Vec<String>>,
}

#[derive(Debug, Error)]
pub enum DagBuildError {
    #[error("failed reading config file '{path}': {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("failed parsing TOML config: {0}")]
    ParseToml(#[from] toml::de::Error),
    #[error("duplicate field declaration '{0}'")]
    DuplicateField(String),
    #[error("duplicate rule declaration '{0}'")]
    DuplicateRule(String),
    #[error("branch rule '{rule_id}' references unknown rule '{referenced_rule}'")]
    UnknownBranchRuleReference {
        rule_id: String,
        referenced_rule: String,
    },
    #[error("cycle detected in execution DAG")]
    CycleDetected,
}

pub fn load_execution_dag_from_file<P: AsRef<Path>>(path: P) -> Result<ExecutionDag, DagBuildError> {
    let path_ref = path.as_ref();
    let raw = fs::read_to_string(path_ref).map_err(|source| DagBuildError::Io {
        path: path_ref.display().to_string(),
        source,
    })?;
    let schema: RuleSchema = toml::from_str(&raw)?;
    build_execution_dag(&schema)
}

pub fn load_rule_schema_from_file<P: AsRef<Path>>(path: P) -> Result<RuleSchema, DagBuildError> {
    let path_ref = path.as_ref();
    let raw = fs::read_to_string(path_ref).map_err(|source| DagBuildError::Io {
        path: path_ref.display().to_string(),
        source,
    })?;
    let schema: RuleSchema = toml::from_str(&raw)?;
    Ok(schema)
}

pub fn build_execution_dag(schema: &RuleSchema) -> Result<ExecutionDag, DagBuildError> {
    let mut nodes = BTreeMap::new();
    let mut edges = Vec::new();

    let mut field_node_ids = BTreeMap::new();
    for field in &schema.fields {
        let key = field.id.clone();
        let node_id = format!("field:{}", field.id);

        if field_node_ids.insert(key.clone(), node_id.clone()).is_some() {
            return Err(DagBuildError::DuplicateField(key));
        }

        nodes.insert(
            node_id.clone(),
            DagNode {
                id: node_id,
                kind: DagNodeKind::Field,
                target: Some(field.id.clone()),
                operator: Some(field.source.clone()),
                dependencies: Vec::new(),
                external_inputs: Vec::new(),
            },
        );
    }

    let mut rule_node_ids = BTreeMap::new();
    for rule in &schema.rules {
        let key = rule.id.clone();
        let node_id = format!("rule:{}", rule.id);
        if rule_node_ids.insert(key.clone(), node_id.clone()).is_some() {
            return Err(DagBuildError::DuplicateRule(key));
        }

        nodes.insert(
            node_id.clone(),
            DagNode {
                id: node_id,
                kind: DagNodeKind::Rule,
                target: rule.target.clone(),
                operator: Some(rule.kind.clone()),
                dependencies: Vec::new(),
                external_inputs: Vec::new(),
            },
        );
    }

    let mut target_to_rule = BTreeMap::new();
    for rule in &schema.rules {
        if let Some(target) = &rule.target {
            target_to_rule.insert(target.clone(), rule.id.clone());
        }
    }

    for rule in &schema.rules {
        let rule_node_id = format!("rule:{}", rule.id);
        let mut dependencies: BTreeSet<String> = BTreeSet::new();
        let mut external_inputs: BTreeSet<String> = BTreeSet::new();

        for arg in rule.args.as_deref().unwrap_or_default() {
            for reference in extract_references(arg) {
                dependencies.insert(reference);
            }
        }

        if let Some(condition) = &rule.if_expr {
            for reference in extract_references(condition) {
                dependencies.insert(reference);
            }
        }

        for constant in rule.constants.as_deref().unwrap_or_default() {
            dependencies.insert(constant.to_string());
        }

        for dependency in &dependencies {
            let current_target = rule.target.as_deref();

            if current_target == Some(dependency.as_str()) {
                if let Some(field_node_id) = field_node_ids.get(dependency) {
                    edges.push(DagEdge {
                        from: field_node_id.clone(),
                        to: rule_node_id.clone(),
                        kind: DagEdgeKind::Data,
                    });
                } else {
                    external_inputs.insert(dependency.clone());
                }
            } else if let Some(producer_rule_id) = target_to_rule.get(dependency) {
                edges.push(DagEdge {
                    from: format!("rule:{producer_rule_id}"),
                    to: rule_node_id.clone(),
                    kind: DagEdgeKind::Data,
                });
            } else if let Some(field_node_id) = field_node_ids.get(dependency) {
                edges.push(DagEdge {
                    from: field_node_id.clone(),
                    to: rule_node_id.clone(),
                    kind: DagEdgeKind::Data,
                });
            } else if let Some(rule_ref_node) = rule_node_ids.get(dependency) {
                edges.push(DagEdge {
                    from: rule_ref_node.clone(),
                    to: rule_node_id.clone(),
                    kind: DagEdgeKind::Data,
                });
            } else {
                external_inputs.insert(dependency.clone());
            }
        }

        if let Some(next_rules) = &rule.then_rules {
            for next in next_rules {
                let Some(next_node_id) = rule_node_ids.get(next) else {
                    return Err(DagBuildError::UnknownBranchRuleReference {
                        rule_id: rule.id.clone(),
                        referenced_rule: next.clone(),
                    });
                };

                edges.push(DagEdge {
                    from: rule_node_id.clone(),
                    to: next_node_id.clone(),
                    kind: DagEdgeKind::ControlThen,
                });
            }
        }

        if let Some(next_rules) = &rule.else_rules {
            for next in next_rules {
                let Some(next_node_id) = rule_node_ids.get(next) else {
                    return Err(DagBuildError::UnknownBranchRuleReference {
                        rule_id: rule.id.clone(),
                        referenced_rule: next.clone(),
                    });
                };

                edges.push(DagEdge {
                    from: rule_node_id.clone(),
                    to: next_node_id.clone(),
                    kind: DagEdgeKind::ControlElse,
                });
            }
        }

        if let Some(node) = nodes.get_mut(&rule_node_id) {
            node.dependencies = dependencies.into_iter().collect();
            node.external_inputs = external_inputs.into_iter().collect();
        }
    }

    let dag = ExecutionDag { nodes, edges };
    let _ = dag.topological_order()?;
    Ok(dag)
}

fn extract_references(expression: &str) -> Vec<String> {
    let mut refs = BTreeSet::new();
    let mut current = String::new();

    for ch in expression.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '?') {
            current.push(ch);
            continue;
        }

        flush_reference(&mut current, &mut refs);
    }

    flush_reference(&mut current, &mut refs);
    refs.into_iter().collect()
}

fn flush_reference(current: &mut String, refs: &mut BTreeSet<String>) {
    if current.is_empty() {
        return;
    }

    if !looks_like_number(current) {
        refs.insert(current.clone());
    }

    current.clear();
}

fn looks_like_number(value: &str) -> bool {
    value.parse::<f64>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_us1040_execution_dag_from_config() {
        let config = r#"
            [[fields]]
            id = "L1a"
            type = "money"
            source = "input_list"

            [[fields]]
            id = "L1"
            type = "money"
            source = "computed"

            [[fields]]
            id = "L9"
            type = "money"
            source = "computed"

            [[fields]]
            id = "ApplyTo2025"
            type = "percent"
            source = "input_list"

            [[rules]]
            id = "rule_L1"
            target = "L1"
            kind = "sum"
            args = ["L1a"]

            [[rules]]
            id = "rule_L9"
            target = "L9"
            kind = "sum"
            args = ["L1", "ApplyTo2025", "refund.apply_to_next_year_min_pct"]

            [[rules]]
            id = "rule_refund_or_due"
            kind = "branch"
            if = "L9 > 0"
            then = ["rule_refund_L34"]
            else = ["rule_due_L37"]

            [[rules]]
            id = "rule_refund_L34"
            target = "L34"
            kind = "sub"
            args = ["L9", "0"]

            [[rules]]
            id = "rule_due_L37"
            target = "L37"
            kind = "sum"
            args = ["L9", "L1"]
        "#;
        let schema: RuleSchema = toml::from_str(config).unwrap();
        let dag = build_execution_dag(&schema).unwrap();

        assert!(dag.nodes.contains_key("rule:rule_L1"));
        assert!(dag.nodes.contains_key("rule:rule_L9"));
        assert!(dag.nodes.contains_key("field:L1a"));

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

        assert!(dag.topological_order().is_ok());
    }
}
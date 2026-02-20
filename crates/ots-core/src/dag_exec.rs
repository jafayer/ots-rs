use std::collections::BTreeMap;
use std::sync::Arc;

use thiserror::Error;

use crate::dag::{DagEdgeKind, DagNodeKind, ExecutionDag, HookSpec, RuleSchema, RuleSpec};

pub type HookHandler = Arc<dyn Fn(&HookCall<'_>) -> Result<f64, DagExecutionError> + Send + Sync>;

#[derive(Default)]
pub struct HookRegistry {
    specs: BTreeMap<String, HookSpec>,
    handlers: BTreeMap<String, HookHandler>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_schema_hooks(&mut self, schema: &RuleSchema) {
        for (name, spec) in &schema.hooks {
            self.specs.insert(name.clone(), spec.clone());
        }
    }

    pub fn register_hook<F>(&mut self, name: impl Into<String>, handler: F)
    where
        F: Fn(&HookCall<'_>) -> Result<f64, DagExecutionError> + Send + Sync + 'static,
    {
        self.handlers.insert(name.into(), Arc::new(handler));
    }

    pub fn is_registered(&self, name: &str) -> bool {
        self.handlers.contains_key(name)
    }

    fn get_handler(&self, name: &str) -> Option<&HookHandler> {
        self.handlers.get(name)
    }

    fn get_spec(&self, name: &str) -> Option<&HookSpec> {
        self.specs.get(name)
    }
}

#[derive(Debug)]
pub struct HookCall<'a> {
    pub hook_name: &'a str,
    pub rule: &'a RuleSpec,
    pub context: &'a ExecutionContext,
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionContext {
    values: BTreeMap<String, f64>,
}

impl ExecutionContext {
    pub fn with_values(values: BTreeMap<String, f64>) -> Self {
        Self { values }
    }

    pub fn set_value(&mut self, name: impl Into<String>, value: f64) {
        self.values.insert(name.into(), value);
    }

    pub fn get_value(&self, name: &str) -> Option<f64> {
        self.values.get(name).copied()
    }

    pub fn values(&self) -> &BTreeMap<String, f64> {
        &self.values
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub context: ExecutionContext,
    pub branch_decisions: BTreeMap<String, bool>,
    pub executed_rules: Vec<String>,
}

#[derive(Debug, Error)]
pub enum DagExecutionError {
    #[error("rule '{0}' exists in DAG but not in schema")]
    MissingRuleSpec(String),
    #[error("rule '{0}' requires a target field")]
    MissingRuleTarget(String),
    #[error("missing value for reference '{0}'")]
    MissingValue(String),
    #[error("rule '{rule_id}' has invalid arguments: {message}")]
    InvalidRuleArguments { rule_id: String, message: String },
    #[error("unsupported rule kind '{kind}' for rule '{rule_id}'")]
    UnsupportedRuleKind { rule_id: String, kind: String },
    #[error("hook '{hook_name}' on rule '{rule_id}' has no registered implementation")]
    HookNotRegistered { rule_id: String, hook_name: String },
    #[error("hook '{hook_name}' on rule '{rule_id}' is declared in schema as {module}::{function} but not yet implemented")]
    HookNotImplemented {
        rule_id: String,
        hook_name: String,
        module: String,
        function: String,
    },
    #[error("invalid numeric expression '{expr}': {message}")]
    InvalidExpression { expr: String, message: String },
}

pub fn execute_dag(
    schema: &RuleSchema,
    dag: &ExecutionDag,
    initial_values: BTreeMap<String, f64>,
    hook_registry: &HookRegistry,
) -> Result<ExecutionResult, DagExecutionError> {
    let mut context = ExecutionContext::with_values(initial_values);
    let mut branch_decisions = BTreeMap::new();
    let mut executed_rules = Vec::new();

    let topo = dag
        .topological_order()
        .map_err(|error| DagExecutionError::InvalidRuleArguments {
            rule_id: "<dag>".to_string(),
            message: error.to_string(),
        })?;

    let rule_lookup: BTreeMap<&str, &RuleSpec> = schema.rules.iter().map(|rule| (rule.id.as_str(), rule)).collect();

    let mut control_predecessors: BTreeMap<String, Vec<(String, DagEdgeKind)>> = BTreeMap::new();
    for edge in &dag.edges {
        if !matches!(edge.kind, DagEdgeKind::ControlThen | DagEdgeKind::ControlElse) {
            continue;
        }

        let Some(from_rule) = edge.from.strip_prefix("rule:") else {
            continue;
        };
        let Some(to_rule) = edge.to.strip_prefix("rule:") else {
            continue;
        };

        control_predecessors
            .entry(to_rule.to_string())
            .or_default()
            .push((from_rule.to_string(), edge.kind));
    }

    for node_id in topo {
        let Some(node) = dag.nodes.get(&node_id) else {
            continue;
        };

        if node.kind != DagNodeKind::Rule {
            continue;
        }

        let Some(rule_id) = node_id.strip_prefix("rule:") else {
            continue;
        };

        if !is_rule_enabled(rule_id, &control_predecessors, &branch_decisions) {
            continue;
        }

        let Some(rule) = rule_lookup.get(rule_id).copied() else {
            return Err(DagExecutionError::MissingRuleSpec(rule_id.to_string()));
        };

        execute_rule(rule, &mut context, &mut branch_decisions, hook_registry)?;
        executed_rules.push(rule.id.clone());
    }

    Ok(ExecutionResult {
        context,
        branch_decisions,
        executed_rules,
    })
}

fn is_rule_enabled(
    rule_id: &str,
    control_predecessors: &BTreeMap<String, Vec<(String, DagEdgeKind)>>,
    branch_decisions: &BTreeMap<String, bool>,
) -> bool {
    let Some(predecessors) = control_predecessors.get(rule_id) else {
        return true;
    };

    for (branch_rule_id, edge_kind) in predecessors {
        let Some(decision) = branch_decisions.get(branch_rule_id) else {
            return false;
        };

        let enabled = match edge_kind {
            DagEdgeKind::ControlThen => *decision,
            DagEdgeKind::ControlElse => !*decision,
            DagEdgeKind::Data => true,
        };

        if !enabled {
            return false;
        }
    }

    true
}

fn execute_rule(
    rule: &RuleSpec,
    context: &mut ExecutionContext,
    branch_decisions: &mut BTreeMap<String, bool>,
    hook_registry: &HookRegistry,
) -> Result<(), DagExecutionError> {
    match rule.kind.as_str() {
        "sum" => set_target(rule, context, sum_args(rule, context)?)?,
        "sub" => set_target(rule, context, sub_args(rule, context)?)?,
        "mul" => set_target(rule, context, mul_args(rule, context)?)?,
        "max" => set_target(rule, context, max_args(rule, context)?)?,
        "clamp" => set_target(rule, context, clamp_args(rule, context)?)?,
        "alias" => set_target(rule, context, first_arg(rule, context)?)?,
        "hook" => {
            let hook_name = rule.hook.as_ref().ok_or_else(|| DagExecutionError::InvalidRuleArguments {
                rule_id: rule.id.clone(),
                message: "kind='hook' requires hook name".to_string(),
            })?;

            let value = if let Some(handler) = hook_registry.get_handler(hook_name) {
                handler(&HookCall {
                    hook_name,
                    rule,
                    context,
                })?
            } else if let Some(spec) = hook_registry.get_spec(hook_name) {
                return Err(DagExecutionError::HookNotImplemented {
                    rule_id: rule.id.clone(),
                    hook_name: hook_name.clone(),
                    module: spec.module.clone(),
                    function: spec.function.clone(),
                });
            } else {
                return Err(DagExecutionError::HookNotRegistered {
                    rule_id: rule.id.clone(),
                    hook_name: hook_name.clone(),
                });
            };

            set_target(rule, context, value)?;
        }
        "branch" => {
            let condition = rule.if_expr.as_ref().ok_or_else(|| DagExecutionError::InvalidRuleArguments {
                rule_id: rule.id.clone(),
                message: "kind='branch' requires if expression".to_string(),
            })?;
            let decision = eval_condition(condition, context)?;
            branch_decisions.insert(rule.id.clone(), decision);
        }
        other => {
            return Err(DagExecutionError::UnsupportedRuleKind {
                rule_id: rule.id.clone(),
                kind: other.to_string(),
            })
        }
    }

    Ok(())
}

fn set_target(rule: &RuleSpec, context: &mut ExecutionContext, value: f64) -> Result<(), DagExecutionError> {
    let Some(target) = &rule.target else {
        return Err(DagExecutionError::MissingRuleTarget(rule.id.clone()));
    };

    context.set_value(target.clone(), value);
    Ok(())
}

fn args_for<'a>(rule: &'a RuleSpec) -> &'a [String] {
    rule.args.as_deref().unwrap_or(&[])
}

fn eval_arg(arg: &str, context: &ExecutionContext) -> Result<f64, DagExecutionError> {
    eval_numeric(arg, context)
}

fn sum_args(rule: &RuleSpec, context: &ExecutionContext) -> Result<f64, DagExecutionError> {
    let mut total = 0.0;
    for arg in args_for(rule) {
        total += eval_arg(arg, context)?;
    }
    Ok(total)
}

fn sub_args(rule: &RuleSpec, context: &ExecutionContext) -> Result<f64, DagExecutionError> {
    let args = args_for(rule);
    let (first, rest) = args.split_first().ok_or_else(|| DagExecutionError::InvalidRuleArguments {
        rule_id: rule.id.clone(),
        message: "sub requires at least one argument".to_string(),
    })?;

    let mut value = eval_arg(first, context)?;
    for arg in rest {
        value -= eval_arg(arg, context)?;
    }
    Ok(value)
}

fn mul_args(rule: &RuleSpec, context: &ExecutionContext) -> Result<f64, DagExecutionError> {
    let args = args_for(rule);
    if args.is_empty() {
        return Err(DagExecutionError::InvalidRuleArguments {
            rule_id: rule.id.clone(),
            message: "mul requires at least one argument".to_string(),
        });
    }

    let mut value = 1.0;
    for arg in args {
        value *= eval_arg(arg, context)?;
    }
    Ok(value)
}

fn max_args(rule: &RuleSpec, context: &ExecutionContext) -> Result<f64, DagExecutionError> {
    let args = args_for(rule);
    let (first, rest) = args.split_first().ok_or_else(|| DagExecutionError::InvalidRuleArguments {
        rule_id: rule.id.clone(),
        message: "max requires at least one argument".to_string(),
    })?;

    let mut value = eval_arg(first, context)?;
    for arg in rest {
        value = value.max(eval_arg(arg, context)?);
    }
    Ok(value)
}

fn clamp_args(rule: &RuleSpec, context: &ExecutionContext) -> Result<f64, DagExecutionError> {
    let args = args_for(rule);
    if args.len() != 3 {
        return Err(DagExecutionError::InvalidRuleArguments {
            rule_id: rule.id.clone(),
            message: "clamp requires exactly 3 arguments: value, min, max".to_string(),
        });
    }

    let value = eval_arg(&args[0], context)?;
    let min = eval_arg(&args[1], context)?;
    let max = eval_arg(&args[2], context)?;
    Ok(value.clamp(min, max))
}

fn first_arg(rule: &RuleSpec, context: &ExecutionContext) -> Result<f64, DagExecutionError> {
    let args = args_for(rule);
    if args.len() != 1 {
        return Err(DagExecutionError::InvalidRuleArguments {
            rule_id: rule.id.clone(),
            message: "alias requires exactly one argument".to_string(),
        });
    }

    eval_arg(&args[0], context)
}

fn eval_condition(expr: &str, context: &ExecutionContext) -> Result<bool, DagExecutionError> {
    let candidates = [">=", "<=", "==", "!=", ">", "<"];
    for op in candidates {
        if let Some((lhs, rhs)) = split_top_level(expr, op) {
            let left = eval_numeric(lhs.trim(), context)?;
            let right = eval_numeric(rhs.trim(), context)?;
            let result = match op {
                ">=" => left >= right,
                "<=" => left <= right,
                "==" => (left - right).abs() < 1e-9,
                "!=" => (left - right).abs() >= 1e-9,
                ">" => left > right,
                "<" => left < right,
                _ => unreachable!(),
            };
            return Ok(result);
        }
    }

    Ok(eval_numeric(expr, context)? != 0.0)
}

fn split_top_level<'a>(expr: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    let bytes = expr.as_bytes();
    let mut depth = 0_i32;
    let op_bytes = op.as_bytes();
    let mut idx = 0_usize;

    while idx + op_bytes.len() <= bytes.len() {
        let ch = bytes[idx] as char;
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }

        if depth == 0 && &bytes[idx..idx + op_bytes.len()] == op_bytes {
            let left = &expr[..idx];
            let right = &expr[idx + op_bytes.len()..];
            return Some((left, right));
        }

        idx += 1;
    }

    None
}

fn eval_numeric(expr: &str, context: &ExecutionContext) -> Result<f64, DagExecutionError> {
    let tokens = tokenize(expr)?;
    let mut parser = NumericParser {
        tokens: &tokens,
        idx: 0,
        context,
        expr,
    };
    let value = parser.parse_expression()?;
    if parser.idx != parser.tokens.len() {
        return Err(DagExecutionError::InvalidExpression {
            expr: expr.to_string(),
            message: "unexpected trailing tokens".to_string(),
        });
    }
    Ok(value)
}

#[derive(Debug, Clone, PartialEq)]
enum NumericToken {
    Number(f64),
    Identifier(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

fn tokenize(expr: &str) -> Result<Vec<NumericToken>, DagExecutionError> {
    let chars: Vec<char> = expr.chars().collect();
    let mut idx = 0_usize;
    let mut tokens = Vec::new();

    while idx < chars.len() {
        let ch = chars[idx];

        if ch.is_whitespace() {
            idx += 1;
            continue;
        }

        match ch {
            '+' => {
                tokens.push(NumericToken::Plus);
                idx += 1;
            }
            '-' => {
                tokens.push(NumericToken::Minus);
                idx += 1;
            }
            '*' => {
                tokens.push(NumericToken::Star);
                idx += 1;
            }
            '/' => {
                tokens.push(NumericToken::Slash);
                idx += 1;
            }
            '(' => {
                tokens.push(NumericToken::LParen);
                idx += 1;
            }
            ')' => {
                tokens.push(NumericToken::RParen);
                idx += 1;
            }
            '0'..='9' | '.' => {
                let start = idx;
                idx += 1;
                while idx < chars.len() && (chars[idx].is_ascii_digit() || chars[idx] == '.') {
                    idx += 1;
                }

                let raw: String = chars[start..idx].iter().collect();
                let value = raw.parse::<f64>().map_err(|_| DagExecutionError::InvalidExpression {
                    expr: expr.to_string(),
                    message: format!("invalid number '{raw}'"),
                })?;
                tokens.push(NumericToken::Number(value));
            }
            _ => {
                if ch.is_ascii_alphabetic() || ch == '_' {
                    let start = idx;
                    idx += 1;
                    while idx < chars.len()
                        && (chars[idx].is_ascii_alphanumeric() || matches!(chars[idx], '_' | '.' | '?'))
                    {
                        idx += 1;
                    }
                    let raw: String = chars[start..idx].iter().collect();
                    tokens.push(NumericToken::Identifier(raw));
                } else {
                    return Err(DagExecutionError::InvalidExpression {
                        expr: expr.to_string(),
                        message: format!("unexpected character '{ch}'"),
                    });
                }
            }
        }
    }

    Ok(tokens)
}

struct NumericParser<'a> {
    tokens: &'a [NumericToken],
    idx: usize,
    context: &'a ExecutionContext,
    expr: &'a str,
}

impl NumericParser<'_> {
    fn parse_expression(&mut self) -> Result<f64, DagExecutionError> {
        let mut value = self.parse_term()?;

        while let Some(token) = self.tokens.get(self.idx) {
            match token {
                NumericToken::Plus => {
                    self.idx += 1;
                    value += self.parse_term()?;
                }
                NumericToken::Minus => {
                    self.idx += 1;
                    value -= self.parse_term()?;
                }
                _ => break,
            }
        }

        Ok(value)
    }

    fn parse_term(&mut self) -> Result<f64, DagExecutionError> {
        let mut value = self.parse_factor()?;

        while let Some(token) = self.tokens.get(self.idx) {
            match token {
                NumericToken::Star => {
                    self.idx += 1;
                    value *= self.parse_factor()?;
                }
                NumericToken::Slash => {
                    self.idx += 1;
                    value /= self.parse_factor()?;
                }
                _ => break,
            }
        }

        Ok(value)
    }

    fn parse_factor(&mut self) -> Result<f64, DagExecutionError> {
        let Some(token) = self.tokens.get(self.idx) else {
            return Err(DagExecutionError::InvalidExpression {
                expr: self.expr.to_string(),
                message: "unexpected end of expression".to_string(),
            });
        };

        match token {
            NumericToken::Number(value) => {
                self.idx += 1;
                Ok(*value)
            }
            NumericToken::Identifier(name) => {
                self.idx += 1;
                Ok(self.context.get_value(name).unwrap_or(0.0))
            }
            NumericToken::Minus => {
                self.idx += 1;
                Ok(-self.parse_factor()?)
            }
            NumericToken::LParen => {
                self.idx += 1;
                let value = self.parse_expression()?;
                match self.tokens.get(self.idx) {
                    Some(NumericToken::RParen) => {
                        self.idx += 1;
                        Ok(value)
                    }
                    _ => Err(DagExecutionError::InvalidExpression {
                        expr: self.expr.to_string(),
                        message: "unclosed parenthesis".to_string(),
                    }),
                }
            }
            _ => Err(DagExecutionError::InvalidExpression {
                expr: self.expr.to_string(),
                message: "unexpected token".to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::{build_execution_dag, RuleSchema};

    #[test]
    fn executes_builtins_branch_and_hook() {
        let config = r#"
            [[fields]]
            id = "A"
            type = "money"
            source = "input"

            [[fields]]
            id = "B"
            type = "money"
            source = "input"

            [[fields]]
            id = "C"
            type = "money"
            source = "input"

            [[fields]]
            id = "Min"
            type = "money"
            source = "input"

            [[fields]]
            id = "Max"
            type = "money"
            source = "input"

            [[fields]]
            id = "X"
            type = "money"
            source = "computed"

            [[fields]]
            id = "Y"
            type = "money"
            source = "computed"

            [[fields]]
            id = "Clamped"
            type = "money"
            source = "computed"

            [[fields]]
            id = "BranchYes"
            type = "money"
            source = "computed"

            [[fields]]
            id = "BranchNo"
            type = "money"
            source = "computed"

            [[fields]]
            id = "HookOut"
            type = "money"
            source = "computed"

            [[rules]]
            id = "r_sum"
            target = "X"
            kind = "sum"
            args = ["A", "B", "2"]

            [[rules]]
            id = "r_sub"
            target = "Y"
            kind = "sub"
            args = ["X", "C"]

            [[rules]]
            id = "r_clamp"
            target = "Clamped"
            kind = "clamp"
            args = ["Y", "Min", "Max"]

            [[rules]]
            id = "r_branch"
            kind = "branch"
            if = "Y > 0"
            then = ["r_then"]
            else = ["r_else"]

            [[rules]]
            id = "r_then"
            target = "BranchYes"
            kind = "alias"
            args = ["Y"]

            [[rules]]
            id = "r_else"
            target = "BranchNo"
            kind = "alias"
            args = ["Y"]

            [[rules]]
            id = "r_hook"
            target = "HookOut"
            kind = "hook"
            hook = "demo::calc"
            args = ["Y"]

            [hooks]
            "demo::calc" = { module = "forms::demo", fn = "calc" }
        "#;

        let schema: RuleSchema = toml::from_str(config).unwrap();
        let dag = build_execution_dag(&schema).unwrap();

        let mut hooks = HookRegistry::new();
        hooks.register_schema_hooks(&schema);
        hooks.register_hook("demo::calc", |call| {
            let y = call.context.get_value("Y").unwrap_or(0.0);
            Ok(y * 10.0)
        });

        let mut values = BTreeMap::new();
        values.insert("A".to_string(), 10.0);
        values.insert("B".to_string(), 5.0);
        values.insert("C".to_string(), 4.0);
        values.insert("Min".to_string(), 0.0);
        values.insert("Max".to_string(), 8.0);

        let result = execute_dag(&schema, &dag, values, &hooks).unwrap();

        assert_eq!(result.context.get_value("X"), Some(17.0));
        assert_eq!(result.context.get_value("Y"), Some(13.0));
        assert_eq!(result.context.get_value("Clamped"), Some(8.0));
        assert_eq!(result.context.get_value("BranchYes"), Some(13.0));
        assert_eq!(result.context.get_value("BranchNo"), None);
        assert_eq!(result.context.get_value("HookOut"), Some(130.0));
        assert_eq!(result.branch_decisions.get("r_branch"), Some(&true));
    }

    #[test]
    fn returns_not_implemented_for_declared_but_unregistered_hook() {
        let config = r#"
            [[fields]]
            id = "L1"
            type = "money"
            source = "computed"

            [[rules]]
            id = "r_hook"
            target = "L1"
            kind = "hook"
            hook = "demo::missing"

            [hooks]
            "demo::missing" = { module = "forms::demo", fn = "missing" }
        "#;

        let schema: RuleSchema = toml::from_str(config).unwrap();
        let dag = build_execution_dag(&schema).unwrap();

        let mut hooks = HookRegistry::new();
        hooks.register_schema_hooks(&schema);

        let err = execute_dag(&schema, &dag, BTreeMap::new(), &hooks).unwrap_err();
        assert!(matches!(
            err,
            DagExecutionError::HookNotImplemented {
                ref hook_name,
                ref module,
                ref function,
                ..
            } if hook_name == "demo::missing" && module == "forms::demo" && function == "missing"
        ));
    }

    #[test]
    fn evaluates_arithmetic_expressions() {
        let mut context = ExecutionContext::default();
        context.set_value("L24", 100.0);
        context.set_value("L33", 60.0);
        context.set_value("ApplyTo2025", 25.0);

        assert_eq!(eval_numeric("L24 - L33", &context).unwrap(), 40.0);
        assert_eq!(eval_numeric("(1 - ApplyTo2025/100)", &context).unwrap(), 0.75);
        assert!(eval_condition("L33 > L24", &context).unwrap() == false);
    }

}

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use chrono::NaiveDate;
use ots_core::dag_exec::{DagExecutionError, HookCall, HookRegistry};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FormHooksError {
    #[error("failed reading constants file '{path}': {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("failed parsing constants TOML: {0}")]
    ParseToml(#[from] toml::de::Error),
    #[error("invalid tax table data for status '{status}': breakpoints/rates size mismatch")]
    InvalidTaxTable { status: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct Us1040Constants {
    pub status: StatusCodes,
    pub tax_table: TaxTable,
    pub standard_deduction: StandardDeduction,
    pub social_security_taxability: SocialSecurityTaxability,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatusCodes {
    pub single: i64,
    pub married_filing_jointly: i64,
    pub married_filing_separately: i64,
    pub head_of_household: i64,
    pub widow: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaxTable {
    pub brackets: TaxBracketsByStatus,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaxBracketsByStatus {
    pub single: TaxBracketSet,
    pub married_filing_jointly: TaxBracketSet,
    pub married_filing_separately: TaxBracketSet,
    pub head_of_household: TaxBracketSet,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaxBracketSet {
    pub breakpoints: Vec<f64>,
    pub rates: Vec<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StandardDeduction {
    pub base: StandardDeductionBase,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StandardDeductionBase {
    pub single: f64,
    pub married_filing_jointly: f64,
    pub married_filing_separately: f64,
    pub head_of_household: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SocialSecurityTaxability {
    pub taxable_fraction: f64,
}

pub fn load_constants_from_file<P: AsRef<Path>>(path: P) -> Result<Us1040Constants, FormHooksError> {
    let path_ref = path.as_ref();
    let raw = fs::read_to_string(path_ref).map_err(|source| FormHooksError::Io {
        path: path_ref.display().to_string(),
        source,
    })?;
    let constants: Us1040Constants = toml::from_str(&raw)?;
    validate_constants(&constants)?;
    Ok(constants)
}

pub fn register_hooks(registry: &mut HookRegistry, constants: Us1040Constants) {
    let constants = Arc::new(constants);

    {
        let constants = Arc::clone(&constants);
        registry.register_hook("us1040::socsec_taxability", move |call| {
            let benefits = value_or_zero(call, "L6a");
            Ok((benefits * constants.social_security_taxability.taxable_fraction).max(0.0))
        });
    }

    registry.register_hook("us1040::capital_gains_pipeline", |call| {
        let derived = value_or_zero(call, "CapGainsTaxable");
        if derived != 0.0 {
            return Ok(derived);
        }

        let schedule_d_total = value_or_zero(call, "D16");
        if schedule_d_total != 0.0 {
            return Ok(schedule_d_total);
        }

        let short_term = value_or_zero(call, "D7");
        let long_term = value_or_zero(call, "D15");
        Ok(short_term + long_term)
    });

    registry.register_hook("us1040::schedule1_total_additional_income", |call| {
        let aggregate = value_or_zero(call, "Schedule1AdditionalIncome");
        if aggregate != 0.0 {
            return Ok(aggregate);
        }

        let precomputed = value_or_zero(call, "S1_10");
        if precomputed != 0.0 {
            return Ok(precomputed);
        }

        let s1_9 = {
            let s1_8a = value_or_zero(call, "S1_8a").abs();
            let s1_8d = value_or_zero(call, "S1_8d").abs();
            let s1_8s = value_or_zero(call, "S1_8s").abs();
            -s1_8a
                + value_or_zero(call, "S1_8b")
                + value_or_zero(call, "S1_8c")
                - s1_8d
                + value_or_zero(call, "S1_8e")
                + value_or_zero(call, "S1_8f")
                + value_or_zero(call, "S1_8g")
                + value_or_zero(call, "S1_8h")
                + value_or_zero(call, "S1_8i")
                + value_or_zero(call, "S1_8j")
                + value_or_zero(call, "S1_8k")
                + value_or_zero(call, "S1_8l")
                + value_or_zero(call, "S1_8m")
                + value_or_zero(call, "S1_8n")
                + value_or_zero(call, "S1_8o")
                + value_or_zero(call, "S1_8p")
                + value_or_zero(call, "S1_8q")
                + value_or_zero(call, "S1_8r")
                - s1_8s
                + value_or_zero(call, "S1_8t")
                + value_or_zero(call, "S1_8u")
                + value_or_zero(call, "S1_8v")
                + value_or_zero(call, "S1_8z")
        };

        Ok(value_or_zero(call, "S1_1")
            + value_or_zero(call, "S1_2a")
            + value_or_zero(call, "S1_3")
            + value_or_zero(call, "S1_4")
            + value_or_zero(call, "S1_5")
            + value_or_zero(call, "S1_6")
            + value_or_zero(call, "S1_7")
            + s1_9)
    });

    registry.register_hook("us1040::schedule1_adjustments_total", |call| {
        let aggregate = value_or_zero(call, "Schedule1Adjustments");
        if aggregate != 0.0 {
            return Ok(aggregate);
        }

        let precomputed = value_or_zero(call, "S1_26");
        if precomputed != 0.0 {
            return Ok(precomputed);
        }

        let detail_keys = [
            "S1_11", "S1_12", "S1_13", "S1_14", "S1_15", "S1_16", "S1_17", "S1_18", "S1_19a", "S1_20",
            "S1_21", "S1_23", "S1_24a", "S1_24b", "S1_24c", "S1_24d", "S1_24e", "S1_24f", "S1_24g", "S1_24h",
            "S1_24i", "S1_24j", "S1_24k", "S1_24z",
        ];

        Ok(sum_keys(call, &detail_keys))
    });

    registry.register_hook("us1040::schedule2_line3_tax", |call| {
        let aggregate = value_or_zero(call, "Schedule2Line3Taxes");
        if aggregate != 0.0 {
            return Ok(aggregate);
        }

        let precomputed = value_or_zero(call, "S2_3");
        if precomputed != 0.0 {
            return Ok(precomputed);
        }

        let detail_keys = ["S2_1a", "S2_1b", "S2_1c", "S2_1d", "S2_1e", "S2_1f", "S2_1y", "S2_2"];
        Ok(sum_keys(call, &detail_keys))
    });

    {
        let constants = Arc::clone(&constants);
        registry.register_hook("us1040::standard_or_itemized_deduction", move |call| {
            let status = value_or_zero(call, "status") as i64;
            let base = standard_deduction_for_status(call, &constants, status);
            let schedule_a = value_or_zero(call, "ScheduleAItemized");
            Ok(base.max(schedule_a))
        });
    }

    registry.register_hook("us1040::taxable_income_after_dependency_checks", |call| {
        Ok(value_or_zero(call, "L14").max(0.0))
    });

    {
        let constants = Arc::clone(&constants);
        registry.register_hook("us1040::compute_tax_line", move |call| {
            let taxable_income = value_or_zero(call, "L15").max(0.0);
            let status = value_or_zero(call, "status") as i64;
            let qualified_dividends = value_or_zero(call, "L3a").max(0.0).min(taxable_income);

            let schedule_d_long = value_or_zero(call, "D15");
            let schedule_d_total = value_or_zero(call, "D16");
            let line7_capital_gain = value_or_zero(call, "L7");

            let capital_gain_component = if schedule_d_long != 0.0 || schedule_d_total != 0.0 {
                schedule_d_long.min(schedule_d_total).max(0.0)
            } else {
                line7_capital_gain.max(0.0)
            };

            if qualified_dividends > 0.0 || capital_gain_component > 0.0 {
                return Ok(qualified_dividend_and_capital_gain_tax(
                    taxable_income,
                    qualified_dividends,
                    capital_gain_component,
                    status,
                    &constants,
                ));
            }

            Ok(tax_rate_function(taxable_income, status, &constants))
        });
    }

    registry.register_hook("us1040::schedule2_additional_taxes_total", |call| {
        let aggregate = value_or_zero(call, "Schedule2AdditionalTaxes");
        if aggregate != 0.0 {
            return Ok(aggregate);
        }

        let precomputed = value_or_zero(call, "S2_21");
        if precomputed != 0.0 {
            return Ok(precomputed);
        }

        let detail_keys = [
            "S2_4", "S2_5", "S2_6", "S2_8", "S2_9", "S2_11", "S2_12", "S2_13", "S2_14",
            "S2_15", "S2_16", "S2_17a", "S2_17b", "S2_17c", "S2_17d", "S2_17e",
            "S2_17f", "S2_17g", "S2_17h", "S2_17i", "S2_17j", "S2_17k", "S2_17l",
            "S2_17m", "S2_17n", "S2_17o", "S2_17p", "S2_17q", "S2_17z", "S2_19",
            "S2_20",
        ];

        Ok(sum_keys(call, &detail_keys))
    });

    registry.register_hook("us1040::schedule3_nonrefundable_credits_total", |call| {
        let aggregate = value_or_zero(call, "Schedule3NonrefundableCredits");
        if aggregate != 0.0 {
            return Ok(aggregate);
        }

        let already_aggregated = value_or_zero(call, "S3_8");
        if already_aggregated != 0.0 {
            return Ok(already_aggregated);
        }

        let detail_keys = [
            "S3_1", "S3_2", "S3_3", "S3_4", "S3_5a", "S3_5b", "S3_6a", "S3_6b",
            "S3_6c", "S3_6d", "S3_6e", "S3_6f", "S3_6g", "S3_6h", "S3_6i", "S3_6j",
            "S3_6k", "S3_6l", "S3_6m", "S3_6z", "S3_7",
        ];

        Ok(sum_keys(call, &detail_keys))
    });

    registry.register_hook("us1040::schedule3_refundable_total", |call| {
        let aggregate = value_or_zero(call, "Schedule3RefundableCredits");
        if aggregate != 0.0 {
            return Ok(aggregate);
        }

        let already_aggregated = value_or_zero(call, "S3_15");
        if already_aggregated != 0.0 {
            return Ok(already_aggregated);
        }

        let detail_keys = [
            "S3_9", "S3_10", "S3_11", "S3_12", "S3_13a", "S3_13b", "S3_13c", "S3_13d",
        ];

        Ok(sum_keys(call, &detail_keys))
    });
}

pub fn register_hooks_from_constants_file<P: AsRef<Path>>(
    registry: &mut HookRegistry,
    path: P,
) -> Result<(), FormHooksError> {
    let constants = load_constants_from_file(path)?;
    register_hooks(registry, constants);
    Ok(())
}

fn validate_constants(constants: &Us1040Constants) -> Result<(), FormHooksError> {
    let sets = [
        ("single", &constants.tax_table.brackets.single),
        (
            "married_filing_jointly",
            &constants.tax_table.brackets.married_filing_jointly,
        ),
        (
            "married_filing_separately",
            &constants.tax_table.brackets.married_filing_separately,
        ),
        (
            "head_of_household",
            &constants.tax_table.brackets.head_of_household,
        ),
    ];

    for (status, set) in sets {
        if set.breakpoints.len() != set.rates.len() + 1 {
            return Err(FormHooksError::InvalidTaxTable {
                status: status.to_string(),
            });
        }
    }

    Ok(())
}

fn standard_deduction_for_status(call: &HookCall<'_>, constants: &Us1040Constants, status: i64) -> f64 {
    let boxes_checked = std_deduction_boxes_checked(call, constants, status);

    if status == constants.status.single {
        constants.standard_deduction.base.single + 2_000.0 * f64::from(boxes_checked.min(2))
    } else if status == constants.status.married_filing_jointly || status == constants.status.widow {
        constants.standard_deduction.base.married_filing_jointly + 1_600.0 * f64::from(boxes_checked.min(4))
    } else if status == constants.status.married_filing_separately {
        constants.standard_deduction.base.married_filing_separately + 1_600.0 * f64::from(boxes_checked.min(4))
    } else if status == constants.status.head_of_household {
        constants.standard_deduction.base.head_of_household + 2_000.0 * f64::from(boxes_checked.min(2))
    } else {
        constants.standard_deduction.base.single
    }
}

fn std_deduction_boxes_checked(call: &HookCall<'_>, constants: &Us1040Constants, status: i64) -> u32 {
    let mut boxes = 0_u32;

    if value_or_zero(call, "You_65+Over?") != 0.0 {
        boxes += 1;
    }
    if value_or_zero(call, "You_Blind?") != 0.0 {
        boxes += 1;
    }

    if status == constants.status.married_filing_jointly
        || status == constants.status.married_filing_separately
        || status == constants.status.widow
    {
        if value_or_zero(call, "Spouse_65+Over?") != 0.0 {
            boxes += 1;
        }
        if value_or_zero(call, "Spouse_Blind?") != 0.0 {
            boxes += 1;
        }
    }

    boxes
}

fn brackets_for_status(constants: &Us1040Constants, status: i64) -> &TaxBracketSet {
    if status == constants.status.single {
        &constants.tax_table.brackets.single
    } else if status == constants.status.married_filing_jointly || status == constants.status.widow {
        &constants.tax_table.brackets.married_filing_jointly
    } else if status == constants.status.married_filing_separately {
        &constants.tax_table.brackets.married_filing_separately
    } else if status == constants.status.head_of_household {
        &constants.tax_table.brackets.head_of_household
    } else {
        &constants.tax_table.brackets.single
    }
}

fn compute_progressive_tax(
    taxable_income: f64,
    brackets: &TaxBracketSet,
) -> Result<f64, DagExecutionError> {
    let mut tax = 0.0;

    for idx in 0..brackets.rates.len() {
        let bracket_start = brackets.breakpoints[idx];
        let bracket_end = brackets.breakpoints[idx + 1];

        if taxable_income <= bracket_start {
            break;
        }

        let taxable_in_bracket = (taxable_income.min(bracket_end) - bracket_start).max(0.0);
        tax += taxable_in_bracket * brackets.rates[idx];

        if taxable_income <= bracket_end {
            break;
        }
    }

    Ok(tax)
}

fn tax_rate_function(taxable_income: f64, status: i64, constants: &Us1040Constants) -> f64 {
    let income = taxable_income.max(0.0);
    if income < 100_000.0 {
        let step = if income < 25.0 {
            5.0
        } else if income < 3_000.0 {
            25.0
        } else {
            50.0
        };

        let quantized = (income / step).floor() * step + 0.5 * step;
        return tax_rate_formula(quantized, status, constants).round();
    }

    tax_rate_formula(income, status, constants)
}

fn tax_rate_formula(taxable_income: f64, status: i64, constants: &Us1040Constants) -> f64 {
    let brackets = brackets_for_status(constants, status);
    compute_progressive_tax(taxable_income, brackets).unwrap_or(0.0)
}

fn qualified_dividend_and_capital_gain_tax(
    taxable_income: f64,
    qualified_dividends: f64,
    capital_gain_component: f64,
    status: i64,
    constants: &Us1040Constants,
) -> f64 {
    let q1 = taxable_income.max(0.0);
    let q2 = qualified_dividends.max(0.0).min(q1);
    let q3 = capital_gain_component.max(0.0).min(q1);
    let q4 = q2 + q3;
    let q5 = (q1 - q4).max(0.0);
    let q6 = qd_zero_rate_ceiling(status, constants);
    let q7 = q1.min(q6);
    let q8 = q5.min(q7);
    let q9 = q7 - q8;
    let q10 = q1.min(q4);
    let q11 = q9;
    let q12 = q10 - q11;
    let q13 = qd_fifteen_rate_ceiling(status, constants);
    let q14 = q1.min(q13);
    let q15 = q5 + q9;
    let q16 = (q14 - q15).max(0.0);
    let q17 = q12.min(q16);
    let q18 = 0.15 * q17;
    let q19 = q9 + q17;
    let q20 = q10 - q19;
    let q21 = 0.20 * q20;
    let q22 = tax_rate_function(q5, status, constants);
    let q23 = q18 + q21 + q22;
    let q24 = tax_rate_function(q1, status, constants);
    q23.min(q24)
}

fn qd_zero_rate_ceiling(status: i64, constants: &Us1040Constants) -> f64 {
    if status == constants.status.single || status == constants.status.married_filing_separately {
        48_350.0
    } else if status == constants.status.married_filing_jointly || status == constants.status.widow {
        96_700.0
    } else if status == constants.status.head_of_household {
        64_750.0
    } else {
        48_350.0
    }
}

fn qd_fifteen_rate_ceiling(status: i64, constants: &Us1040Constants) -> f64 {
    if status == constants.status.single {
        533_400.0
    } else if status == constants.status.married_filing_separately {
        300_000.0
    } else if status == constants.status.married_filing_jointly || status == constants.status.widow {
        600_050.0
    } else if status == constants.status.head_of_household {
        566_700.0
    } else {
        533_400.0
    }
}

fn value_or_zero(call: &HookCall<'_>, key: &str) -> f64 {
    call.context.get_value(key).unwrap_or(0.0)
}

fn sum_keys(call: &HookCall<'_>, keys: &[&str]) -> f64 {
    keys.iter().map(|key| value_or_zero(call, key)).sum()
}

pub fn seed_derived_input_values(input: &str, values: &mut BTreeMap<String, f64>) {
    let cap_gains = derive_cap_gains_from_input(input);
    if cap_gains.total != 0.0 {
        values.insert("CapGainsTaxable".to_string(), cap_gains.total);
        values.insert("D16".to_string(), cap_gains.total);
    }
    if cap_gains.long_term != 0.0 {
        values.insert("D15".to_string(), cap_gains.long_term);
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct DerivedCapGains {
    total: f64,
    long_term: f64,
}

fn derive_cap_gains_from_input(input: &str) -> DerivedCapGains {
    let mut in_cap_gains = false;
    let mut stage = 0_u8;
    let mut buy = 0.0;
    let mut buy_date: Option<NaiveDate> = None;
    let mut sell = 0.0;
    let mut sell_date: Option<NaiveDate> = None;
    let mut derived = DerivedCapGains::default();

    for raw_line in input.lines() {
        let stripped = strip_brace_comment(raw_line);
        let line = stripped.trim();
        if line.is_empty() {
            continue;
        }

        let normalized = normalize_name(line);
        if normalized.starts_with("capgains") {
            in_cap_gains = true;
            stage = 0;
            continue;
        }

        if !in_cap_gains {
            continue;
        }

        if line.starts_with(';') {
            in_cap_gains = false;
            stage = 0;
            continue;
        }

        match stage {
            0 => {
                if let Some(value) = first_numeric_token(line) {
                    buy = value;
                    buy_date = first_date_token(line);
                    stage = 1;
                }
            }
            1 => {
                if let Some(value) = first_numeric_token(line) {
                    sell = value;
                    sell_date = first_date_token(line);
                    stage = 2;
                }
            }
            _ => {
                let adjustment = first_numeric_token(line).unwrap_or(0.0);
                let gain = sell - buy.abs() + adjustment;
                derived.total += gain;
                if is_long_term_trade(buy_date, sell_date) {
                    derived.long_term += gain;
                }
                stage = 0;
            }
        }
    }

    derived
}

fn strip_brace_comment(line: &str) -> String {
    if let Some((head, _)) = line.split_once('{') {
        head.to_string()
    } else {
        line.to_string()
    }
}

fn first_numeric_token(line: &str) -> Option<f64> {
    line.split_whitespace().find_map(parse_money_like_token)
}

fn first_date_token(line: &str) -> Option<NaiveDate> {
    line.split_whitespace().find_map(parse_date_token)
}

fn parse_money_like_token(token: &str) -> Option<f64> {
    let has_digit = token.chars().any(|ch| ch.is_ascii_digit());
    if !has_digit || is_date_like_token(token) {
        return None;
    }

    let normalized = token.replace(',', "");
    normalized.parse::<f64>().ok()
}

fn is_date_like_token(token: &str) -> bool {
    let separators = token.matches('-').count() + token.matches('/').count();
    if separators < 2 {
        return false;
    }

    token
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch == '-' || ch == '/')
}

fn parse_date_token(token: &str) -> Option<NaiveDate> {
    if !is_date_like_token(token) {
        return None;
    }

    let separator = if token.contains('-') { '-' } else { '/' };
    let mut parts = token.split(separator);
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    let year_raw = parts.next()?.parse::<i32>().ok()?;
    if parts.next().is_some() {
        return None;
    }

    let year = if year_raw < 100 {
        if year_raw <= 30 {
            2000 + year_raw
        } else {
            1900 + year_raw
        }
    } else {
        year_raw
    };

    NaiveDate::from_ymd_opt(year, month, day)
}

fn is_long_term_trade(buy_date: Option<NaiveDate>, sell_date: Option<NaiveDate>) -> bool {
    let (Some(buy), Some(sell)) = (buy_date, sell_date) else {
        return false;
    };

    sell.signed_duration_since(buy).num_days() > 365
}

fn normalize_name(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use ots_core::dag::{build_execution_dag, RuleSchema};
    use ots_core::dag_exec::{execute_dag, HookRegistry};

    use super::*;

    #[test]
    fn loads_constants_from_us1040_toml() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/2025/us1040.toml");
        let constants = load_constants_from_file(path).unwrap();

        assert_eq!(constants.status.single, 1);
        assert_eq!(constants.standard_deduction.base.married_filing_jointly, 31_500.0);
        assert_eq!(constants.tax_table.brackets.single.rates.len(), 7);
    }

    #[test]
    fn registers_all_us1040_hooks() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/2025/us1040.toml");
        let constants = load_constants_from_file(path).unwrap();

        let mut registry = HookRegistry::new();
        register_hooks(&mut registry, constants);

        let hook_names = [
            "us1040::socsec_taxability",
            "us1040::capital_gains_pipeline",
            "us1040::schedule1_total_additional_income",
            "us1040::schedule1_adjustments_total",
            "us1040::standard_or_itemized_deduction",
            "us1040::taxable_income_after_dependency_checks",
            "us1040::compute_tax_line",
            "us1040::schedule2_additional_taxes_total",
            "us1040::schedule3_nonrefundable_credits_total",
            "us1040::schedule3_refundable_total",
        ];

        for hook in hook_names {
            assert!(registry.is_registered(hook), "missing hook registration: {hook}");
        }
    }

    #[test]
    fn executes_compute_tax_hook_using_toml_brackets() {
        let config = r#"
            [[fields]]
            id = "status"
            type = "enum"
            source = "input"

            [[fields]]
            id = "L15"
            type = "money"
            source = "input"

            [[fields]]
            id = "L16"
            type = "money"
            source = "computed"

            [[rules]]
            id = "rule_L16"
            target = "L16"
            kind = "hook"
            hook = "us1040::compute_tax_line"
            args = ["status", "L15"]

            [hooks]
            "us1040::compute_tax_line" = { module = "forms::y2025::us1040::tax", fn = "compute_l16" }
        "#;

        let schema: RuleSchema = toml::from_str(config).unwrap();
        let dag = build_execution_dag(&schema).unwrap();

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/2025/us1040.toml");
        let constants = load_constants_from_file(path).unwrap();

        let mut registry = HookRegistry::new();
        registry.register_schema_hooks(&schema);
        register_hooks(&mut registry, constants);

        let mut values = BTreeMap::new();
        values.insert("status".to_string(), 1.0);
        values.insert("L15".to_string(), 50_000.0);

        let result = execute_dag(&schema, &dag, values, &registry).unwrap();
        let l16 = result.context.get_value("L16").unwrap();
        assert!(l16 > 5_000.0);
    }
}

use std::fs;
use std::path::Path;
use std::sync::Arc;

use ots_core::dag_exec::{HookCall, HookRegistry};
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
	#[error("invalid tax formula table for status group '{status_group}'")]
	InvalidTaxFormula { status_group: &'static str },
}

#[derive(Debug, Clone, Deserialize)]
pub struct Nj1040Constants {
	pub status: StatusCodes,
	pub tax_table: TaxTable,
	pub tax_formula: TaxFormula,
	pub filing_threshold: FilingThreshold,
	pub exemptions: Exemptions,
	pub property_tax: PropertyTax,
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
	pub quantize_income_below: f64,
	pub quantize_step: f64,
	pub quantize_midpoint_factor: f64,
	pub quantize_floor_epsilon: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaxFormula {
	pub single_or_mfs: TaxFormulaSet,
	pub mfj_hoh_widow: TaxFormulaSet,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaxFormulaSet {
	pub brackets: Vec<TaxFormulaBracket>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaxFormulaBracket {
	pub max_income: f64,
	pub rate: f64,
	pub offset: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FilingThreshold {
	pub single_or_mfs: f64,
	pub mfj_or_hoh_or_widow: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Exemptions {
	pub base_personal_count_multiplier: f64,
	pub over65_multiplier: f64,
	pub blind_or_disabled_multiplier: f64,
	pub veteran_multiplier: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PropertyTax {
	pub deduction_cap_non_mfs: f64,
	pub deduction_cap_mfs: f64,
	pub credit_non_mfs: f64,
	pub credit_mfs: f64,
	#[serde(default)]
	pub coj_ratio_cap: f64,
}

pub fn load_constants_from_file<P: AsRef<Path>>(path: P) -> Result<Nj1040Constants, FormHooksError> {
	let path_ref = path.as_ref();
	let raw = fs::read_to_string(path_ref).map_err(|source| FormHooksError::Io {
		path: path_ref.display().to_string(),
		source,
	})?;
	let constants: Nj1040Constants = toml::from_str(&raw)?;
	validate_constants(&constants)?;
	Ok(constants)
}

pub fn register_hooks(registry: &mut HookRegistry, constants: Nj1040Constants) {
	let constants = Arc::new(constants);

	{
		let constants = Arc::clone(&constants);
		registry.register_hook("nj1040::compute_L6", move |call| {
			let count = if is_joint_filing(call, &constants.status) { 2.0 } else { 1.0 };
			Ok(count * constants.exemptions.base_personal_count_multiplier)
		});
	}

	{
		let constants = Arc::clone(&constants);
		registry.register_hook("nj1040::compute_L7", move |call| {
			let you = bool_to_count(call, "YouOver65");
			let spouse = if is_joint_filing(call, &constants.status) {
				bool_to_count(call, "SpouseOver65")
			} else {
				0.0
			};
			Ok((you + spouse) * constants.exemptions.over65_multiplier)
		});
	}

	{
		let constants = Arc::clone(&constants);
		registry.register_hook("nj1040::compute_L8", move |call| {
			let you = bool_to_count(call, "YouBlindDisa");
			let spouse = if is_joint_filing(call, &constants.status) {
				bool_to_count(call, "SpouseBlindDisa")
			} else {
				0.0
			};
			Ok((you + spouse) * constants.exemptions.blind_or_disabled_multiplier)
		});
	}

	{
		let constants = Arc::clone(&constants);
		registry.register_hook("nj1040::compute_L9", move |call| {
			let you = bool_to_count(call, "YouVeteran");
			let spouse = if is_joint_filing(call, &constants.status) {
				bool_to_count(call, "SpouseVeteran")
			} else {
				0.0
			};
			Ok((you + spouse) * constants.exemptions.veteran_multiplier)
		});
	}

	{
		let constants = Arc::clone(&constants);
		registry.register_hook("nj1040::compute_L41", move |call| {
			let status = value_or_zero(call, "status") as i64;
			let property_tax_paid = value_or_zero(call, "L40a").max(0.0);
			let homeowner = bool_to_count(call, "HomeOwner") > 0.0;
			let tenant = bool_to_count(call, "Tenant") > 0.0;
			if !homeowner || tenant || property_tax_paid <= 0.0 {
				return Ok(0.0);
			}

			let deduction_cap = if is_mfs(status, &constants.status) {
				constants.property_tax.deduction_cap_mfs
			} else {
				constants.property_tax.deduction_cap_non_mfs
			};

			Ok(property_tax_paid.min(deduction_cap))
		});
	}

	{
		let constants = Arc::clone(&constants);
		registry.register_hook("nj1040::compute_L43", move |call| {
			let status = value_or_zero(call, "status") as i64;
			let taxable_income = value_or_zero(call, "L42").max(0.0);
			Ok(compute_tax_line(&constants, status, taxable_income))
		});
	}

	{
		let constants = Arc::clone(&constants);
		registry.register_hook("nj1040::compute_L44", move |call| {
			let l43 = value_or_zero(call, "L43").max(0.0);
			let total_income = value_or_zero(call, "L27").max(0.0);
			let coj_income = value_or_zero(call, "COJ1").max(0.0);
			let paid_to_other_jurisdiction = value_or_zero(call, "COJ9a").max(0.0);

			if l43 <= 0.0 || coj_income <= 0.0 || total_income <= 0.0 || paid_to_other_jurisdiction <= 0.0 {
				return Ok(0.0);
			}

			let ratio_cap = if constants.property_tax.coj_ratio_cap > 0.0 {
				constants.property_tax.coj_ratio_cap
			} else {
				1.0
			};

			let ratio = (coj_income / total_income).clamp(0.0, ratio_cap);
			let proportional_limit = l43 * ratio;
			Ok(paid_to_other_jurisdiction
				.min(proportional_limit)
				.min(l43)
				.max(0.0))
		});
	}

	{
		let constants = Arc::clone(&constants);
		registry.register_hook("nj1040::compute_L56", move |call| {
			let status = value_or_zero(call, "status") as i64;
			let has_property_tax = value_or_zero(call, "L40a") > 0.0;
			let homeowner_or_tenant = bool_to_count(call, "HomeOwner") > 0.0 || bool_to_count(call, "Tenant") > 0.0;
			let threshold = filing_threshold_for_status(&constants, status);
			let l29 = value_or_zero(call, "L29").max(0.0);
			let l44 = value_or_zero(call, "L44").max(0.0);

			if !has_property_tax || !homeowner_or_tenant || l29 >= threshold || l44 > 0.0 {
				return Ok(0.0);
			}

			let credit_amount = if is_mfs(status, &constants.status) {
				constants.property_tax.credit_mfs
			} else {
				constants.property_tax.credit_non_mfs
			};

			Ok(credit_amount.max(0.0))
		});
	}

	{
		let constants = Arc::clone(&constants);
		registry.register_hook("nj1040::compute_filing_threshold", move |call| {
			let status = value_or_zero(call, "status") as i64;
			Ok(filing_threshold_for_status(&constants, status))
		});
	}
}

pub fn register_hooks_from_constants_file<P: AsRef<Path>>(
	registry: &mut HookRegistry,
	path: P,
) -> Result<(), FormHooksError> {
	let constants = load_constants_from_file(path)?;
	register_hooks(registry, constants);
	Ok(())
}

fn validate_constants(constants: &Nj1040Constants) -> Result<(), FormHooksError> {
	if constants.tax_formula.single_or_mfs.brackets.is_empty() {
		return Err(FormHooksError::InvalidTaxFormula {
			status_group: "single_or_mfs",
		});
	}

	if constants.tax_formula.mfj_hoh_widow.brackets.is_empty() {
		return Err(FormHooksError::InvalidTaxFormula {
			status_group: "mfj_hoh_widow",
		});
	}

	Ok(())
}

fn compute_tax_line(constants: &Nj1040Constants, status: i64, taxable_income: f64) -> f64 {
	let quantized_income = quantize_income(constants, taxable_income);
	let is_quantized_path = taxable_income < constants.tax_table.quantize_income_below;
	let brackets = if is_single_or_mfs(status, &constants.status) {
		&constants.tax_formula.single_or_mfs.brackets
	} else {
		&constants.tax_formula.mfj_hoh_widow.brackets
	};

	let bracket = brackets
		.iter()
		.find(|entry| quantized_income <= entry.max_income)
		.unwrap_or(&brackets[brackets.len() - 1]);

	let tax = (quantized_income * bracket.rate - bracket.offset).max(0.0);
	if is_quantized_path {
		tax.round()
	} else {
		tax
	}
}

fn quantize_income(constants: &Nj1040Constants, income: f64) -> f64 {
	if income >= constants.tax_table.quantize_income_below {
		return income;
	}

	let step = constants.tax_table.quantize_step;
	let midpoint = step * constants.tax_table.quantize_midpoint_factor;
	let shifted = income + midpoint - constants.tax_table.quantize_floor_epsilon;
	(shifted / step).floor() * step
}

fn filing_threshold_for_status(constants: &Nj1040Constants, status: i64) -> f64 {
	if is_single_or_mfs(status, &constants.status) {
		constants.filing_threshold.single_or_mfs
	} else if is_mfj_hoh_or_widow(status, &constants.status) {
		constants.filing_threshold.mfj_or_hoh_or_widow
	} else {
		constants.filing_threshold.single_or_mfs
	}
}

fn is_joint_filing(call: &HookCall<'_>, status_codes: &StatusCodes) -> bool {
	(value_or_zero(call, "status") as i64) == status_codes.married_filing_jointly
}

fn is_single_or_mfs(status: i64, status_codes: &StatusCodes) -> bool {
	status == status_codes.single || status == status_codes.married_filing_separately
}

fn is_mfj_hoh_or_widow(status: i64, status_codes: &StatusCodes) -> bool {
	status == status_codes.married_filing_jointly
		|| status == status_codes.head_of_household
		|| status == status_codes.widow
}

fn is_mfs(status: i64, status_codes: &StatusCodes) -> bool {
	status == status_codes.married_filing_separately
}

fn bool_to_count(call: &HookCall<'_>, key: &str) -> f64 {
	if value_or_zero(call, key) != 0.0 {
		1.0
	} else {
		0.0
	}
}

fn value_or_zero(call: &HookCall<'_>, key: &str) -> f64 {
	call.context.get_value(key).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use ots_core::dag_exec::HookRegistry;

	use super::*;

	#[test]
	fn loads_constants_from_nj1040_toml() {
		let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/2025/nj1040.toml");
		let constants = load_constants_from_file(path).unwrap();

		assert_eq!(constants.status.married_filing_jointly, 2);
		assert_eq!(constants.exemptions.veteran_multiplier, 6000.0);
		assert!(!constants.tax_formula.single_or_mfs.brackets.is_empty());
	}

	#[test]
	fn registers_all_nj1040_hooks() {
		let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/2025/nj1040.toml");
		let constants = load_constants_from_file(path).unwrap();

		let mut registry = HookRegistry::new();
		register_hooks(&mut registry, constants);

		for hook_name in [
			"nj1040::compute_L6",
			"nj1040::compute_L7",
			"nj1040::compute_L8",
			"nj1040::compute_L9",
			"nj1040::compute_L41",
			"nj1040::compute_L43",
			"nj1040::compute_L44",
			"nj1040::compute_L56",
			"nj1040::compute_filing_threshold",
		] {
			assert!(registry.is_registered(hook_name), "missing hook registration: {hook_name}");
		}
	}
}

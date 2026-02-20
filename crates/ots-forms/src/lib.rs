use std::collections::BTreeMap;
use std::path::Path;

use ots_core::dag_exec::HookRegistry;
use thiserror::Error;

#[path = "2025/us1040.rs"]
mod us1040_2025;
#[path = "2025/nj1040.rs"]
mod nj1040_2025;

#[derive(Debug, Error)]
pub enum FormRegistryError {
	#[error("unsupported form registration target year={year}, form={form}")]
	UnsupportedForm { year: u16, form: String },
	#[error("failed registering hooks for form={form}, year={year}: {source}")]
	HookRegistration {
		year: u16,
		form: &'static str,
		#[source]
		source: Box<dyn std::error::Error + Send + Sync + 'static>,
	},
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredForm {
	pub canonical_name: &'static str,
	pub year: u16,
	pub rules_file: &'static str,
}

type HookRegistrar = fn(&mut HookRegistry, &Path) -> Result<(), FormRegistryError>;

struct FormRegistration {
	canonical_name: &'static str,
	year: u16,
	rules_file: &'static str,
	aliases: &'static [&'static str],
	register_hooks: HookRegistrar,
}

macro_rules! form_registration {
	(
		canonical: $canonical:literal,
		year: $year:literal,
		rules_file: $rules_file:literal,
		aliases: [$($alias:literal),+ $(,)?],
		register_hooks: $register_hooks:path
	) => {{
		fn register_hooks(
			registry: &mut HookRegistry,
			constants_file: &Path,
		) -> Result<(), FormRegistryError> {
			($register_hooks)(registry, constants_file)
				.map_err(|error| FormRegistryError::HookRegistration {
					year: $year,
					form: $canonical,
					source: Box::new(error),
				})
		}

		FormRegistration {
			canonical_name: $canonical,
			year: $year,
			rules_file: $rules_file,
			aliases: &[$($alias),+],
			register_hooks,
		}
	}};
}

const FORM_REGISTRATIONS: &[FormRegistration] = &[
	form_registration!(
		canonical: "US1040",
		year: 2025,
		rules_file: "data/2025/us1040.rules.toml",
		aliases: ["us1040", "1040"],
		register_hooks: us1040_2025::register_hooks_from_constants_file
	),
	form_registration!(
		canonical: "NJ1040",
		year: 2025,
		rules_file: "data/2025/nj1040.rules.toml",
		aliases: ["nj1040", "nj_1040"],
		register_hooks: nj1040_2025::register_hooks_from_constants_file
	),
];

fn find_registration(form_name: &str, year: u16) -> Option<&'static FormRegistration> {
	let normalized = normalize_form_name(form_name);
	FORM_REGISTRATIONS
		.iter()
		.find(|registration| {
			registration.year == year
				&& registration
					.aliases
					.iter()
					.any(|alias| normalize_form_name(alias) == normalized)
		})
}

pub fn resolve_form(form_name: &str, year: u16) -> Option<RegisteredForm> {
	find_registration(form_name, year).map(|registration| RegisteredForm {
		canonical_name: registration.canonical_name,
		year: registration.year,
		rules_file: registration.rules_file,
	})
}

pub fn list_registered_forms() -> Vec<String> {
	FORM_REGISTRATIONS
		.iter()
		.map(|registration| {
			format!(
				"{} ({})",
				registration.canonical_name, registration.year
			)
		})
		.collect()
}

pub fn register_form_hooks_from_constants_file<P: AsRef<Path>>(
	registry: &mut HookRegistry,
	year: u16,
	form: &str,
	constants_file: P,
) -> Result<(), FormRegistryError> {
	let Some(registration) = find_registration(form, year) else {
		return Err(FormRegistryError::UnsupportedForm {
			year,
			form: form.to_string(),
		});
	};

	(registration.register_hooks)(registry, constants_file.as_ref())
}

pub fn enrich_initial_values(form_name: &str, year: u16, input: &str, values: &mut BTreeMap<String, f64>) {
	let Some(registration) = find_registration(form_name, year) else {
		return;
	};

	if registration.canonical_name == "US1040" && registration.year == 2025 {
		us1040_2025::seed_derived_input_values(input, values);
	}
}

fn normalize_form_name(raw: &str) -> String {
	raw.chars()
		.filter(|ch| ch.is_ascii_alphanumeric())
		.map(|ch| ch.to_ascii_lowercase())
		.collect()
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use ots_core::dag_exec::HookRegistry;

	use super::*;

	#[test]
	fn registers_hooks_via_dispatcher() {
		let mut registry = HookRegistry::new();
		let constants_path =
			PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/2025/us1040.toml");

		register_form_hooks_from_constants_file(&mut registry, 2025, "US_1040", constants_path)
			.unwrap();

		assert!(registry.is_registered("us1040::compute_tax_line"));
	}

	#[test]
	fn rejects_unknown_form_or_year() {
		let mut registry = HookRegistry::new();
		let constants_path =
			PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/2025/us1040.toml");

		let error =
			register_form_hooks_from_constants_file(&mut registry, 2024, "US_1040", constants_path)
				.unwrap_err();

		assert!(matches!(error, FormRegistryError::UnsupportedForm { .. }));
	}

	#[test]
	fn resolves_registered_form() {
		let registration = resolve_form("US_1040", 2025).unwrap();
		assert_eq!(registration.canonical_name, "US1040");
		assert_eq!(registration.rules_file, "data/2025/us1040.rules.toml");

		let nj_registration = resolve_form("NJ_1040", 2025).unwrap();
		assert_eq!(nj_registration.canonical_name, "NJ1040");
		assert_eq!(nj_registration.rules_file, "data/2025/nj1040.rules.toml");
	}
}

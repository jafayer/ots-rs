use ots_core::FieldValue;

pub struct ImportedValue {
    pub key: String,
    pub value: FieldValue,
}

pub fn import_prior_return(_path: &str) -> Vec<ImportedValue> {
    Vec::new()
}

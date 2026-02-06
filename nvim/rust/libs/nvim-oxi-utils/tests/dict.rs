use nvim_oxi::Dictionary;
use nvim_oxi_utils::{Error, dict};

#[test]
fn require_i64_missing_key() {
    let dict = Dictionary::new();
    let err = dict::require_i64(&dict, "missing");
    assert!(matches!(err, Err(Error::MissingKey { .. })));
}

#[test]
fn require_i64_invalid_value() {
    let mut dict = Dictionary::new();
    dict.insert("value", "not-a-number");
    let err = dict::require_i64(&dict, "value");
    assert!(matches!(err, Err(Error::InvalidValue { .. })));
}

#[test]
fn require_string_ok() {
    let mut dict = Dictionary::new();
    dict.insert("name", "snacks");
    let value = dict::require_string(&dict, "name");
    assert!(matches!(value, Ok(ref val) if val == "snacks"));
}

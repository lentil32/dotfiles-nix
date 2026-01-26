use nvim_oxi::Dictionary;
use nvim_oxi_utils::{Error, dict};

#[test]
fn require_i64_missing_key() {
    let dict = Dictionary::new();
    let err = dict::require_i64(&dict, "missing").unwrap_err();
    assert!(matches!(err, Error::MissingKey { .. }));
}

#[test]
fn require_i64_invalid_value() {
    let mut dict = Dictionary::new();
    dict.insert("value", "not-a-number");
    let err = dict::require_i64(&dict, "value").unwrap_err();
    assert!(matches!(err, Error::InvalidValue { .. }));
}

#[test]
fn require_string_ok() {
    let mut dict = Dictionary::new();
    dict.insert("name", "snacks");
    let value = dict::require_string(&dict, "name").expect("string required");
    assert_eq!(value, "snacks");
}

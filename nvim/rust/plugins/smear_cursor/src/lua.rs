use crate::types::EPSILON;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Dictionary, Object, Result, String as NvimString};

fn error(message: String) -> nvim_oxi::Error {
    nvim_oxi::api::Error::Other(message).into()
}

fn missing_key(key: &str) -> nvim_oxi::Error {
    error(format!("missing key '{key}'"))
}

pub(crate) fn invalid_key(key: &str, expected: &str) -> nvim_oxi::Error {
    error(format!("invalid key '{key}', expected {expected}"))
}

pub(crate) fn get_object(args: &Dictionary, key: &str) -> Result<Object> {
    args.get(&NvimString::from(key))
        .cloned()
        .ok_or_else(|| missing_key(key))
}

pub(crate) fn f64_from_object(key: &str, value: Object) -> Result<f64> {
    if let Ok(parsed) = f64::from_object(value.clone()) {
        return Ok(parsed);
    }
    if let Ok(parsed) = i64::from_object(value) {
        return Ok(parsed as f64);
    }
    Err(invalid_key(key, "number"))
}

pub(crate) fn i64_from_object(key: &str, value: Object) -> Result<i64> {
    if let Ok(parsed) = i64::from_object(value.clone()) {
        return Ok(parsed);
    }
    if let Ok(parsed) = f64::from_object(value) {
        if parsed.is_finite() {
            let rounded = parsed.round();
            if (rounded - parsed).abs() <= EPSILON {
                return Ok(rounded as i64);
            }
        }
    }
    Err(invalid_key(key, "integer"))
}

pub(crate) fn bool_from_object(key: &str, value: Object) -> Result<bool> {
    bool::from_object(value).map_err(|_| invalid_key(key, "boolean"))
}

pub(crate) fn string_from_object(key: &str, value: Object) -> Result<String> {
    if let Ok(parsed) = String::from_object(value.clone()) {
        return Ok(parsed);
    }
    let parsed = NvimString::from_object(value).map_err(|_| invalid_key(key, "string"))?;
    Ok(parsed.to_string_lossy().into_owned())
}

pub(crate) fn get_f64(args: &Dictionary, key: &str) -> Result<f64> {
    f64_from_object(key, get_object(args, key)?)
}

pub(crate) fn get_optional_f64(args: &Dictionary, key: &str) -> Result<Option<f64>> {
    let Some(value) = args.get(&NvimString::from(key)).cloned() else {
        return Ok(None);
    };
    if value.is_nil() {
        return Ok(None);
    }
    Ok(Some(f64_from_object(key, value)?))
}

pub(crate) fn get_i64(args: &Dictionary, key: &str) -> Result<i64> {
    i64_from_object(key, get_object(args, key)?)
}

pub(crate) fn get_bool(args: &Dictionary, key: &str) -> Result<bool> {
    bool_from_object(key, get_object(args, key)?)
}

pub(crate) fn get_string(args: &Dictionary, key: &str) -> Result<String> {
    string_from_object(key, get_object(args, key)?)
}

pub(crate) fn parse_indexed_objects(
    key: &str,
    value: Object,
    expected_len: Option<usize>,
) -> Result<Vec<Object>> {
    if let Ok(values) = Vec::<Object>::from_object(value.clone()) {
        if let Some(length) = expected_len {
            if values.len() != length {
                return Err(invalid_key(key, "array"));
            }
        }
        return Ok(values);
    }

    let dict = Dictionary::from_object(value).map_err(|_| invalid_key(key, "array"))?;

    let length = expected_len.unwrap_or(dict.len());
    let mut values = Vec::with_capacity(length);
    for index in 1..=length {
        let index_key = NvimString::from(index.to_string());
        let indexed = dict
            .get(&index_key)
            .cloned()
            .ok_or_else(|| invalid_key(key, "array"))?;
        values.push(indexed);
    }

    Ok(values)
}

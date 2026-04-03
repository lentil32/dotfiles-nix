use crate::types::EPSILON;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::ObjectKind;
use nvim_oxi::Result;
use nvim_oxi::String as NvimString;
use nvim_oxi::conversion::FromObject;
use nvimrs_nvim_oxi_utils::Error as OxiError;

pub(crate) type LuaParseError = OxiError;
pub(crate) type LuaParseResult<T> = std::result::Result<T, LuaParseError>;

pub(crate) fn to_nvim_error(err: &LuaParseError) -> nvim_oxi::Error {
    crate::other_error(err.to_string())
}

fn into_nvim_error(err: LuaParseError) -> nvim_oxi::Error {
    to_nvim_error(&err)
}

pub(crate) fn invalid_key_error(key: &str, expected: &'static str) -> LuaParseError {
    OxiError::invalid_value(key, expected)
}

pub(crate) fn missing_key_error(key: &str) -> LuaParseError {
    OxiError::missing_key(key)
}

pub(crate) fn invalid_key(key: &str, expected: &'static str) -> nvim_oxi::Error {
    into_nvim_error(invalid_key_error(key, expected))
}

pub(crate) fn require_object_typed(value: Option<Object>, key: &str) -> LuaParseResult<Object> {
    value.ok_or_else(|| missing_key_error(key))
}

pub(crate) fn require_with_typed<T, F>(
    value: Option<Object>,
    key: &str,
    parse: F,
) -> LuaParseResult<T>
where
    F: Fn(&str, Object) -> LuaParseResult<T>,
{
    parse(key, require_object_typed(value, key)?)
}

fn parse_typed_object<T>(
    key: &str,
    value: Object,
    parse: impl FnOnce(&str, Object) -> LuaParseResult<T>,
) -> Result<T> {
    parse(key, value).map_err(into_nvim_error)
}

pub(crate) fn f64_from_object_typed(key: &str, value: Object) -> LuaParseResult<f64> {
    match value.kind() {
        ObjectKind::Float => f64::from_object(value).map_err(|_| invalid_key_error(key, "number")),
        ObjectKind::Integer | ObjectKind::Buffer | ObjectKind::Window | ObjectKind::TabPage => {
            i64::from_object(value)
                .map(|parsed| parsed as f64)
                .map_err(|_| invalid_key_error(key, "number"))
        }
        _ => Err(invalid_key_error(key, "number")),
    }
}

pub(crate) fn f64_from_object(key: &str, value: Object) -> Result<f64> {
    parse_typed_object(key, value, f64_from_object_typed)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Most object parse call sites own their Object already; the borrowed fast path lives in i64_from_object_ref_with"
)]
pub(crate) fn i64_from_object_typed(key: &str, value: Object) -> LuaParseResult<i64> {
    i64_from_object_ref_with_typed(&value, || key.to_owned())
}

pub(crate) fn i64_from_object(key: &str, value: Object) -> Result<i64> {
    parse_typed_object(key, value, i64_from_object_typed)
}

pub(crate) fn i64_from_object_ref_with_typed<K>(value: &Object, key: K) -> LuaParseResult<i64>
where
    K: Fn() -> String,
{
    let invalid_integer = || {
        let key = key();
        invalid_key_error(&key, "integer")
    };

    match value.kind() {
        ObjectKind::Integer | ObjectKind::Buffer | ObjectKind::Window | ObjectKind::TabPage => {
            Ok(unsafe { value.as_integer_unchecked() })
        }
        ObjectKind::Float => {
            let parsed = unsafe { value.as_float_unchecked() };
            if parsed.is_finite() {
                let rounded = parsed.round();
                if (rounded - parsed).abs() <= EPSILON {
                    return Ok(rounded as i64);
                }
            }
            Err(invalid_integer())
        }
        _ => Err(invalid_integer()),
    }
}

pub(crate) fn bool_from_object_typed(key: &str, value: Object) -> LuaParseResult<bool> {
    bool::from_object(value).map_err(|_| invalid_key_error(key, "boolean"))
}

pub(crate) fn bool_from_object(key: &str, value: Object) -> Result<bool> {
    parse_typed_object(key, value, bool_from_object_typed)
}

pub(crate) fn u8_from_object_typed(key: &str, value: Object) -> LuaParseResult<u8> {
    let parsed = i64_from_object_typed(key, value)?;
    u8::try_from(parsed).map_err(|_| invalid_key_error(key, "u8"))
}

pub(crate) fn string_from_object_typed(key: &str, value: Object) -> LuaParseResult<String> {
    if value.kind() != ObjectKind::String {
        return Err(invalid_key_error(key, "string"));
    }
    let parsed = NvimString::from_object(value).map_err(|_| invalid_key_error(key, "string"))?;
    Ok(parsed.to_string_lossy().into_owned())
}

pub(crate) fn string_from_object(key: &str, value: Object) -> Result<String> {
    parse_typed_object(key, value, string_from_object_typed)
}

pub(crate) fn parse_optional_with<T, F>(
    value: Option<Object>,
    key: &str,
    parse: F,
) -> Result<Option<T>>
where
    F: Fn(&str, Object) -> Result<T>,
{
    value.map(|value| parse(key, value)).transpose()
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParsedOptionalChange<T> {
    Set(T),
    Clear,
}

pub(crate) fn parse_optional_change_with<T, F>(
    value: Option<Object>,
    key: &str,
    parse: F,
) -> Result<Option<ParsedOptionalChange<T>>>
where
    F: Fn(&str, Object) -> Result<T>,
{
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_nil() {
        return Ok(Some(ParsedOptionalChange::Clear));
    }
    parse(key, value).map(|parsed| Some(ParsedOptionalChange::Set(parsed)))
}

pub(crate) fn parse_indexed_objects(
    key: &str,
    value: Object,
    expected_len: Option<usize>,
) -> Result<Vec<Object>> {
    parse_indexed_objects_typed(key, value, expected_len).map_err(into_nvim_error)
}

pub(crate) fn parse_indexed_objects_typed(
    key: &str,
    value: Object,
    expected_len: Option<usize>,
) -> LuaParseResult<Vec<Object>> {
    match value.kind() {
        ObjectKind::Array => {
            let values =
                Vec::<Object>::from_object(value).map_err(|_| invalid_key_error(key, "array"))?;
            if let Some(length) = expected_len
                && values.len() != length
            {
                return Err(invalid_key_error(key, "array"));
            }
            Ok(values)
        }
        ObjectKind::Dictionary => {
            let dict =
                Dictionary::from_object(value).map_err(|_| invalid_key_error(key, "array"))?;
            let length = expected_len.unwrap_or(dict.len());
            let mut values = Vec::with_capacity(length);
            for index in 1..=length {
                let index_key = NvimString::from(index.to_string());
                let indexed = dict
                    .get(&index_key)
                    .cloned()
                    .ok_or_else(|| invalid_key_error(key, "array"))?;
                values.push(indexed);
            }
            Ok(values)
        }
        _ => Err(invalid_key_error(key, "array")),
    }
}

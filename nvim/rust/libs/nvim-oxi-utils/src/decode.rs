use crate::{Error, Result};
use nvim_oxi::conversion::FromObject;
use nvim_oxi::serde::Deserializer;
use nvim_oxi::{Dictionary, Object, String as NvimString};
use serde::Deserialize;

pub fn deserialize<T>(dict: &Dictionary) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    T::deserialize(Deserializer::new(Object::from(dict.clone())))
        .map_err(|err| Error::unexpected(err.to_string()))
}

pub fn get_object(dict: &Dictionary, key: &str) -> Option<Object> {
    let key = NvimString::from(key);
    dict.get(&key).cloned()
}

pub fn require_object(dict: &Dictionary, key: &str) -> Result<Object> {
    get_object(dict, key).ok_or_else(|| Error::missing_key(key))
}

pub fn require_from_object<T>(
    maybe_value: Option<Object>,
    key: &str,
    expected: &'static str,
) -> Result<T>
where
    T: FromObject,
{
    let value = maybe_value.ok_or_else(|| Error::missing_key(key))?;
    T::from_object(value).map_err(|_| Error::invalid_value(key, expected))
}

pub fn parse_from_object<T>(value: Object, key: &str, expected: &'static str) -> Result<T>
where
    T: FromObject,
{
    T::from_object(value).map_err(|_| Error::invalid_value(key, expected))
}

pub fn optional_from_object<T>(
    maybe_value: Option<Object>,
    key: &str,
    expected: &'static str,
) -> Result<Option<T>>
where
    T: FromObject,
{
    maybe_value
        .map(|value| parse_from_object(value, key, expected))
        .transpose()
}

pub fn require_i64(maybe_value: Option<Object>, key: &str) -> Result<i64> {
    require_from_object(maybe_value, key, "i64")
}

pub fn require_string(maybe_value: Option<Object>, key: &str) -> Result<String> {
    let value: NvimString = require_from_object(maybe_value, key, "string")?;
    Ok(value.to_string_lossy().into_owned())
}

pub fn require_nonempty_string(maybe_value: Option<Object>, key: &str) -> Result<String> {
    let value = require_string(maybe_value, key)?;
    if value.is_empty() {
        return Err(Error::empty_value(key));
    }
    Ok(value)
}

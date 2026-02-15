use nvim_oxi::serde::Deserializer;
use nvim_oxi::{Dictionary, Object};
use nvim_oxi_utils::{Error as DecodeError, decode};
use serde::Deserialize;
use support::NonEmptyString;

#[derive(Debug)]
pub struct OilMoveAction {
    pub src_url: NonEmptyString,
    pub dest_url: NonEmptyString,
}

#[derive(Debug)]
pub enum OilAction {
    Move(OilMoveAction),
    Other,
}

#[derive(Debug)]
pub struct OilActionsPostArgs {
    pub action: OilAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OilActionsPostParseError {
    InvalidPayloadType {
        expected: &'static str,
    },
    MissingKey {
        key: String,
    },
    InvalidValue {
        key: String,
        expected: &'static str,
    },
    EmptyValue {
        key: String,
    },
    EmptyActions,
}

impl std::fmt::Display for OilActionsPostParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPayloadType { expected } => {
                write!(f, "invalid payload type, expected {expected}")
            }
            Self::MissingKey { key } => write!(f, "missing key '{key}'"),
            Self::InvalidValue { key, expected } => {
                write!(f, "invalid value for '{key}', expected {expected}")
            }
            Self::EmptyValue { key } => write!(f, "empty value for '{key}'"),
            Self::EmptyActions => write!(f, "oil actions list is empty"),
        }
    }
}

impl OilActionsPostArgs {
    pub fn parse(data: Object) -> Result<Self, OilActionsPostParseError> {
        let dict = Dictionary::try_from(data).map_err(|_| {
            OilActionsPostParseError::InvalidPayloadType {
                expected: "dictionary",
            }
        })?;
        let actions_obj = decode::require_object(&dict, "actions").map_err(map_decode_error)?;
        let actions =
            Vec::<RawOilAction>::deserialize(Deserializer::new(actions_obj)).map_err(|_| {
                OilActionsPostParseError::InvalidValue {
                    key: "actions".to_string(),
                    expected: "array of dictionaries",
                }
            })?;
        let first = actions
            .into_iter()
            .next()
            .ok_or(OilActionsPostParseError::EmptyActions)?;
        let action = OilAction::parse(first)?;
        Ok(Self { action })
    }
}

impl OilAction {
    fn parse(action: RawOilAction) -> Result<Self, OilActionsPostParseError> {
        let action_type = require_nonempty_field(action.action_type, "type")?;
        if action_type.as_str() != "move" {
            return Ok(Self::Other);
        }
        let src_url = require_nonempty_field(action.src_url, "src_url")?;
        let dest_url = require_nonempty_field(action.dest_url, "dest_url")?;
        Ok(Self::Move(OilMoveAction { src_url, dest_url }))
    }
}

fn require_nonempty_field(
    value: Option<Object>,
    key: &str,
) -> Result<NonEmptyString, OilActionsPostParseError> {
    let Some(value) = value else {
        return Err(OilActionsPostParseError::EmptyValue {
            key: key.to_string(),
        });
    };
    let value = decode::parse_from_object(value, key, "string").map_err(map_decode_error)?;
    NonEmptyString::try_new(value).map_err(|_| OilActionsPostParseError::EmptyValue {
        key: key.to_string(),
    })
}

fn map_decode_error(value: DecodeError) -> OilActionsPostParseError {
    match value {
        DecodeError::MissingKey { key } => OilActionsPostParseError::MissingKey { key },
        DecodeError::InvalidValue { key, expected } => {
            OilActionsPostParseError::InvalidValue { key, expected }
        }
        DecodeError::EmptyValue { key } => OilActionsPostParseError::EmptyValue { key },
        DecodeError::Unexpected { .. } | DecodeError::Nvim(_) => {
            OilActionsPostParseError::InvalidPayloadType {
                expected: "dictionary",
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawOilAction {
    #[serde(rename = "type", default)]
    action_type: Option<Object>,
    #[serde(default)]
    src_url: Option<Object>,
    #[serde(default)]
    dest_url: Option<Object>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn move_action_dict(src: &str, dest: &str) -> Dictionary {
        let mut action = Dictionary::new();
        action.insert("type", "move");
        action.insert("src_url", src);
        action.insert("dest_url", dest);
        action
    }

    fn actions_post_payload(actions: Vec<Dictionary>) -> Object {
        let mut payload = Dictionary::new();
        payload.insert("actions", nvim_oxi::Array::from_iter(actions));
        Object::from(payload)
    }

    #[test]
    fn oil_actions_post_parse_move_action() -> Result<(), &'static str> {
        let payload = actions_post_payload(vec![move_action_dict("a", "b")]);
        let parsed = OilActionsPostArgs::parse(payload).map_err(|_| "parse failed")?;
        match parsed.action {
            OilAction::Move(action) => {
                assert_eq!(action.src_url.as_str(), "a");
                assert_eq!(action.dest_url.as_str(), "b");
            }
            OilAction::Other => return Err("expected move action"),
        }
        Ok(())
    }

    #[test]
    fn oil_actions_post_parse_non_move_action() -> Result<(), &'static str> {
        let mut action = Dictionary::new();
        action.insert("type", "delete");
        let payload = actions_post_payload(vec![action]);
        let parsed = OilActionsPostArgs::parse(payload).map_err(|_| "parse failed")?;
        assert!(matches!(parsed.action, OilAction::Other));
        Ok(())
    }

    #[test]
    fn oil_actions_post_parse_rejects_non_dict_payload() {
        let parsed = OilActionsPostArgs::parse(Object::from(123_i64));
        assert!(matches!(
            parsed,
            Err(OilActionsPostParseError::InvalidPayloadType { .. })
        ));
    }

    #[test]
    fn oil_actions_post_parse_rejects_missing_actions_key() {
        let payload = Dictionary::new();
        let parsed = OilActionsPostArgs::parse(Object::from(payload));
        assert!(matches!(
            parsed,
            Err(OilActionsPostParseError::MissingKey { key }) if key == "actions"
        ));
    }

    #[test]
    fn oil_actions_post_parse_rejects_empty_actions() {
        let payload = actions_post_payload(Vec::new());
        let parsed = OilActionsPostArgs::parse(payload);
        assert!(matches!(
            parsed,
            Err(OilActionsPostParseError::EmptyActions)
        ));
    }

    #[test]
    fn oil_actions_post_parse_rejects_missing_move_fields() {
        let mut action = Dictionary::new();
        action.insert("type", "move");
        let payload = actions_post_payload(vec![action]);
        let parsed = OilActionsPostArgs::parse(payload);
        assert!(matches!(
            parsed,
            Err(OilActionsPostParseError::EmptyValue { key }) if key == "src_url"
        ));
    }
}

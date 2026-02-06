use std::collections::HashMap;

use nvim_oxi::conversion::FromObject;
use nvim_oxi::{Dictionary, Object, String as NvimString};
use nvim_oxi_utils::dict;
use nvim_oxi_utils::handles::{BufHandle, WinHandle};
use support::NonEmptyString;

pub type OilMap = HashMap<WinHandle, BufHandle>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OilLastBufEvent {
    OilBufEntered { win: WinHandle, buf: BufHandle },
    WinClosed { win: WinHandle },
    BufWiped { buf: BufHandle },
}

#[derive(Debug, Default, Clone)]
pub struct OilLastBufState {
    by_win: OilMap,
}

impl OilLastBufState {
    pub fn apply(&mut self, event: OilLastBufEvent) {
        match event {
            OilLastBufEvent::OilBufEntered { win, buf } => {
                if self.by_win.get(&win).copied() == Some(buf) {
                    return;
                }
                let _ = self.by_win.insert(win, buf);
            }
            OilLastBufEvent::WinClosed { win } => {
                let _ = self.by_win.remove(&win);
            }
            OilLastBufEvent::BufWiped { buf } => {
                self.by_win.retain(|_, mapped| *mapped != buf);
            }
        }
    }

    #[cfg(test)]
    pub fn reconcile<FW, FB>(&mut self, is_win_valid: FW, is_buf_valid: FB) -> bool
    where
        FW: Fn(WinHandle) -> bool,
        FB: Fn(BufHandle) -> bool,
    {
        let before = self.by_win.len();
        self.by_win
            .retain(|win, buf| is_win_valid(*win) && is_buf_valid(*buf));
        self.by_win.len() != before
    }

    #[cfg(test)]
    pub fn last_buf_for_win<FB>(&mut self, win: WinHandle, is_buf_valid: FB) -> Option<BufHandle>
    where
        FB: Fn(BufHandle) -> bool,
    {
        let buf = self.by_win.get(&win).copied()?;
        if is_buf_valid(buf) {
            return Some(buf);
        }
        let _ = self.by_win.remove(&win);
        None
    }

    pub fn mapped_buf_for_win(&self, win: WinHandle) -> Option<BufHandle> {
        self.by_win.get(&win).copied()
    }

    pub fn clear_mapping_if_matches(&mut self, win: WinHandle, expected: BufHandle) -> bool {
        match self.by_win.get(&win).copied() {
            Some(mapped) if mapped == expected => {
                let _ = self.by_win.remove(&win);
                true
            }
            _ => false,
        }
    }

    #[cfg(test)]
    fn contains(&self, win: WinHandle, buf: BufHandle) -> bool {
        self.by_win.get(&win).copied() == Some(buf)
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OilActionsPostParseError {
    InvalidPayloadType {
        expected: &'static str,
    },
    MissingKey {
        key: &'static str,
    },
    InvalidValue {
        key: &'static str,
        expected: &'static str,
    },
    EmptyValue {
        key: &'static str,
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

fn require_nonempty_field(
    dict: &Dictionary,
    key: &'static str,
) -> Result<NonEmptyString, OilActionsPostParseError> {
    let value =
        dict::get_string_nonempty(dict, key).ok_or(OilActionsPostParseError::EmptyValue { key })?;
    NonEmptyString::try_new(value).map_err(|_| OilActionsPostParseError::EmptyValue { key })
}

impl OilActionsPostArgs {
    pub fn parse(data: Object) -> Result<Self, OilActionsPostParseError> {
        let dict = Dictionary::try_from(data).map_err(|_| {
            OilActionsPostParseError::InvalidPayloadType {
                expected: "dictionary",
            }
        })?;
        let actions_key = NvimString::from("actions");
        let actions_obj = dict
            .get(&actions_key)
            .cloned()
            .ok_or(OilActionsPostParseError::MissingKey { key: "actions" })?;
        let actions = Vec::<Dictionary>::from_object(actions_obj).map_err(|_| {
            OilActionsPostParseError::InvalidValue {
                key: "actions",
                expected: "array of dictionaries",
            }
        })?;
        let first = actions
            .into_iter()
            .next()
            .ok_or(OilActionsPostParseError::EmptyActions)?;
        let action = OilAction::parse(&first)?;
        Ok(Self { action })
    }
}

impl OilAction {
    fn parse(action: &Dictionary) -> Result<Self, OilActionsPostParseError> {
        let action_type = dict::get_string_nonempty(action, "type")
            .ok_or(OilActionsPostParseError::EmptyValue { key: "type" })?;
        if action_type != "move" {
            return Ok(Self::Other);
        }
        let src_url = require_nonempty_field(action, "src_url")?;
        let dest_url = require_nonempty_field(action, "dest_url")?;
        Ok(Self::Move(OilMoveAction { src_url, dest_url }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutocmdAction {
    Keep,
}

impl AutocmdAction {
    pub const fn as_bool(self) -> bool {
        match self {
            Self::Keep => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: i64) -> Result<WinHandle, &'static str> {
        WinHandle::try_from_i64(id).ok_or("expected valid window handle")
    }

    fn buf(id: i64) -> Result<BufHandle, &'static str> {
        BufHandle::try_from_i64(id).ok_or("expected valid buffer handle")
    }

    #[test]
    fn oil_last_buf_tracks_latest_per_window() -> Result<(), &'static str> {
        let mut state = OilLastBufState::default();
        let w1 = win(1)?;
        let w2 = win(2)?;
        let b10 = buf(10)?;
        let b11 = buf(11)?;
        let b20 = buf(20)?;

        state.apply(OilLastBufEvent::OilBufEntered { win: w1, buf: b10 });
        state.apply(OilLastBufEvent::OilBufEntered { win: w2, buf: b20 });
        state.apply(OilLastBufEvent::OilBufEntered { win: w1, buf: b11 });

        assert!(state.contains(w1, b11));
        assert!(state.contains(w2, b20));
        Ok(())
    }

    #[test]
    fn oil_last_buf_win_closed_removes_mapping() -> Result<(), &'static str> {
        let mut state = OilLastBufState::default();
        let w1 = win(1)?;
        let b10 = buf(10)?;
        state.apply(OilLastBufEvent::OilBufEntered { win: w1, buf: b10 });

        state.apply(OilLastBufEvent::WinClosed { win: w1 });

        assert_eq!(state.last_buf_for_win(w1, |_| true), None);
        Ok(())
    }

    #[test]
    fn oil_last_buf_buf_wiped_removes_all_matching_mappings() -> Result<(), &'static str> {
        let mut state = OilLastBufState::default();
        let w1 = win(1)?;
        let w2 = win(2)?;
        let b10 = buf(10)?;
        state.apply(OilLastBufEvent::OilBufEntered { win: w1, buf: b10 });
        state.apply(OilLastBufEvent::OilBufEntered { win: w2, buf: b10 });

        state.apply(OilLastBufEvent::BufWiped { buf: b10 });

        assert_eq!(state.last_buf_for_win(w1, |_| true), None);
        assert_eq!(state.last_buf_for_win(w2, |_| true), None);
        Ok(())
    }

    #[test]
    fn oil_last_buf_reconcile_drops_invalid_entries() -> Result<(), &'static str> {
        let mut state = OilLastBufState::default();
        let w1 = win(1)?;
        let w2 = win(2)?;
        let b10 = buf(10)?;
        let b20 = buf(20)?;
        state.apply(OilLastBufEvent::OilBufEntered { win: w1, buf: b10 });
        state.apply(OilLastBufEvent::OilBufEntered { win: w2, buf: b20 });

        let changed = state.reconcile(|win| win == w1, |buf| buf == b10);

        assert!(changed);
        assert_eq!(state.last_buf_for_win(w1, |_| true), Some(b10));
        assert_eq!(state.last_buf_for_win(w2, |_| true), None);
        Ok(())
    }

    #[test]
    fn oil_last_buf_lookup_removes_stale_buffer_mapping() -> Result<(), &'static str> {
        let mut state = OilLastBufState::default();
        let w1 = win(1)?;
        let b10 = buf(10)?;
        state.apply(OilLastBufEvent::OilBufEntered { win: w1, buf: b10 });

        assert_eq!(state.last_buf_for_win(w1, |_| false), None);
        assert_eq!(state.last_buf_for_win(w1, |_| true), None);
        Ok(())
    }

    #[test]
    fn oil_last_buf_clear_mapping_if_matches_only_removes_same_buf() -> Result<(), &'static str> {
        let mut state = OilLastBufState::default();
        let w1 = win(1)?;
        let b10 = buf(10)?;
        let b11 = buf(11)?;
        state.apply(OilLastBufEvent::OilBufEntered { win: w1, buf: b10 });

        assert!(!state.clear_mapping_if_matches(w1, b11));
        assert_eq!(state.mapped_buf_for_win(w1), Some(b10));

        assert!(state.clear_mapping_if_matches(w1, b10));
        assert_eq!(state.mapped_buf_for_win(w1), None);
        Ok(())
    }

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
            Err(OilActionsPostParseError::MissingKey { key: "actions" })
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
            Err(OilActionsPostParseError::EmptyValue { key: "src_url" })
        ));
    }
}

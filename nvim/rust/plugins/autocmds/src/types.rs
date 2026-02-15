use std::collections::HashMap;

use nvim_oxi::conversion::FromObject;
use nvim_oxi::serde::Deserializer;
use nvim_oxi::{Dictionary, Object, String as NvimString};
use nvim_oxi_utils::handles::{BufHandle, WinHandle};
use serde::Deserialize;
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
        let actions =
            Vec::<RawOilAction>::deserialize(Deserializer::new(actions_obj)).map_err(|_| {
                OilActionsPostParseError::InvalidValue {
                    key: "actions",
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
    key: &'static str,
) -> Result<NonEmptyString, OilActionsPostParseError> {
    let value = value.ok_or(OilActionsPostParseError::EmptyValue { key })?;
    let value = NvimString::from_object(value)
        .map_err(|_| OilActionsPostParseError::EmptyValue { key })?
        .to_string_lossy()
        .into_owned();
    NonEmptyString::try_new(value).map_err(|_| OilActionsPostParseError::EmptyValue { key })
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
    use std::collections::HashMap;

    fn win(id: i64) -> Result<WinHandle, &'static str> {
        WinHandle::try_from_i64(id).ok_or("expected valid window handle")
    }

    fn buf(id: i64) -> Result<BufHandle, &'static str> {
        BufHandle::try_from_i64(id).ok_or("expected valid buffer handle")
    }

    fn assert_oil_state_invariants(state: &OilLastBufState) {
        for (win, buf) in &state.by_win {
            assert!(win.raw() > 0);
            assert!(buf.raw() > 0);
        }
    }

    #[derive(Clone, Copy)]
    enum OilStep {
        EnterW1B10,
        EnterW1B11,
        EnterW2B10,
        EnterW2B20,
        CloseW1,
        CloseW2,
        WipeB10,
        WipeB11,
        WipeB20,
    }

    impl OilStep {
        const ALL: [Self; 9] = [
            Self::EnterW1B10,
            Self::EnterW1B11,
            Self::EnterW2B10,
            Self::EnterW2B20,
            Self::CloseW1,
            Self::CloseW2,
            Self::WipeB10,
            Self::WipeB11,
            Self::WipeB20,
        ];
    }

    #[derive(Clone, Copy)]
    struct Handles {
        w1: WinHandle,
        w2: WinHandle,
        b10: BufHandle,
        b11: BufHandle,
        b20: BufHandle,
    }

    fn apply_model(model: &mut HashMap<WinHandle, BufHandle>, handles: Handles, step: OilStep) {
        match step {
            OilStep::EnterW1B10 => {
                let _ = model.insert(handles.w1, handles.b10);
            }
            OilStep::EnterW1B11 => {
                let _ = model.insert(handles.w1, handles.b11);
            }
            OilStep::EnterW2B10 => {
                let _ = model.insert(handles.w2, handles.b10);
            }
            OilStep::EnterW2B20 => {
                let _ = model.insert(handles.w2, handles.b20);
            }
            OilStep::CloseW1 => {
                let _ = model.remove(&handles.w1);
            }
            OilStep::CloseW2 => {
                let _ = model.remove(&handles.w2);
            }
            OilStep::WipeB10 => model.retain(|_, mapped| *mapped != handles.b10),
            OilStep::WipeB11 => model.retain(|_, mapped| *mapped != handles.b11),
            OilStep::WipeB20 => model.retain(|_, mapped| *mapped != handles.b20),
        }
    }

    fn apply_state(state: &mut OilLastBufState, handles: Handles, step: OilStep) {
        let event = match step {
            OilStep::EnterW1B10 => OilLastBufEvent::OilBufEntered {
                win: handles.w1,
                buf: handles.b10,
            },
            OilStep::EnterW1B11 => OilLastBufEvent::OilBufEntered {
                win: handles.w1,
                buf: handles.b11,
            },
            OilStep::EnterW2B10 => OilLastBufEvent::OilBufEntered {
                win: handles.w2,
                buf: handles.b10,
            },
            OilStep::EnterW2B20 => OilLastBufEvent::OilBufEntered {
                win: handles.w2,
                buf: handles.b20,
            },
            OilStep::CloseW1 => OilLastBufEvent::WinClosed { win: handles.w1 },
            OilStep::CloseW2 => OilLastBufEvent::WinClosed { win: handles.w2 },
            OilStep::WipeB10 => OilLastBufEvent::BufWiped { buf: handles.b10 },
            OilStep::WipeB11 => OilLastBufEvent::BufWiped { buf: handles.b11 },
            OilStep::WipeB20 => OilLastBufEvent::BufWiped { buf: handles.b20 },
        };
        state.apply(event);
    }

    fn assert_matches_model(
        state: &OilLastBufState,
        model: &HashMap<WinHandle, BufHandle>,
        handles: Handles,
    ) {
        assert_eq!(
            state.mapped_buf_for_win(handles.w1),
            model.get(&handles.w1).copied()
        );
        assert_eq!(
            state.mapped_buf_for_win(handles.w2),
            model.get(&handles.w2).copied()
        );
    }

    fn run_oil_sequences(sequence: &mut Vec<OilStep>, remaining: usize, handles: Handles) {
        if remaining == 0 {
            let mut state = OilLastBufState::default();
            let mut model = HashMap::new();
            for step in sequence {
                apply_state(&mut state, handles, *step);
                apply_model(&mut model, handles, *step);
                assert_oil_state_invariants(&state);
                assert_matches_model(&state, &model, handles);
            }
            return;
        }
        for step in OilStep::ALL {
            sequence.push(step);
            run_oil_sequences(sequence, remaining - 1, handles);
            let _ = sequence.pop();
        }
    }

    #[test]
    fn oil_last_buf_reducer_matches_model_for_bounded_sequences() -> Result<(), &'static str> {
        let handles = Handles {
            w1: win(1)?,
            w2: win(2)?,
            b10: buf(10)?,
            b11: buf(11)?,
            b20: buf(20)?,
        };
        run_oil_sequences(&mut Vec::new(), 4, handles);
        Ok(())
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

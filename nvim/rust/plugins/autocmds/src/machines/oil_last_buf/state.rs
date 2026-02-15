use std::collections::HashMap;

use nvim_oxi_utils::handles::{BufHandle, WinHandle};

#[cfg(test)]
use super::events::{OilLastBufEffect, OilLastBufEvent};

pub(super) type OilMap = HashMap<WinHandle, BufHandle>;

#[derive(Debug, Default, Clone)]
pub struct OilLastBufState {
    pub(super) by_win: OilMap,
}

impl OilLastBufState {
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

    #[cfg(test)]
    fn contains(&self, win: WinHandle, buf: BufHandle) -> bool {
        self.by_win.get(&win).copied() == Some(buf)
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
        let _ = state.reduce(event);
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

        let _ = state.reduce(OilLastBufEvent::OilBufEntered { win: w1, buf: b10 });
        let _ = state.reduce(OilLastBufEvent::OilBufEntered { win: w2, buf: b20 });
        let _ = state.reduce(OilLastBufEvent::OilBufEntered { win: w1, buf: b11 });

        assert!(state.contains(w1, b11));
        assert!(state.contains(w2, b20));
        Ok(())
    }

    #[test]
    fn oil_last_buf_win_closed_removes_mapping() -> Result<(), &'static str> {
        let mut state = OilLastBufState::default();
        let w1 = win(1)?;
        let b10 = buf(10)?;
        let _ = state.reduce(OilLastBufEvent::OilBufEntered { win: w1, buf: b10 });

        let _ = state.reduce(OilLastBufEvent::WinClosed { win: w1 });

        assert_eq!(state.last_buf_for_win(w1, |_| true), None);
        Ok(())
    }

    #[test]
    fn oil_last_buf_buf_wiped_removes_all_matching_mappings() -> Result<(), &'static str> {
        let mut state = OilLastBufState::default();
        let w1 = win(1)?;
        let w2 = win(2)?;
        let b10 = buf(10)?;
        let _ = state.reduce(OilLastBufEvent::OilBufEntered { win: w1, buf: b10 });
        let _ = state.reduce(OilLastBufEvent::OilBufEntered { win: w2, buf: b10 });

        let _ = state.reduce(OilLastBufEvent::BufWiped { buf: b10 });

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
        let _ = state.reduce(OilLastBufEvent::OilBufEntered { win: w1, buf: b10 });
        let _ = state.reduce(OilLastBufEvent::OilBufEntered { win: w2, buf: b20 });

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
        let _ = state.reduce(OilLastBufEvent::OilBufEntered { win: w1, buf: b10 });

        assert_eq!(state.last_buf_for_win(w1, |_| false), None);
        assert_eq!(state.last_buf_for_win(w1, |_| true), None);
        Ok(())
    }

    #[test]
    fn oil_last_buf_invalidate_if_mapped_only_removes_same_buf() -> Result<(), &'static str> {
        let mut state = OilLastBufState::default();
        let w1 = win(1)?;
        let b10 = buf(10)?;
        let b11 = buf(11)?;
        let _ = state.reduce(OilLastBufEvent::OilBufEntered { win: w1, buf: b10 });

        let no_change = state.reduce(OilLastBufEvent::InvalidateIfMapped {
            win: w1,
            expected: b11,
        });
        assert!(no_change.is_empty());
        assert_eq!(state.mapped_buf_for_win(w1), Some(b10));

        let changed = state.reduce(OilLastBufEvent::InvalidateIfMapped {
            win: w1,
            expected: b10,
        });
        assert_eq!(
            changed.effects,
            vec![OilLastBufEffect::MappingClearedForWin { win: w1 }]
        );
        assert_eq!(state.mapped_buf_for_win(w1), None);
        Ok(())
    }
}

use super::events::{OilLastBufEffect, OilLastBufEvent, OilLastBufTransition};
use super::state::OilLastBufState;

impl OilLastBufState {
    pub fn reduce(&mut self, event: OilLastBufEvent) -> OilLastBufTransition {
        match event {
            OilLastBufEvent::OilBufEntered { win, buf } => {
                if self.by_win.get(&win).copied() == Some(buf) {
                    return OilLastBufTransition::default();
                }
                let _ = self.by_win.insert(win, buf);
                OilLastBufTransition::with_effect(OilLastBufEffect::MappingUpdated { win, buf })
            }
            OilLastBufEvent::WinClosed { win } => {
                if self.by_win.remove(&win).is_some() {
                    OilLastBufTransition::with_effect(OilLastBufEffect::MappingClearedForWin {
                        win,
                    })
                } else {
                    OilLastBufTransition::default()
                }
            }
            OilLastBufEvent::BufWiped { buf } => {
                let before = self.by_win.len();
                self.by_win.retain(|_, mapped| *mapped != buf);
                if self.by_win.len() != before {
                    OilLastBufTransition::with_effect(OilLastBufEffect::MappingsClearedForBuf {
                        buf,
                    })
                } else {
                    OilLastBufTransition::default()
                }
            }
            OilLastBufEvent::InvalidateIfMapped { win, expected } => {
                if self.by_win.get(&win).copied() == Some(expected) {
                    let _ = self.by_win.remove(&win);
                    OilLastBufTransition::with_effect(OilLastBufEffect::MappingClearedForWin {
                        win,
                    })
                } else {
                    OilLastBufTransition::default()
                }
            }
        }
    }
}

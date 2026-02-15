use nvim_oxi_utils::handles::{BufHandle, WinHandle};
use nvim_oxi_utils::state_machine::{NoCommand, Transition};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OilLastBufEvent {
    OilBufEntered { win: WinHandle, buf: BufHandle },
    WinClosed { win: WinHandle },
    BufWiped { buf: BufHandle },
    InvalidateIfMapped { win: WinHandle, expected: BufHandle },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OilLastBufEffect {
    MappingUpdated { win: WinHandle, buf: BufHandle },
    MappingClearedForWin { win: WinHandle },
    MappingsClearedForBuf { buf: BufHandle },
}

pub(super) type OilLastBufTransition = Transition<OilLastBufEffect, NoCommand>;

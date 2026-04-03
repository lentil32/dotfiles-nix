use nvimrs_nvim_oxi_utils::handles::BufHandle;
use nvimrs_nvim_oxi_utils::handles::WinHandle;
use nvimrs_nvim_oxi_utils::state_machine::NoCommand;
use nvimrs_nvim_oxi_utils::state_machine::Transition;

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

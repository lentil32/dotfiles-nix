use nvim_oxi_utils::state_machine::{NoEffect, Transition};
use support::TabTitle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeztermCompletion {
    Success,
    Failed,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeztermCommand {
    SetTabTitle(TabTitle),
    SetWorkingDir(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeztermEvent {
    RequestTitle {
        title: TabTitle,
    },
    RequestWorkingDir {
        cwd: String,
    },
    TitleCompleted {
        title: TabTitle,
        completion: WeztermCompletion,
    },
    WorkingDirCompleted {
        cwd: String,
        completion: WeztermCompletion,
    },
}

pub(super) type WeztermTransition = Transition<NoEffect, WeztermCommand>;

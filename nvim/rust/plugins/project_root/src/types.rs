use crate::core::{RootIndicators, default_root_indicators};

#[derive(Debug)]
pub struct State {
    pub root_indicators: RootIndicators,
    pub did_setup: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            root_indicators: default_root_indicators(),
            did_setup: false,
        }
    }
}

mod background;
mod base;
mod cursor_color;
mod text_context;

use crate::core::effect::RequestProbeEffect;
use crate::core::event::Event as CoreEvent;
use crate::core::state::ProbeKind;

pub(crate) use base::execute_core_request_observation_base_effect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::events) enum ProbeDispatchWave {
    NewReducerWave,
    SameReducerWave,
}

pub(crate) fn execute_core_request_probe_effect(payload: &RequestProbeEffect) -> Vec<CoreEvent> {
    execute_core_request_probe_effect_with_wave(payload, ProbeDispatchWave::NewReducerWave)
}

pub(crate) fn execute_core_request_probe_effect_same_reducer_wave(
    payload: &RequestProbeEffect,
) -> Vec<CoreEvent> {
    execute_core_request_probe_effect_with_wave(payload, ProbeDispatchWave::SameReducerWave)
}

fn execute_core_request_probe_effect_with_wave(
    payload: &RequestProbeEffect,
    dispatch_wave: ProbeDispatchWave,
) -> Vec<CoreEvent> {
    let event = match payload.kind {
        ProbeKind::CursorColor => cursor_color::collect_cursor_color_report(payload, dispatch_wave),
        ProbeKind::Background => background::collect_background_report(payload),
    };
    vec![event]
}

mod background;
mod base;
mod cursor_color;
mod text_context;

use crate::core::effect::RequestProbeEffect;
use crate::core::event::Event as CoreEvent;
use crate::core::state::ProbeKind;

pub(crate) use base::execute_core_request_observation_base_effect;

pub(crate) fn execute_core_request_probe_effect(payload: &RequestProbeEffect) -> Vec<CoreEvent> {
    execute_core_request_probe_effect_with_reuse(payload, false)
}

pub(crate) fn execute_core_request_probe_effect_same_reducer_wave(
    payload: &RequestProbeEffect,
) -> Vec<CoreEvent> {
    execute_core_request_probe_effect_with_reuse(payload, true)
}

fn execute_core_request_probe_effect_with_reuse(
    payload: &RequestProbeEffect,
    same_reducer_wave: bool,
) -> Vec<CoreEvent> {
    let event = match payload.kind {
        ProbeKind::CursorColor => {
            cursor_color::collect_cursor_color_report(payload, same_reducer_wave)
        }
        ProbeKind::Background => background::collect_background_report(payload),
    };
    vec![event]
}

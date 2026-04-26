use super::super::ReducerState;
use super::super::logging::set_log_level;
use super::super::logging::warn;
use super::IngressReadSnapshot;
use super::RuntimeAccessResult;
use super::cell::restore_reducer_state;
use super::cell::take_reducer_state;
use super::recovery::RuntimeRecoveryPlan;
use super::shell::capture_runtime_shell_recovery_state;
use super::timers::capture_runtime_timer_bridge_recovery_state;
use crate::config::LogLevel;
use crate::core::state::CoreState;
use crate::host::api;
use crate::position::RenderPoint;
use crate::state::RuntimeOptionsPatch;
use crate::state::TrackedCursor;
use nvim_oxi::Dictionary;
use nvim_oxi::Result as NvimResult;
use nvim_oxi::String as NvimString;
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::panic::resume_unwind;

fn with_reducer_state_access<R>(
    accessor: impl FnOnce(&mut ReducerState) -> R,
) -> RuntimeAccessResult<R> {
    let mut state = take_reducer_state()?;
    match catch_unwind(AssertUnwindSafe(|| accessor(&mut state))) {
        Ok(output) => {
            restore_reducer_state(state);
            Ok(output)
        }
        Err(panic_payload) => {
            let recovery_state = capture_runtime_shell_recovery_state();
            let timer_recovery_state = capture_runtime_timer_bridge_recovery_state();
            let recovery_plan =
                RuntimeRecoveryPlan::runtime_lane_panic(recovery_state, timer_recovery_state);
            restore_reducer_state(state);
            recovery_plan.apply();
            resume_unwind(panic_payload);
        }
    }
}

pub(crate) fn with_core_read<R>(reader: impl FnOnce(&CoreState) -> R) -> RuntimeAccessResult<R> {
    with_reducer_state_access(|state| reader(state.core_state()))
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct CoreRuntimeSetup {
    pub(crate) enabled: bool,
    pub(crate) warning: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct CoreRuntimeToggle {
    pub(crate) is_enabled: bool,
    pub(crate) hide_target_hack: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CoreRuntimeSetupMutation {
    setup: CoreRuntimeSetup,
    logging_level: Option<LogLevel>,
}

pub(crate) fn sync_core_runtime_to_current_cursor(
    position: RenderPoint,
    mode: &str,
    tracked_cursor: &TrackedCursor,
) -> RuntimeAccessResult<bool> {
    with_reducer_state_access(|state| {
        let runtime = state.core_state_mut().runtime_mut();
        let cursor_shape =
            crate::state::CursorShape::from_cell_shape(runtime.config.cursor_cell_shape(mode));
        runtime.sync_to_current_cursor(position, cursor_shape, tracked_cursor);
        runtime.config.hide_target_hack
    })
}

pub(crate) fn disable_core_runtime() -> RuntimeAccessResult<()> {
    with_reducer_state_access(|state| {
        state.core_state_mut().runtime_mut().disable();
    })
}

pub(crate) fn apply_core_setup_options(opts: &Dictionary) -> NvimResult<CoreRuntimeSetup> {
    let enabled_option_present = opts.get(&NvimString::from("enabled")).is_some();
    let mutation = with_reducer_state_access(|state| {
        let runtime = state.core_state_mut().runtime_mut();
        if !enabled_option_present {
            runtime.set_enabled(true);
        }

        let patch_result = RuntimeOptionsPatch::parse(opts).and_then(|patch| {
            patch.validate_against(&runtime.config)?;
            runtime.apply_runtime_options_patch(patch);
            Ok(())
        });

        match patch_result {
            Ok(()) => {
                let logging_level = runtime.config.logging_level;
                runtime.clear_runtime_state();
                CoreRuntimeSetupMutation {
                    setup: CoreRuntimeSetup {
                        enabled: runtime.is_enabled(),
                        warning: None,
                    },
                    logging_level: Some(logging_level),
                }
            }
            Err(err) => {
                runtime.disable();
                CoreRuntimeSetupMutation {
                    setup: CoreRuntimeSetup {
                        enabled: false,
                        warning: Some(format!(
                            "setup rejected options; smear cursor remains disabled: {err}"
                        )),
                    },
                    logging_level: None,
                }
            }
        }
    })
    .map_err(nvim_oxi::Error::from)?;

    if let Some(logging_level) = mutation.logging_level {
        set_log_level(logging_level);
    }

    Ok(mutation.setup)
}

pub(crate) fn toggle_core_runtime() -> RuntimeAccessResult<CoreRuntimeToggle> {
    with_reducer_state_access(|state| {
        let runtime = state.core_state_mut().runtime_mut();
        let toggled_enabled = !runtime.is_enabled();
        if toggled_enabled {
            runtime.set_enabled(true);
        } else {
            runtime.disable();
        }

        CoreRuntimeToggle {
            is_enabled: runtime.is_enabled(),
            hide_target_hack: runtime.config.hide_target_hack,
        }
    })
}

pub(crate) fn with_core_transition<R>(
    transition: impl FnOnce(CoreState) -> (CoreState, R),
) -> RuntimeAccessResult<R> {
    with_reducer_state_access(|state| {
        let core_state = state.take_core_state();
        let (next_state, output) = transition(core_state);
        state.set_core_state(next_state);
        output
    })
}

#[cfg(not(test))]
pub(crate) fn ingress_read_snapshot() -> RuntimeAccessResult<IngressReadSnapshot> {
    IngressReadSnapshot::capture()
}

pub(crate) fn ingress_read_snapshot_with_current_buffer(
    current_buffer: Option<&api::Buffer>,
) -> RuntimeAccessResult<IngressReadSnapshot> {
    IngressReadSnapshot::capture_with_current_buffer(current_buffer)
}

#[cfg(test)]
pub(crate) fn core_state() -> RuntimeAccessResult<CoreState> {
    with_core_read(CoreState::clone)
}

#[cfg(test)]
pub(crate) fn set_core_state(next_state: CoreState) -> RuntimeAccessResult<()> {
    with_core_transition(|_| (next_state, ()))
}

pub(crate) fn reset_core_state() {
    if let Err(err) = with_core_transition(|mut state| {
        let runtime = state.take_runtime();
        (CoreState::default().with_runtime(runtime), ())
    }) {
        warn(&format!(
            "reducer state re-entered during core reset; keeping existing state: {err}"
        ));
    }
}

use super::super::handlers::reset_scheduled_effect_queue;
use super::super::logging::set_log_level;
#[cfg(not(test))]
use super::super::logging::warn;
use super::engine::reset_core_state;
use super::shell::ShellRecoveryState;
use super::shell::reset_recovered_runtime_shell_state;
use super::shell::reset_transient_shell_caches;
use super::telemetry::clear_autocmd_event_timestamp;
use super::telemetry::clear_cursor_callback_duration_estimate;
use super::telemetry::clear_observation_request_timestamp;
use super::timer_bridge::TimerBridgeRecoveryState;
use super::timers::clear_recovered_runtime_timer_bridge;
use super::timers::reset_core_timer_bridge;
use super::timers::stop_recovered_core_timer_handles;
use crate::config::RuntimeConfig;
use crate::draw::next_palette_recovery_epoch;
use crate::draw::recover_all_namespaces;
use crate::draw::recover_palette_to_epoch;
#[cfg(test)]
use std::sync::Mutex;
#[cfg(test)]
use std::thread::ThreadId;

fn warn_runtime_recovery(message: &str) {
    #[cfg(not(test))]
    warn(message);

    #[cfg(test)]
    let _ = message;
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum RuntimeRecoveryAction {
    RestoreDefaultLogLevel,
    EmitPanicRecoveryWarning,
    RecoverDrawResources,
    StopRecoveredCoreTimerHandles,
    ResetTimerBridge,
    ClearRecoveredTimerBridge,
    ResetDispatchQueue,
    ResetShellCaches,
    ResetRecoveredShellState,
    ClearTelemetryTimestamps,
    RecoverPaletteEpoch,
    ResetCoreState,
}

const TRANSIENT_RESET_ACTIONS: &[RuntimeRecoveryAction] = &[
    RuntimeRecoveryAction::ResetTimerBridge,
    RuntimeRecoveryAction::ResetDispatchQueue,
    RuntimeRecoveryAction::ResetShellCaches,
    RuntimeRecoveryAction::ClearTelemetryTimestamps,
    RuntimeRecoveryAction::ResetCoreState,
];

// Runtime-lane panic recovery is intentionally host cleanup first, queue/timer
// cleanup second, and reducer reset last. That keeps stale host callbacks from
// observing partially reset reducer state and makes repeated application converge
// to the same runtime snapshot.
const RUNTIME_LANE_PANIC_ACTIONS: &[RuntimeRecoveryAction] = &[
    RuntimeRecoveryAction::RestoreDefaultLogLevel,
    RuntimeRecoveryAction::EmitPanicRecoveryWarning,
    RuntimeRecoveryAction::RecoverDrawResources,
    RuntimeRecoveryAction::StopRecoveredCoreTimerHandles,
    RuntimeRecoveryAction::ClearRecoveredTimerBridge,
    RuntimeRecoveryAction::ResetDispatchQueue,
    RuntimeRecoveryAction::ResetRecoveredShellState,
    RuntimeRecoveryAction::ClearTelemetryTimestamps,
    RuntimeRecoveryAction::RecoverPaletteEpoch,
    RuntimeRecoveryAction::ResetCoreState,
];

#[cfg(test)]
static RECOVERY_ACTION_LOG: Mutex<Option<(ThreadId, Vec<RuntimeRecoveryAction>)>> =
    Mutex::new(None);

#[cfg(test)]
type RecoveryActionLogGuard =
    std::sync::MutexGuard<'static, Option<(ThreadId, Vec<RuntimeRecoveryAction>)>>;

#[cfg(test)]
fn recovery_action_log() -> RecoveryActionLogGuard {
    RECOVERY_ACTION_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
pub(super) fn start_runtime_recovery_action_log_for_test() {
    let mut log = recovery_action_log();
    *log = Some((std::thread::current().id(), Vec::new()));
}

#[cfg(test)]
pub(super) fn take_runtime_recovery_action_log_for_test() -> Vec<RuntimeRecoveryAction> {
    let mut log = recovery_action_log();
    let Some((owner, actions)) = log.take() else {
        return Vec::new();
    };
    if owner == std::thread::current().id() {
        actions
    } else {
        *log = Some((owner, actions));
        Vec::new()
    }
}

#[cfg(test)]
fn record_recovery_action(action: RuntimeRecoveryAction) {
    let mut log = recovery_action_log();
    let Some((owner, actions)) = log.as_mut() else {
        return;
    };
    if *owner == std::thread::current().id() {
        actions.push(action);
    }
}

#[cfg(not(test))]
fn record_recovery_action(_action: RuntimeRecoveryAction) {}

#[derive(Debug, Clone)]
pub(super) struct RuntimeRecoveryPlan {
    actions: &'static [RuntimeRecoveryAction],
    shell_recovery_state: ShellRecoveryState,
    timer_recovery_state: TimerBridgeRecoveryState,
    palette_recovery_epoch: Option<u64>,
}

impl RuntimeRecoveryPlan {
    pub(super) fn transient_reset() -> Self {
        Self {
            actions: TRANSIENT_RESET_ACTIONS,
            shell_recovery_state: ShellRecoveryState::default(),
            timer_recovery_state: TimerBridgeRecoveryState::default(),
            palette_recovery_epoch: None,
        }
    }

    pub(super) fn runtime_lane_panic(
        shell_recovery_state: ShellRecoveryState,
        timer_recovery_state: TimerBridgeRecoveryState,
    ) -> Self {
        Self {
            actions: RUNTIME_LANE_PANIC_ACTIONS,
            shell_recovery_state,
            timer_recovery_state,
            palette_recovery_epoch: next_palette_recovery_epoch(),
        }
    }

    #[cfg(test)]
    pub(super) const fn actions(&self) -> &'static [RuntimeRecoveryAction] {
        self.actions
    }

    pub(super) fn apply(&self) {
        for action in self.actions {
            self.apply_action(*action);
        }
    }

    fn apply_action(&self, action: RuntimeRecoveryAction) {
        record_recovery_action(action);
        match action {
            RuntimeRecoveryAction::RestoreDefaultLogLevel => {
                set_log_level(RuntimeConfig::default().logging_level);
            }
            RuntimeRecoveryAction::EmitPanicRecoveryWarning => {
                warn_runtime_recovery(
                    "runtime lane panicked while borrowed; resetting runtime state",
                );
            }
            RuntimeRecoveryAction::RecoverDrawResources => {
                if let Some(namespace_id) = self.shell_recovery_state.namespace_id {
                    recover_all_namespaces(namespace_id);
                }
            }
            RuntimeRecoveryAction::StopRecoveredCoreTimerHandles => {
                stop_recovered_core_timer_handles(
                    self.timer_recovery_state.core_timer_handles.clone(),
                );
            }
            RuntimeRecoveryAction::ResetTimerBridge => {
                reset_core_timer_bridge();
            }
            RuntimeRecoveryAction::ClearRecoveredTimerBridge => {
                clear_recovered_runtime_timer_bridge();
            }
            RuntimeRecoveryAction::ResetDispatchQueue => {
                reset_scheduled_effect_queue();
            }
            RuntimeRecoveryAction::ResetShellCaches => {
                if let Err(err) = reset_transient_shell_caches() {
                    warn_runtime_recovery(&format!(
                        "shell state re-entered during transient reset; skipping shell cache reset: {err}"
                    ));
                }
            }
            RuntimeRecoveryAction::ResetRecoveredShellState => {
                if let Err(err) = reset_recovered_runtime_shell_state(self.shell_recovery_state) {
                    warn_runtime_recovery(&format!(
                        "shell state re-entered during runtime recovery; skipping shell state reset: {err}"
                    ));
                }
            }
            RuntimeRecoveryAction::ClearTelemetryTimestamps => {
                clear_autocmd_event_timestamp();
                clear_observation_request_timestamp();
                clear_cursor_callback_duration_estimate();
            }
            RuntimeRecoveryAction::RecoverPaletteEpoch => {
                if let Some(epoch) = self.palette_recovery_epoch
                    && !recover_palette_to_epoch(epoch)
                {
                    warn_runtime_recovery(
                        "palette state re-entered during runtime recovery; skipping palette reset",
                    );
                }
            }
            RuntimeRecoveryAction::ResetCoreState => {
                reset_core_state();
            }
        }
    }
}

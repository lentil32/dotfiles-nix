use super::super::ReducerState;
use super::super::RuntimeAccessError;
use super::super::ShellState;
use super::super::event_loop::EventLoopState;
use super::super::logging::LogFileWriter;
use super::diagnostics_lane::DiagnosticsLane;
use super::dispatch_queue::ScheduledEffectQueueState;
use super::host_capabilities::FlushRedrawCapability;
use super::host_capabilities::HostCapabilitiesLane;
use super::timer_bridge::TimerBridge;
use crate::config::LogLevel;
use crate::draw::DrawResourcesLane;
use crate::draw::PaletteStateLane;
use crate::draw::PrepaintOverlay;
use crate::draw::TabWindows;
use crate::host::TabHandle;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Debug)]
struct RuntimeCell {
    reducer: ReducerStateLane,
    shell: ShellStateLane,
    timer_bridge: TimerBridgeLane,
    dispatch_queue: DispatchQueueLane,
    host_capabilities: HostCapabilitiesLane,
    draw_resources: DrawResourcesLane,
    palette: PaletteStateLane,
    telemetry: TelemetryLane,
    diagnostics: DiagnosticsLane,
}

impl RuntimeCell {
    fn new() -> Self {
        Self {
            reducer: ReducerStateLane::default(),
            shell: ShellStateLane::default(),
            timer_bridge: TimerBridgeLane::default(),
            dispatch_queue: DispatchQueueLane::default(),
            host_capabilities: HostCapabilitiesLane::default(),
            draw_resources: DrawResourcesLane::default(),
            palette: PaletteStateLane::default(),
            telemetry: TelemetryLane::default(),
            diagnostics: DiagnosticsLane::default(),
        }
    }

    fn take_reducer_state(&self) -> Result<ReducerState, RuntimeAccessError> {
        self.reducer.take_state()
    }

    fn restore_reducer_state(&self, state: ReducerState) {
        self.reducer.restore_state(state);
    }

    fn take_shell_state(&self) -> Result<ShellState, RuntimeAccessError> {
        self.shell.take_state()
    }

    fn restore_shell_state(&self, state: ShellState) {
        self.shell.restore_state(state);
    }

    fn take_timer_bridge(&self) -> Result<TimerBridge, RuntimeAccessError> {
        self.timer_bridge.take_state()
    }

    fn restore_timer_bridge(&self, bridge: TimerBridge) {
        self.timer_bridge.restore_state(bridge);
    }

    fn with_dispatch_queue<R>(
        &self,
        mutator: impl FnOnce(&mut ScheduledEffectQueueState) -> R,
    ) -> R {
        self.dispatch_queue.with_state(mutator)
    }

    fn with_event_loop_state(&self, mutator: impl FnOnce(&mut EventLoopState)) {
        self.telemetry.with_state(mutator);
    }

    fn read_event_loop_state<R>(&self, reader: impl FnOnce(&EventLoopState) -> R) -> Option<R> {
        self.telemetry.read_state(reader)
    }

    fn set_flush_redraw_capability(&self, capability: FlushRedrawCapability) {
        self.host_capabilities
            .set_flush_redraw_capability(capability);
    }

    fn flush_redraw_capability(&self) -> FlushRedrawCapability {
        self.host_capabilities.flush_redraw_capability()
    }

    #[cfg(test)]
    fn with_event_loop_state_for_test<R>(
        &self,
        mutator: impl FnOnce(&mut EventLoopState) -> R,
    ) -> R {
        self.telemetry.with_state_for_test(mutator)
    }

    fn set_log_level(&self, level: LogLevel) {
        self.diagnostics.set_log_level(level);
    }

    fn should_log(&self, level: LogLevel) -> bool {
        self.diagnostics.should_log(level)
    }

    fn with_log_file_handle<R>(
        &self,
        mutator: impl FnOnce(&mut Option<LogFileWriter>) -> R,
    ) -> Option<R> {
        self.diagnostics.with_log_file_handle(mutator)
    }
}

#[derive(Debug, Default)]
struct ReducerStateLane {
    state: RefCell<ReducerStateSlot>,
}

impl ReducerStateLane {
    fn take_state(&self) -> Result<ReducerState, RuntimeAccessError> {
        let mut slot = self.state.borrow_mut();
        match std::mem::replace(&mut *slot, ReducerStateSlot::InUse) {
            ReducerStateSlot::Ready(state) => Ok(*state),
            ReducerStateSlot::InUse => Err(RuntimeAccessError::Reentered),
        }
    }

    fn restore_state(&self, state: ReducerState) {
        let mut slot = self.state.borrow_mut();
        let previous = std::mem::replace(&mut *slot, ReducerStateSlot::Ready(Box::new(state)));
        debug_assert!(matches!(previous, ReducerStateSlot::InUse));
    }
}

#[derive(Debug)]
enum ReducerStateSlot {
    Ready(Box<ReducerState>),
    InUse,
}

impl Default for ReducerStateSlot {
    fn default() -> Self {
        Self::Ready(Box::default())
    }
}

#[derive(Debug, Default)]
struct ShellStateLane {
    state: RefCell<ShellStateSlot>,
}

impl ShellStateLane {
    fn take_state(&self) -> Result<ShellState, RuntimeAccessError> {
        let mut slot = self.state.borrow_mut();
        match std::mem::replace(&mut *slot, ShellStateSlot::InUse) {
            ShellStateSlot::Ready(state) => Ok(*state),
            ShellStateSlot::InUse => Err(RuntimeAccessError::Reentered),
        }
    }

    fn restore_state(&self, state: ShellState) {
        let mut slot = self.state.borrow_mut();
        let previous = std::mem::replace(&mut *slot, ShellStateSlot::Ready(Box::new(state)));
        debug_assert!(matches!(previous, ShellStateSlot::InUse));
    }
}

#[derive(Debug)]
enum ShellStateSlot {
    Ready(Box<ShellState>),
    InUse,
}

impl Default for ShellStateSlot {
    fn default() -> Self {
        Self::Ready(Box::default())
    }
}

#[derive(Debug, Default)]
struct TimerBridgeLane {
    state: RefCell<TimerBridgeSlot>,
}

impl TimerBridgeLane {
    fn take_state(&self) -> Result<TimerBridge, RuntimeAccessError> {
        let mut slot = self.state.borrow_mut();
        match std::mem::replace(&mut *slot, TimerBridgeSlot::InUse) {
            TimerBridgeSlot::Ready(bridge) => Ok(*bridge),
            TimerBridgeSlot::InUse => Err(RuntimeAccessError::Reentered),
        }
    }

    fn restore_state(&self, bridge: TimerBridge) {
        let mut slot = self.state.borrow_mut();
        let previous = std::mem::replace(&mut *slot, TimerBridgeSlot::Ready(Box::new(bridge)));
        debug_assert!(matches!(previous, TimerBridgeSlot::InUse));
    }
}

#[derive(Debug)]
enum TimerBridgeSlot {
    Ready(Box<TimerBridge>),
    InUse,
}

impl Default for TimerBridgeSlot {
    fn default() -> Self {
        Self::Ready(Box::default())
    }
}

#[derive(Debug, Default)]
struct DispatchQueueLane {
    state: RefCell<ScheduledEffectQueueState>,
}

impl DispatchQueueLane {
    fn with_state<R>(&self, mutator: impl FnOnce(&mut ScheduledEffectQueueState) -> R) -> R {
        // Keep queue borrows scoped to staging/pop bookkeeping only. Reducer execution and effect
        // dispatch always happen after this borrow is released, so re-entering here would signal a
        // structural bug we should fix directly instead of silently dropping queued work.
        let mut state = self.state.borrow_mut();
        mutator(&mut state)
    }
}

#[derive(Debug)]
struct TelemetryLane {
    state: RefCell<EventLoopState>,
}

impl Default for TelemetryLane {
    fn default() -> Self {
        Self {
            state: RefCell::new(EventLoopState::new()),
        }
    }
}

impl TelemetryLane {
    fn with_state(&self, mutator: impl FnOnce(&mut EventLoopState)) {
        // Event-loop telemetry is advisory. If a nested callback is already
        // sampling it, drop the contended sample instead of panicking the plugin.
        let Ok(mut state) = self.state.try_borrow_mut() else {
            return;
        };
        mutator(&mut state);
    }

    fn read_state<R>(&self, reader: impl FnOnce(&EventLoopState) -> R) -> Option<R> {
        let Ok(state) = self.state.try_borrow() else {
            return None;
        };
        Some(reader(&state))
    }

    #[cfg(test)]
    fn with_state_for_test<R>(&self, mutator: impl FnOnce(&mut EventLoopState) -> R) -> R {
        let mut state = self.state.borrow_mut();
        mutator(&mut state)
    }
}

thread_local! {
    // CONTEXT: smear_cursor funnels host callbacks back through Neovim's scheduled
    // main-thread path, so runtime lanes only need single-thread interior mutability.
    static RUNTIME_CELL: RuntimeCell = RuntimeCell::new();
}

pub(super) fn take_reducer_state() -> Result<ReducerState, RuntimeAccessError> {
    RUNTIME_CELL.with(RuntimeCell::take_reducer_state)
}

pub(super) fn restore_reducer_state(state: ReducerState) {
    RUNTIME_CELL.with(|runtime| runtime.restore_reducer_state(state));
}

pub(super) fn take_shell_state() -> Result<ShellState, RuntimeAccessError> {
    RUNTIME_CELL.with(RuntimeCell::take_shell_state)
}

pub(super) fn restore_shell_state(state: ShellState) {
    RUNTIME_CELL.with(|runtime| runtime.restore_shell_state(state));
}

pub(super) fn take_timer_bridge() -> Result<TimerBridge, RuntimeAccessError> {
    RUNTIME_CELL.with(RuntimeCell::take_timer_bridge)
}

pub(super) fn restore_timer_bridge(bridge: TimerBridge) {
    RUNTIME_CELL.with(|runtime| runtime.restore_timer_bridge(bridge));
}

pub(in crate::events) fn with_dispatch_queue<R>(
    mutator: impl FnOnce(&mut ScheduledEffectQueueState) -> R,
) -> R {
    RUNTIME_CELL.with(|runtime| runtime.with_dispatch_queue(mutator))
}

pub(in crate::events) fn with_event_loop_state(mutator: impl FnOnce(&mut EventLoopState)) {
    RUNTIME_CELL.with(|runtime| runtime.with_event_loop_state(mutator));
}

pub(in crate::events) fn read_event_loop_state<R>(
    reader: impl FnOnce(&EventLoopState) -> R,
) -> Option<R> {
    RUNTIME_CELL.with(|runtime| runtime.read_event_loop_state(reader))
}

pub(crate) fn set_flush_redraw_capability(capability: FlushRedrawCapability) {
    RUNTIME_CELL.with(|runtime| runtime.set_flush_redraw_capability(capability));
}

pub(crate) fn flush_redraw_capability() -> FlushRedrawCapability {
    RUNTIME_CELL.with(RuntimeCell::flush_redraw_capability)
}

pub(crate) fn take_draw_render_tabs() -> HashMap<TabHandle, TabWindows> {
    RUNTIME_CELL.with(|runtime| runtime.draw_resources.take_render_tabs())
}

pub(crate) fn restore_draw_render_tabs(render_tabs: HashMap<TabHandle, TabWindows>) {
    RUNTIME_CELL.with(|runtime| runtime.draw_resources.restore_render_tabs(render_tabs));
}

pub(crate) fn take_draw_prepaint_by_tab() -> HashMap<TabHandle, PrepaintOverlay> {
    RUNTIME_CELL.with(|runtime| runtime.draw_resources.take_prepaint_by_tab())
}

pub(crate) fn restore_draw_prepaint_by_tab(prepaint_by_tab: HashMap<TabHandle, PrepaintOverlay>) {
    RUNTIME_CELL.with(|runtime| {
        runtime
            .draw_resources
            .restore_prepaint_by_tab(prepaint_by_tab);
    });
}

pub(crate) fn tracked_runtime_draw_tab_handles() -> Vec<TabHandle> {
    RUNTIME_CELL.with(|runtime| runtime.draw_resources.tracked_tab_handles())
}

pub(crate) fn with_runtime_palette_lane<R>(accessor: impl FnOnce(&PaletteStateLane) -> R) -> R {
    RUNTIME_CELL.with(|runtime| accessor(&runtime.palette))
}

#[cfg(test)]
pub(crate) fn runtime_render_tab_handles_for_test() -> Vec<TabHandle> {
    RUNTIME_CELL.with(|runtime| runtime.draw_resources.render_tab_handles_for_test())
}

#[cfg(test)]
pub(crate) fn clear_runtime_draw_context_for_test() {
    RUNTIME_CELL.with(|runtime| runtime.draw_resources.clear_for_test());
}

#[cfg(test)]
pub(in crate::events) fn with_event_loop_state_for_test<R>(
    mutator: impl FnOnce(&mut EventLoopState) -> R,
) -> R {
    RUNTIME_CELL.with(|runtime| runtime.with_event_loop_state_for_test(mutator))
}

pub(in crate::events) fn set_runtime_log_level(level: LogLevel) {
    RUNTIME_CELL.with(|runtime| runtime.set_log_level(level));
}

pub(in crate::events) fn should_runtime_log(level: LogLevel) -> bool {
    RUNTIME_CELL.with(|runtime| runtime.should_log(level))
}

pub(in crate::events) fn with_runtime_log_file_handle<R>(
    mutator: impl FnOnce(&mut Option<LogFileWriter>) -> R,
) -> Option<R> {
    RUNTIME_CELL.with(|runtime| runtime.with_log_file_handle(mutator))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn reducer_lane_rejects_nested_borrows_until_state_is_restored() {
        let runtime = RuntimeCell::new();
        let state = runtime
            .take_reducer_state()
            .expect("initial reducer state borrow should succeed");

        assert_eq!(
            matches!(
                runtime.take_reducer_state(),
                Err(RuntimeAccessError::Reentered)
            ),
            true
        );

        runtime.restore_reducer_state(state);
        assert_eq!(runtime.take_reducer_state().is_ok(), true);
    }

    #[test]
    fn shell_lane_rejects_nested_borrows_until_state_is_restored() {
        let runtime = RuntimeCell::new();
        let state = runtime
            .take_shell_state()
            .expect("initial shell state borrow should succeed");

        assert_eq!(
            matches!(
                runtime.take_shell_state(),
                Err(RuntimeAccessError::Reentered)
            ),
            true
        );

        runtime.restore_shell_state(state);
        assert_eq!(runtime.take_shell_state().is_ok(), true);
    }

    #[test]
    fn timer_bridge_lane_rejects_nested_borrows_until_bridge_is_restored() {
        let runtime = RuntimeCell::new();
        let bridge = runtime
            .take_timer_bridge()
            .expect("initial timer bridge borrow should succeed");

        assert_eq!(
            matches!(
                runtime.take_timer_bridge(),
                Err(RuntimeAccessError::Reentered)
            ),
            true
        );

        runtime.restore_timer_bridge(bridge);
        assert_eq!(runtime.take_timer_bridge().is_ok(), true);
    }

    #[test]
    fn telemetry_lane_drops_nested_mutation_samples() {
        let runtime = RuntimeCell::new();

        runtime.with_event_loop_state_for_test(|state| {
            runtime.with_event_loop_state(|nested| nested.note_autocmd_event(7.0));
            assert_eq!(state.diagnostics_snapshot().last_autocmd_event_ms, 0.0);
            state.note_autocmd_event(11.0);
        });

        assert_eq!(
            runtime
                .read_event_loop_state(|state| state.diagnostics_snapshot().last_autocmd_event_ms),
            Some(11.0)
        );
    }
}

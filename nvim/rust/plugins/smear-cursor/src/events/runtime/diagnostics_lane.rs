use super::super::logging::LogFileWriter;
use crate::config::LogLevel;
use std::cell::Cell;
use std::cell::RefCell;

#[derive(Debug)]
pub(super) struct DiagnosticsLane {
    log_level: Cell<LogLevel>,
    log_file_handle: RefCell<Option<LogFileWriter>>,
}

impl Default for DiagnosticsLane {
    fn default() -> Self {
        Self {
            log_level: Cell::new(LogLevel::Info),
            log_file_handle: RefCell::new(None),
        }
    }
}

impl DiagnosticsLane {
    pub(super) fn set_log_level(&self, level: LogLevel) {
        self.log_level.set(level);
    }

    pub(super) fn should_log(&self, level: LogLevel) -> bool {
        self.log_level.get().allows(level)
    }

    pub(super) fn with_log_file_handle<R>(
        &self,
        mutator: impl FnOnce(&mut Option<LogFileWriter>) -> R,
    ) -> Option<R> {
        // File logging is diagnostic only. If logging re-enters while a line is being written,
        // drop the nested line instead of letting diagnostics perturb runtime execution.
        let Ok(mut file_handle) = self.log_file_handle.try_borrow_mut() else {
            return None;
        };
        Some(mutator(&mut file_handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn diagnostics_lane_filters_by_typed_log_threshold() {
        let lane = DiagnosticsLane::default();

        lane.set_log_level(LogLevel::Warn);
        assert!(!lane.should_log(LogLevel::Debug));
        assert!(lane.should_log(LogLevel::Warn));

        lane.set_log_level(LogLevel::Off);
        assert!(!lane.should_log(LogLevel::Error));
    }

    #[test]
    fn diagnostics_lane_drops_nested_log_file_mutations() {
        let lane = DiagnosticsLane::default();

        let nested_ran = lane
            .with_log_file_handle(|_| lane.with_log_file_handle(|_| true).unwrap_or(false))
            .unwrap_or(false);

        assert_eq!(nested_ran, false);
        assert_eq!(
            lane.with_log_file_handle(|file_handle| file_handle.is_none()),
            Some(true)
        );
    }
}

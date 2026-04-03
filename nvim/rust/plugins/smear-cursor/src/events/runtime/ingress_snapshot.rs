use super::super::logging::warn;
use super::super::policy::BufferEventPolicy;
use super::super::runtime::resolved_current_buffer_event_policy;
use super::EngineAccessResult;
use super::read_engine_state;
use crate::config::BufferPerfMode;
use crate::config::RuntimeConfig;
use crate::core::state::BufferPerfClass;
use crate::state::CursorLocation;
use crate::types::Point;
use nvim_oxi::api;
use nvimrs_nvim_utils::mode::is_cmdline_mode;
use nvimrs_nvim_utils::mode::is_insert_like_mode;
use nvimrs_nvim_utils::mode::is_replace_like_mode;
use nvimrs_nvim_utils::mode::is_terminal_like_mode;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct IngressModePolicySnapshot(u8);

impl IngressModePolicySnapshot {
    const INSERT: u8 = 1 << 0;
    const REPLACE: u8 = 1 << 1;
    const TERMINAL: u8 = 1 << 2;
    const CMDLINE: u8 = 1 << 3;

    fn from_runtime_config(config: &RuntimeConfig) -> Self {
        Self::from_mode_flags([
            config.smear_insert_mode,
            config.smear_replace_mode,
            config.smear_terminal_mode,
            config.smear_to_cmd,
        ])
    }

    const fn from_mode_flags(mode_flags: [bool; 4]) -> Self {
        let mut encoded = 0;
        if mode_flags[0] {
            encoded |= Self::INSERT;
        }
        if mode_flags[1] {
            encoded |= Self::REPLACE;
        }
        if mode_flags[2] {
            encoded |= Self::TERMINAL;
        }
        if mode_flags[3] {
            encoded |= Self::CMDLINE;
        }
        Self(encoded)
    }

    const fn allows(self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    fn mode_allowed(self, mode: &str) -> bool {
        if is_insert_like_mode(mode) {
            self.allows(Self::INSERT)
        } else if is_replace_like_mode(mode) {
            self.allows(Self::REPLACE)
        } else if is_terminal_like_mode(mode) {
            self.allows(Self::TERMINAL)
        } else if is_cmdline_mode(mode) {
            self.allows(Self::CMDLINE)
        } else {
            true
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IngressReadSnapshot {
    enabled: bool,
    needs_initialize: bool,
    current_corners: [Point; 4],
    tracked_location: Option<CursorLocation>,
    mode_policy: IngressModePolicySnapshot,
    buffer_perf_mode: BufferPerfMode,
    callback_duration_estimate_ms: f64,
    current_buffer_event_policy: Option<BufferEventPolicy>,
    filetypes_disabled: Arc<HashSet<String>>,
}

impl IngressReadSnapshot {
    pub(crate) fn capture() -> EngineAccessResult<Self> {
        let callback_duration_estimate_ms = super::cursor_callback_duration_estimate_ms();
        let mut snapshot = read_engine_state(|state| {
            let runtime = state.core_state.runtime();
            let config = &runtime.config;

            Self {
                enabled: runtime.is_enabled(),
                needs_initialize: state.core_state.needs_initialize(),
                current_corners: runtime.current_corners(),
                tracked_location: runtime.tracked_location(),
                mode_policy: IngressModePolicySnapshot::from_runtime_config(config),
                buffer_perf_mode: config.buffer_perf_mode,
                callback_duration_estimate_ms,
                current_buffer_event_policy: None,
                filetypes_disabled: Arc::clone(&config.filetypes_disabled),
            }
        })?;
        snapshot.current_buffer_event_policy = snapshot.read_current_buffer_event_policy();
        Ok(snapshot)
    }

    pub(crate) const fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) const fn needs_initialize(&self) -> bool {
        self.needs_initialize
    }

    pub(crate) const fn current_corners(&self) -> [Point; 4] {
        self.current_corners
    }

    pub(crate) fn tracked_location(&self) -> Option<&CursorLocation> {
        self.tracked_location.as_ref()
    }

    pub(crate) fn mode_allowed(&self, mode: &str) -> bool {
        self.mode_policy.mode_allowed(mode)
    }

    pub(crate) const fn callback_duration_estimate_ms(&self) -> f64 {
        self.callback_duration_estimate_ms
    }

    pub(crate) const fn buffer_perf_mode(&self) -> BufferPerfMode {
        self.buffer_perf_mode
    }

    pub(crate) const fn current_buffer_event_policy(&self) -> Option<BufferEventPolicy> {
        self.current_buffer_event_policy
    }

    pub(crate) fn current_buffer_perf_class(&self) -> Option<BufferPerfClass> {
        self.current_buffer_event_policy
            .map(BufferEventPolicy::core_perf_class)
    }

    pub(crate) fn has_disabled_filetypes(&self) -> bool {
        !self.filetypes_disabled.is_empty()
    }

    pub(crate) fn filetype_disabled(&self, filetype: &str) -> bool {
        self.filetypes_disabled.contains(filetype)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        enabled: bool,
        needs_initialize: bool,
        current_corners: [Point; 4],
        tracked_location: Option<CursorLocation>,
        mode_policy: (bool, bool, bool, bool),
        buffer_perf_mode: BufferPerfMode,
        callback_duration_estimate_ms: f64,
        current_buffer_perf_class: Option<BufferPerfClass>,
        filetypes_disabled: Vec<String>,
    ) -> Self {
        Self {
            enabled,
            needs_initialize,
            current_corners,
            tracked_location,
            mode_policy: IngressModePolicySnapshot::from_mode_flags([
                mode_policy.0,
                mode_policy.1,
                mode_policy.2,
                mode_policy.3,
            ]),
            buffer_perf_mode,
            callback_duration_estimate_ms,
            current_buffer_event_policy: current_buffer_perf_class.map(test_policy_for_perf_class),
            filetypes_disabled: Arc::new(filetypes_disabled.into_iter().collect()),
        }
    }

    fn read_current_buffer_event_policy(&self) -> Option<BufferEventPolicy> {
        let buffer = api::get_current_buf();
        if !buffer.is_valid() {
            return None;
        }

        match resolved_current_buffer_event_policy(self, &buffer) {
            Ok(policy) => Some(policy),
            Err(err) => {
                warn(&format!(
                    "current buffer perf policy snapshot failed; falling back to live policy reads: {err}"
                ));
                None
            }
        }
    }
}

#[cfg(test)]
fn test_policy_for_perf_class(perf_class: BufferPerfClass) -> BufferEventPolicy {
    match perf_class {
        BufferPerfClass::Full => BufferEventPolicy::from_buffer_metadata("", true, 1, 0.0),
        BufferPerfClass::FastMotion => {
            BufferEventPolicy::from_buffer_metadata("", true, 20_000, 0.0)
        }
        BufferPerfClass::Skip => BufferEventPolicy::from_test_input("", true, 1, 0.0, true),
    }
}

#[cfg(test)]
mod tests {
    use super::IngressModePolicySnapshot;
    use super::IngressReadSnapshot;
    use crate::config::BufferPerfMode;
    use crate::core::state::BufferPerfClass;
    use crate::types::Point;
    use std::collections::HashSet;
    use std::sync::Arc;

    #[test]
    fn ingress_mode_policy_rejects_insert_composite_modes_without_insert_flag() {
        let policy = IngressModePolicySnapshot::from_mode_flags([false, true, false, true]);
        assert!(!policy.mode_allowed("ic"));
    }

    #[test]
    fn ingress_mode_policy_accepts_replace_visual_modes_with_replace_flag() {
        let policy = IngressModePolicySnapshot::from_mode_flags([false, true, false, true]);
        assert!(policy.mode_allowed("Rv"));
    }

    #[test]
    fn ingress_mode_policy_rejects_terminal_pending_modes_without_terminal_flag() {
        let policy = IngressModePolicySnapshot::from_mode_flags([false, true, false, true]);
        assert!(!policy.mode_allowed("ntT"));
    }

    #[test]
    fn ingress_mode_policy_accepts_cmdline_visual_modes_with_cmdline_flag() {
        let policy = IngressModePolicySnapshot::from_mode_flags([false, true, false, true]);
        assert!(policy.mode_allowed("cv"));
    }

    #[test]
    fn ingress_mode_policy_keeps_normal_mode_enabled() {
        let policy = IngressModePolicySnapshot::from_mode_flags([false, true, false, true]);
        assert!(policy.mode_allowed("n"));
    }

    #[test]
    fn ingress_snapshot_filetype_filter_matches_exact_entries() {
        let snapshot = IngressReadSnapshot::new_for_test(
            true,
            false,
            [Point { row: 1.0, col: 2.0 }; 4],
            None,
            (true, true, true, true),
            BufferPerfMode::Auto,
            0.0,
            Some(BufferPerfClass::FastMotion),
            vec!["lua".to_string(), "rust".to_string()],
        );

        assert!(snapshot.has_disabled_filetypes());
        assert!(snapshot.filetype_disabled("lua"));
        assert!(snapshot.filetype_disabled("rust"));
        assert!(!snapshot.filetype_disabled("nix"));
        assert_eq!(snapshot.callback_duration_estimate_ms(), 0.0);
        assert_eq!(
            snapshot.current_buffer_perf_class(),
            Some(BufferPerfClass::FastMotion)
        );
        assert_eq!(
            snapshot
                .current_buffer_event_policy()
                .map(crate::events::policy::BufferEventPolicy::core_perf_class),
            Some(BufferPerfClass::FastMotion)
        );
    }

    #[test]
    fn ingress_read_snapshot_can_share_disabled_filetypes_arc() {
        let filetypes_disabled: Arc<HashSet<String>> = Arc::new(
            ["lua".to_string(), "rust".to_string()]
                .into_iter()
                .collect(),
        );
        let snapshot = IngressReadSnapshot {
            enabled: true,
            needs_initialize: false,
            current_corners: [Point::ZERO; 4],
            tracked_location: None,
            mode_policy: IngressModePolicySnapshot::from_mode_flags([true, true, true, true]),
            buffer_perf_mode: BufferPerfMode::Auto,
            callback_duration_estimate_ms: 12.5,
            current_buffer_event_policy: Some(super::test_policy_for_perf_class(
                BufferPerfClass::FastMotion,
            )),
            filetypes_disabled: Arc::clone(&filetypes_disabled),
        };

        assert!(Arc::ptr_eq(
            &snapshot.filetypes_disabled,
            &filetypes_disabled
        ));
        assert_eq!(snapshot.callback_duration_estimate_ms(), 12.5);
        assert_eq!(
            snapshot.current_buffer_perf_class(),
            Some(BufferPerfClass::FastMotion)
        );
        assert_eq!(
            snapshot
                .current_buffer_event_policy()
                .map(crate::events::policy::BufferEventPolicy::core_perf_class),
            Some(BufferPerfClass::FastMotion)
        );
    }
}

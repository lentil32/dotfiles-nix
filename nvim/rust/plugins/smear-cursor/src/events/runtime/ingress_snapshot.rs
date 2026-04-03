use super::EngineAccessResult;
use super::read_engine_state;
use crate::config::RuntimeConfig;
use crate::state::CursorLocation;
use crate::types::Point;
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
    filetypes_disabled: Arc<HashSet<String>>,
}

impl IngressReadSnapshot {
    pub(crate) fn capture() -> EngineAccessResult<Self> {
        read_engine_state(|state| {
            let runtime = state.core_state.runtime();
            let config = &runtime.config;

            Self {
                enabled: runtime.is_enabled(),
                needs_initialize: state.core_state.needs_initialize(),
                current_corners: runtime.current_corners(),
                tracked_location: runtime.tracked_location(),
                mode_policy: IngressModePolicySnapshot::from_runtime_config(config),
                filetypes_disabled: Arc::clone(&config.filetypes_disabled),
            }
        })
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
            filetypes_disabled: Arc::new(filetypes_disabled.into_iter().collect()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IngressModePolicySnapshot;
    use super::IngressReadSnapshot;
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
            vec!["lua".to_string(), "rust".to_string()],
        );

        assert!(snapshot.has_disabled_filetypes());
        assert!(snapshot.filetype_disabled("lua"));
        assert!(snapshot.filetype_disabled("rust"));
        assert!(!snapshot.filetype_disabled("nix"));
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
            filetypes_disabled: Arc::clone(&filetypes_disabled),
        };

        assert!(Arc::ptr_eq(
            &snapshot.filetypes_disabled,
            &filetypes_disabled
        ));
    }
}

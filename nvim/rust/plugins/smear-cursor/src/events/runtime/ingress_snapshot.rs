use super::super::logging::warn;
use super::super::policy::BufferEventPolicy;
use super::super::runtime::resolved_current_buffer_event_policy;
use super::EngineAccessResult;
use super::read_engine_state;
use crate::config::BufferPerfMode;
use crate::config::RuntimeConfig;
use crate::core::state::BufferPerfClass;
use crate::state::CursorLocation;
use crate::types::CursorCellShape;
use crate::types::Point;
use crate::types::ScreenCell;
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
    target_corners: [Point; 4],
    target_position: Point,
    tracked_location: Option<CursorLocation>,
    mode_policy: IngressModePolicySnapshot,
    buffer_perf_mode: BufferPerfMode,
    callback_duration_estimate_ms: f64,
    current_buffer_event_policy: Option<BufferEventPolicy>,
    filetypes_disabled: Arc<HashSet<String>>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct IngressReadSnapshotTestInput {
    pub(crate) enabled: bool,
    pub(crate) needs_initialize: bool,
    pub(crate) current_corners: [Point; 4],
    pub(crate) target_corners: [Point; 4],
    pub(crate) target_position: Point,
    pub(crate) tracked_location: Option<CursorLocation>,
    pub(crate) mode_flags: [bool; 4],
    pub(crate) buffer_perf_mode: BufferPerfMode,
    pub(crate) callback_duration_estimate_ms: f64,
    pub(crate) current_buffer_perf_class: Option<BufferPerfClass>,
    pub(crate) filetypes_disabled: Vec<String>,
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
                target_corners: runtime.target_corners(),
                target_position: runtime.target_position(),
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

    pub(crate) fn current_visual_cursor_cell(&self) -> Option<ScreenCell> {
        ScreenCell::from_visual_cursor_anchor(
            &self.current_corners,
            &self.target_corners,
            self.target_position,
        )
    }

    pub(crate) fn current_visual_cursor_shape(&self) -> CursorCellShape {
        CursorCellShape::from_corners(&self.target_corners)
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
            .map(BufferEventPolicy::perf_class)
    }

    pub(crate) fn has_disabled_filetypes(&self) -> bool {
        !self.filetypes_disabled.is_empty()
    }

    pub(crate) fn filetype_disabled(&self, filetype: &str) -> bool {
        self.filetypes_disabled.contains(filetype)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(input: IngressReadSnapshotTestInput) -> Self {
        Self {
            enabled: input.enabled,
            needs_initialize: input.needs_initialize,
            current_corners: input.current_corners,
            target_corners: input.target_corners,
            target_position: input.target_position,
            tracked_location: input.tracked_location,
            mode_policy: IngressModePolicySnapshot::from_mode_flags(input.mode_flags),
            buffer_perf_mode: input.buffer_perf_mode,
            callback_duration_estimate_ms: input.callback_duration_estimate_ms,
            current_buffer_event_policy: input
                .current_buffer_perf_class
                .map(test_policy_for_perf_class),
            filetypes_disabled: Arc::new(input.filetypes_disabled.into_iter().collect()),
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
    use super::IngressReadSnapshotTestInput;
    use crate::config::BufferPerfMode;
    use crate::core::state::BufferPerfClass;
    use crate::test_support::proptest::pure_config;
    use crate::types::Point;
    use crate::types::ScreenCell;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    fn current_buffer_perf_class_strategy() -> BoxedStrategy<Option<BufferPerfClass>> {
        prop_oneof![
            Just(None),
            Just(Some(BufferPerfClass::Full)),
            Just(Some(BufferPerfClass::FastMotion)),
            Just(Some(BufferPerfClass::Skip)),
        ]
        .boxed()
    }

    proptest! {
        #![proptest_config(pure_config())]

        #[test]
        fn prop_ingress_snapshot_filetype_filter_matches_exact_entries(
            lua_disabled in any::<bool>(),
            rust_disabled in any::<bool>(),
            nix_disabled in any::<bool>(),
            go_disabled in any::<bool>(),
            query in prop_oneof![
                Just("lua"),
                Just("rust"),
                Just("nix"),
                Just("go"),
                Just("toml"),
            ],
            callback_duration_estimate_ms in 0_u16..=200_u16,
            current_buffer_perf_class in current_buffer_perf_class_strategy(),
        ) {
            let mut disabled = Vec::new();
            if lua_disabled {
                disabled.push("lua".to_string());
            }
            if rust_disabled {
                disabled.push("rust".to_string());
            }
            if nix_disabled {
                disabled.push("nix".to_string());
            }
            if go_disabled {
                disabled.push("go".to_string());
            }

            let snapshot = IngressReadSnapshot::new_for_test(IngressReadSnapshotTestInput {
                enabled: true,
                needs_initialize: false,
                current_corners: [Point { row: 1.0, col: 2.0 }; 4],
                target_corners: [Point { row: 1.0, col: 2.0 }; 4],
                target_position: Point { row: 1.0, col: 2.0 },
                tracked_location: None,
                mode_flags: [true, true, true, true],
                buffer_perf_mode: BufferPerfMode::Auto,
                callback_duration_estimate_ms: f64::from(callback_duration_estimate_ms),
                current_buffer_perf_class,
                filetypes_disabled: disabled,
            });
            let expected_disabled = match query {
                "lua" => lua_disabled,
                "rust" => rust_disabled,
                "nix" => nix_disabled,
                "go" => go_disabled,
                "toml" => false,
                _ => unreachable!("filetype query strategy only generates known literals"),
            };

            prop_assert_eq!(
                snapshot.has_disabled_filetypes(),
                lua_disabled || rust_disabled || nix_disabled || go_disabled
            );
            prop_assert_eq!(snapshot.filetype_disabled(query), expected_disabled);
            prop_assert_eq!(
                snapshot.callback_duration_estimate_ms(),
                f64::from(callback_duration_estimate_ms)
            );
            prop_assert_eq!(
                snapshot.current_buffer_perf_class(),
                current_buffer_perf_class
            );
            prop_assert_eq!(
                snapshot
                    .current_buffer_event_policy()
                    .map(crate::events::policy::BufferEventPolicy::perf_class),
                current_buffer_perf_class
            );
        }
    }

    #[test]
    fn ingress_mode_policy_smoke_routes_known_mode_families() {
        let policy = IngressModePolicySnapshot::from_mode_flags([false, true, false, true]);

        assert!(!policy.mode_allowed("i"));
        assert!(policy.mode_allowed("R"));
        assert!(!policy.mode_allowed("t"));
        assert!(policy.mode_allowed("cv"));
        assert!(policy.mode_allowed("n"));
        assert!(policy.mode_allowed("v"));
    }

    #[test]
    fn ingress_read_snapshot_preserves_disabled_filetypes_arc_sharing_as_a_perf_contract() {
        let filetypes_disabled: Arc<HashSet<String>> = Arc::new(
            ["lua".to_string(), "rust".to_string()]
                .into_iter()
                .collect(),
        );
        let snapshot = IngressReadSnapshot {
            enabled: true,
            needs_initialize: false,
            current_corners: [Point::ZERO; 4],
            target_corners: [Point::ZERO; 4],
            target_position: Point::ZERO,
            tracked_location: None,
            mode_policy: IngressModePolicySnapshot::from_mode_flags([true, true, true, true]),
            buffer_perf_mode: BufferPerfMode::Auto,
            callback_duration_estimate_ms: 12.5,
            current_buffer_event_policy: Some(super::test_policy_for_perf_class(
                BufferPerfClass::FastMotion,
            )),
            filetypes_disabled: Arc::clone(&filetypes_disabled),
        };

        assert!(
            Arc::ptr_eq(&snapshot.filetypes_disabled, &filetypes_disabled),
            "snapshot should reuse the disabled-filetypes Arc instead of cloning the set"
        );
        assert_eq!(snapshot.callback_duration_estimate_ms(), 12.5);
        assert_eq!(
            snapshot.current_buffer_perf_class(),
            Some(BufferPerfClass::FastMotion)
        );
        assert_eq!(
            snapshot
                .current_buffer_event_policy()
                .map(crate::events::policy::BufferEventPolicy::perf_class),
            Some(BufferPerfClass::FastMotion)
        );
    }

    #[test]
    fn ingress_snapshot_smoke_exposes_visual_cursor_accessors_from_stored_runtime_geometry() {
        let snapshot = IngressReadSnapshot::new_for_test(IngressReadSnapshotTestInput {
            enabled: true,
            needs_initialize: false,
            current_corners: [
                Point {
                    row: 9.0,
                    col: 14.0,
                },
                Point {
                    row: 10.0,
                    col: 14.0,
                },
                Point {
                    row: 10.0,
                    col: 15.0,
                },
                Point {
                    row: 9.0,
                    col: 15.0,
                },
            ],
            target_corners: [
                Point {
                    row: 9.0,
                    col: 14.0,
                },
                Point {
                    row: 10.0,
                    col: 14.0,
                },
                Point {
                    row: 10.0,
                    col: 15.0,
                },
                Point {
                    row: 9.0,
                    col: 15.0,
                },
            ],
            target_position: Point {
                row: 10.0,
                col: 15.0,
            },
            tracked_location: None,
            mode_flags: [true, true, true, true],
            buffer_perf_mode: BufferPerfMode::Auto,
            callback_duration_estimate_ms: 0.0,
            current_buffer_perf_class: None,
            filetypes_disabled: Vec::new(),
        });

        assert_eq!(
            snapshot.current_visual_cursor_cell(),
            ScreenCell::new(10, 15)
        );
        assert_eq!(
            snapshot.current_visual_cursor_shape(),
            crate::types::CursorCellShape::Block
        );
    }
}

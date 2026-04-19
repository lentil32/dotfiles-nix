use super::super::logging::warn;
use super::super::policy::BufferEventPolicy;
use super::super::runtime::resolved_current_buffer_event_policy;
use super::EngineAccessResult;
use super::read_engine_state;
use crate::config::BufferPerfMode;
use crate::config::RuntimeConfig;
use crate::core::state::BufferPerfClass;
use crate::position::RenderPoint;
use crate::position::ScreenCell;
use crate::position::current_visual_cursor_anchor;
use crate::state::TrackedCursor;
use crate::types::CursorCellShape;
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

// Operation-scoped copy of shell-visible runtime facts for one ingress read. This
// snapshot may borrow immutable shell-owned data, but it must never become a
// retained reducer-owned semantic owner.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IngressReadSnapshot {
    enabled: bool,
    needs_initialize: bool,
    current_corners: [RenderPoint; 4],
    target_corners: [RenderPoint; 4],
    target_position: RenderPoint,
    tracked_cursor: Option<TrackedCursor>,
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
    pub(crate) current_corners: [RenderPoint; 4],
    pub(crate) target_corners: [RenderPoint; 4],
    pub(crate) target_position: RenderPoint,
    pub(crate) tracked_cursor: Option<TrackedCursor>,
    pub(crate) mode_flags: [bool; 4],
    pub(crate) buffer_perf_mode: BufferPerfMode,
    pub(crate) callback_duration_estimate_ms: f64,
    pub(crate) current_buffer_perf_class: Option<BufferPerfClass>,
    pub(crate) filetypes_disabled: Vec<String>,
}

impl IngressReadSnapshot {
    #[cfg(not(test))]
    pub(crate) fn capture() -> EngineAccessResult<Self> {
        let current_buffer = api::get_current_buf();
        let current_buffer = current_buffer.is_valid().then_some(current_buffer);
        Self::capture_with_current_buffer(current_buffer.as_ref())
    }

    pub(crate) fn capture_with_current_buffer(
        current_buffer: Option<&api::Buffer>,
    ) -> EngineAccessResult<Self> {
        let callback_duration_estimate_ms = super::cursor_callback_duration_estimate_ms(
            current_buffer.map(api::Buffer::handle).map(i64::from),
        );
        let mut snapshot = read_engine_state(|state| {
            let runtime = state.core_state.runtime();
            let config = &runtime.config;

            Self {
                enabled: runtime.is_enabled(),
                needs_initialize: state.core_state.needs_initialize(),
                current_corners: runtime.current_corners(),
                target_corners: runtime.target_corners(),
                target_position: runtime.target_position(),
                tracked_cursor: runtime.tracked_cursor(),
                mode_policy: IngressModePolicySnapshot::from_runtime_config(config),
                buffer_perf_mode: config.buffer_perf_mode,
                callback_duration_estimate_ms,
                current_buffer_event_policy: None,
                filetypes_disabled: Arc::clone(&config.filetypes_disabled),
            }
        })?;
        if snapshot.enabled {
            snapshot.current_buffer_event_policy =
                snapshot.read_current_buffer_event_policy(current_buffer);
        }
        Ok(snapshot)
    }

    pub(crate) const fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) const fn needs_initialize(&self) -> bool {
        self.needs_initialize
    }

    pub(crate) const fn current_corners(&self) -> [RenderPoint; 4] {
        self.current_corners
    }

    pub(crate) fn current_visual_cursor_cell(&self) -> Option<ScreenCell> {
        ScreenCell::from_rounded_point(current_visual_cursor_anchor(
            &self.current_corners,
            &self.target_corners,
            self.target_position,
        ))
    }

    pub(crate) fn current_visual_cursor_shape(&self) -> CursorCellShape {
        CursorCellShape::from_corners(&self.target_corners)
    }

    pub(crate) fn tracked_cursor(&self) -> Option<&TrackedCursor> {
        self.tracked_cursor.as_ref()
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
            tracked_cursor: input.tracked_cursor,
            mode_policy: IngressModePolicySnapshot::from_mode_flags(input.mode_flags),
            buffer_perf_mode: input.buffer_perf_mode,
            callback_duration_estimate_ms: input.callback_duration_estimate_ms,
            current_buffer_event_policy: input
                .enabled
                .then_some(input.current_buffer_perf_class)
                .flatten()
                .map(test_policy_for_perf_class),
            filetypes_disabled: Arc::new(input.filetypes_disabled.into_iter().collect()),
        }
    }

    fn read_current_buffer_event_policy(
        &self,
        buffer: Option<&api::Buffer>,
    ) -> Option<BufferEventPolicy> {
        let buffer = buffer?;

        match resolved_current_buffer_event_policy(self, buffer) {
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
    use crate::position::RenderPoint;
    use crate::position::ScreenCell;
    use crate::test_support::proptest::pure_config;
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
                current_corners: [RenderPoint { row: 1.0, col: 2.0 }; 4],
                target_corners: [RenderPoint { row: 1.0, col: 2.0 }; 4],
                target_position: RenderPoint { row: 1.0, col: 2.0 },
                tracked_cursor: None,
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
            current_corners: [RenderPoint::ZERO; 4],
            target_corners: [RenderPoint::ZERO; 4],
            target_position: RenderPoint::ZERO,
            tracked_cursor: None,
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
                RenderPoint {
                    row: 9.0,
                    col: 14.0,
                },
                RenderPoint {
                    row: 10.0,
                    col: 14.0,
                },
                RenderPoint {
                    row: 10.0,
                    col: 15.0,
                },
                RenderPoint {
                    row: 9.0,
                    col: 15.0,
                },
            ],
            target_corners: [
                RenderPoint {
                    row: 9.0,
                    col: 14.0,
                },
                RenderPoint {
                    row: 10.0,
                    col: 14.0,
                },
                RenderPoint {
                    row: 10.0,
                    col: 15.0,
                },
                RenderPoint {
                    row: 9.0,
                    col: 15.0,
                },
            ],
            target_position: RenderPoint {
                row: 10.0,
                col: 15.0,
            },
            tracked_cursor: None,
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

    #[test]
    fn disabled_ingress_snapshot_omits_current_buffer_policy() {
        let disabled_snapshot = IngressReadSnapshot::new_for_test(IngressReadSnapshotTestInput {
            enabled: false,
            needs_initialize: false,
            current_corners: [RenderPoint::ZERO; 4],
            target_corners: [RenderPoint::ZERO; 4],
            target_position: RenderPoint::ZERO,
            tracked_cursor: None,
            mode_flags: [true, true, true, true],
            buffer_perf_mode: BufferPerfMode::Auto,
            callback_duration_estimate_ms: 0.0,
            current_buffer_perf_class: Some(BufferPerfClass::Skip),
            filetypes_disabled: Vec::new(),
        });

        assert_eq!(disabled_snapshot.current_buffer_event_policy(), None);
        assert_eq!(disabled_snapshot.current_buffer_perf_class(), None);
    }

    #[test]
    fn ingress_snapshot_remains_stable_after_later_core_state_mutation() {
        let shape = crate::state::CursorShape::block();
        let initial_location = crate::state::TrackedCursor::fixture(11, 22, 3, 4);
        let mut initial_runtime = crate::state::RuntimeState::default();
        initial_runtime.initialize_cursor(
            RenderPoint { row: 3.0, col: 4.0 },
            shape,
            7,
            &initial_location,
        );
        initial_runtime.config.smear_to_cmd = true;
        super::super::set_core_state(
            crate::core::state::CoreState::default().with_runtime(initial_runtime),
        )
        .expect("test core state write should succeed");

        let snapshot =
            IngressReadSnapshot::capture_with_current_buffer(None).expect("snapshot capture");
        let expected = snapshot.clone();

        let mutated_location = crate::state::TrackedCursor::fixture(99, 88, 7, 6);
        let mut mutated_runtime = crate::state::RuntimeState::default();
        mutated_runtime.set_enabled(false);
        mutated_runtime.initialize_cursor(
            RenderPoint {
                row: 30.0,
                col: 40.0,
            },
            shape,
            9,
            &mutated_location,
        );
        mutated_runtime.config.smear_to_cmd = false;
        super::super::set_core_state(
            crate::core::state::CoreState::default().with_runtime(mutated_runtime),
        )
        .expect("mutated test core state write should succeed");

        assert_eq!(snapshot, expected);
    }
}

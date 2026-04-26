use super::HostBridgeRevision;
use super::HostBridgeState;
use super::RuntimeAccessError;
use super::runtime::namespace_id;
use super::runtime::set_namespace_id;
use super::timer_protocol::HostCallbackId;
#[cfg(test)]
use crate::host::BACKGROUND_ALLOWED_MASK_FUNCTION_NAME;
#[cfg(test)]
use crate::host::CURSOR_COLOR_AT_CURSOR_FUNCTION_NAME;
use crate::host::CursorColorExtmarkFallback;
#[cfg(test)]
use crate::host::DISPATCH_TIMER_FUNCTION_NAME;
#[cfg(test)]
use crate::host::HOST_BRIDGE_REVISION_FUNCTION_NAME;
use crate::host::HostBridgePort;
#[cfg(test)]
use crate::host::INSTALL_PROBE_HELPERS_FUNCTION_NAME;
use crate::host::NamespaceId;
use crate::host::NeovimHost;
#[cfg(test)]
use crate::host::START_TIMER_ONCE_FUNCTION_NAME;
#[cfg(test)]
use crate::host::STOP_TIMER_FUNCTION_NAME;
use nvim_oxi::Array;
use nvim_oxi::Object;
use thiserror::Error;

pub(super) use crate::host::DISPATCH_AUTOCMD_FUNCTION_NAME;
#[cfg(test)]
const HOST_BRIDGE_SCRIPT: &str = include_str!("../../autoload/nvimrs_smear_cursor/host_bridge.vim");
#[cfg(test)]
const PROBE_HELPERS_SCRIPT: &str = include_str!("../../lua/nvimrs_smear_cursor/probes.lua");
#[cfg(test)]
const CURSOR_COLOR_EXTMARKS_TEST_SCRIPT: &str =
    include_str!("../../scripts/test_cursor_color_probe_extmarks.lua");

fn host_bridge_state() -> Result<HostBridgeState, RuntimeAccessError> {
    super::runtime::host_bridge_state()
}

fn note_host_bridge_verified(revision: HostBridgeRevision) -> Result<(), RuntimeAccessError> {
    super::runtime::note_host_bridge_verified(revision)
}

type HostBridgeResult<T> = std::result::Result<T, HostBridgeError>;

#[derive(Debug, Error)]
pub(super) enum HostBridgeError {
    #[error(
        "smear cursor host bridge revision mismatch: expected v{expected}, found v{found}",
        expected = .expected.get(),
        found = .found.get()
    )]
    RevisionMismatch {
        expected: HostBridgeRevision,
        found: HostBridgeRevision,
    },
    #[error(
        "smear cursor host bridge is not verified; call setup() before scheduling host effects"
    )]
    Unverified,
    #[error("failed to query smear cursor host bridge revision: {0}")]
    RevisionQuery(#[source] nvim_oxi::Error),
    #[error("smear cursor host bridge returned invalid revision: {value}")]
    RevisionDecode { value: i64 },
    #[error("failed to start smear cursor host bridge timer: {0}")]
    StartTimerOnce(#[source] nvim_oxi::Error),
    #[error("failed to stop smear cursor host bridge timer: {0}")]
    StopTimer(#[source] nvim_oxi::Error),
    #[error("failed to install smear cursor probe helpers: {0}")]
    InstallProbeHelpers(#[source] nvim_oxi::Error),
    #[error("failed to call smear cursor cursor-color probe: {0}")]
    CursorColorProbe(#[source] nvim_oxi::Error),
    #[error("failed to call smear cursor background-mask probe: {0}")]
    BackgroundAllowedMask(#[source] nvim_oxi::Error),
    #[error("runtime access failed while resolving host bridge state: {0}")]
    RuntimeAccess(#[from] RuntimeAccessError),
}

impl From<HostBridgeError> for nvim_oxi::Error {
    fn from(error: HostBridgeError) -> Self {
        crate::other_error(error.to_string())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct InstalledHostBridge;

impl InstalledHostBridge {
    pub(super) fn start_timer_once(
        self,
        host_callback_id: HostCallbackId,
        timeout_ms: i64,
    ) -> HostBridgeResult<i64> {
        self.start_timer_once_with(&NeovimHost, host_callback_id, timeout_ms)
    }

    fn start_timer_once_with(
        self,
        host: &impl HostBridgePort,
        host_callback_id: HostCallbackId,
        timeout_ms: i64,
    ) -> HostBridgeResult<i64> {
        host.start_timer_once(host_callback_id.get(), timeout_ms)
            .map_err(HostBridgeError::StartTimerOnce)
    }

    pub(in crate::events) fn stop_timer_with(
        self,
        host: &impl HostBridgePort,
        timer_id: i64,
    ) -> HostBridgeResult<()> {
        host.stop_timer(timer_id)
            .map_err(HostBridgeError::StopTimer)
    }

    fn install_probe_helpers_with(self, host: &impl HostBridgePort) -> HostBridgeResult<()> {
        host.install_probe_helpers()
            .map_err(HostBridgeError::InstallProbeHelpers)
    }

    pub(super) fn cursor_color_at_cursor(
        self,
        extmark_fallback: CursorColorExtmarkFallback,
    ) -> HostBridgeResult<Object> {
        self.cursor_color_at_cursor_with(&NeovimHost, extmark_fallback)
    }

    fn cursor_color_at_cursor_with(
        self,
        host: &impl HostBridgePort,
        extmark_fallback: CursorColorExtmarkFallback,
    ) -> HostBridgeResult<Object> {
        host.cursor_color_at_cursor(extmark_fallback)
            .map_err(HostBridgeError::CursorColorProbe)
    }

    pub(super) fn background_allowed_mask(self, request: Array) -> HostBridgeResult<Object> {
        self.background_allowed_mask_with(&NeovimHost, request)
    }

    fn background_allowed_mask_with(
        self,
        host: &impl HostBridgePort,
        request: Array,
    ) -> HostBridgeResult<Object> {
        host.background_allowed_mask(request)
            .map_err(HostBridgeError::BackgroundAllowedMask)
    }
}

fn installed_host_bridge_from_state() -> HostBridgeResult<InstalledHostBridge> {
    match host_bridge_state()? {
        HostBridgeState::Verified { revision } if revision == HostBridgeRevision::CURRENT => {
            Ok(InstalledHostBridge)
        }
        HostBridgeState::Verified { revision } => Err(HostBridgeError::RevisionMismatch {
            expected: HostBridgeRevision::CURRENT,
            found: revision,
        }),
        HostBridgeState::Unverified => Err(HostBridgeError::Unverified),
    }
}

fn query_host_bridge_revision_with(
    host: &impl HostBridgePort,
) -> HostBridgeResult<HostBridgeRevision> {
    let revision = host
        .host_bridge_revision()
        .map_err(HostBridgeError::RevisionQuery)?;
    let revision =
        u32::try_from(revision).map_err(|_| HostBridgeError::RevisionDecode { value: revision })?;
    Ok(HostBridgeRevision(revision))
}

struct HostBridge;

impl HostBridge {
    fn verify() -> HostBridgeResult<InstalledHostBridge> {
        Self::verify_with(&NeovimHost)
    }

    fn verify_with(host: &impl HostBridgePort) -> HostBridgeResult<InstalledHostBridge> {
        match installed_host_bridge_from_state() {
            Ok(host_bridge) => {
                host_bridge.install_probe_helpers_with(host)?;
                Ok(host_bridge)
            }
            Err(_) => {
                let revision = query_host_bridge_revision_with(host)?;
                if revision != HostBridgeRevision::CURRENT {
                    return Err(HostBridgeError::RevisionMismatch {
                        expected: HostBridgeRevision::CURRENT,
                        found: revision,
                    });
                }

                let host_bridge = InstalledHostBridge;
                host_bridge.install_probe_helpers_with(host)?;
                note_host_bridge_verified(revision)?;
                Ok(host_bridge)
            }
        }
    }
}

pub(super) fn verify_host_bridge() -> HostBridgeResult<InstalledHostBridge> {
    HostBridge::verify()
}

pub(super) fn installed_host_bridge() -> HostBridgeResult<InstalledHostBridge> {
    installed_host_bridge_from_state()
}

pub(super) fn ensure_namespace_id() -> Result<NamespaceId, RuntimeAccessError> {
    ensure_namespace_id_with(&NeovimHost)
}

fn ensure_namespace_id_with(host: &impl HostBridgePort) -> Result<NamespaceId, RuntimeAccessError> {
    if let Some(namespace_id) = namespace_id()? {
        return Ok(namespace_id);
    }

    let created = host.create_namespace("nvimrs_smear_cursor");
    set_namespace_id(created)?;
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::FakeHostBridgePort;
    use crate::host::HostBridgeCall;
    use pretty_assertions::assert_eq;

    type SubstringCase<'a> = (&'a str, &'a str, &'a str, &'a [&'a str], &'a [&'a str]);

    fn reset_host_bridge_shell_state_for_test() {
        super::super::runtime::mutate_shell_state(|state| {
            *state = super::super::ShellState::default();
        })
        .expect("shell state should be available for host bridge test setup");
    }

    fn assert_substring_contract(
        script_name: &str,
        script: &str,
        case_name: &str,
        required: &[&str],
        forbidden: &[&str],
    ) {
        for needle in required {
            assert!(
                script.contains(needle),
                "{script_name} missing required substring for {case_name}: {needle}"
            );
        }

        for needle in forbidden {
            assert!(
                !script.contains(needle),
                "{script_name} unexpectedly contained forbidden substring for {case_name}: {needle}"
            );
        }
    }

    #[test]
    fn verify_host_bridge_uses_the_host_port_and_records_verified_revision() {
        reset_host_bridge_shell_state_for_test();
        let host = FakeHostBridgePort::default();
        host.push_host_bridge_revision(
            /*revision*/ i64::from(HostBridgeRevision::CURRENT.get()),
        );

        let verified = HostBridge::verify_with(&host)
            .expect("fake host should satisfy the current host bridge revision");
        let state = host_bridge_state().expect("shell state should be readable after verification");

        assert_eq!(
            (verified, state, host.calls()),
            (
                InstalledHostBridge,
                HostBridgeState::Verified {
                    revision: HostBridgeRevision::CURRENT,
                },
                vec![
                    HostBridgeCall::HostBridgeRevision,
                    HostBridgeCall::InstallProbeHelpers,
                ],
            )
        );
    }

    #[test]
    fn installed_host_bridge_schedules_timers_through_the_host_port() {
        let host = FakeHostBridgePort::default();
        host.push_start_timer_once(/*host_timer_id*/ 91);
        let host_callback_id =
            HostCallbackId::try_new(/*value*/ 7).expect("test callback id should be positive");

        let host_timer_id = InstalledHostBridge
            .start_timer_once_with(&host, host_callback_id, /*timeout_ms*/ 25)
            .expect("fake host should return a host timer id");

        assert_eq!(
            (host_timer_id, host.calls()),
            (
                91,
                vec![HostBridgeCall::StartTimerOnce {
                    host_callback_id: 7,
                    timeout_ms: 25,
                }],
            )
        );
    }

    #[test]
    fn installed_host_bridge_stops_timers_through_the_host_port() {
        let host = FakeHostBridgePort::default();

        InstalledHostBridge
            .stop_timer_with(&host, /*timer_id*/ 44)
            .expect("fake host should stop the timer");

        assert_eq!(
            host.calls(),
            vec![HostBridgeCall::StopTimer { timer_id: 44 }]
        );
    }

    #[test]
    fn installed_host_bridge_routes_cursor_color_fallback_policy_through_the_host_port() {
        let host = FakeHostBridgePort::default();

        InstalledHostBridge
            .cursor_color_at_cursor_with(&host, CursorColorExtmarkFallback::SyntaxThenExtmarks)
            .expect("fake host should accept cursor color probe requests");

        assert_eq!(
            host.calls(),
            vec![HostBridgeCall::CursorColorAtCursor {
                extmark_fallback: CursorColorExtmarkFallback::SyntaxThenExtmarks,
            }]
        );
    }

    #[test]
    fn installed_host_bridge_routes_background_mask_requests_through_the_host_port() {
        let host = FakeHostBridgePort::default();

        InstalledHostBridge
            .background_allowed_mask_with(&host, Array::from_iter([Object::from(7_i64)]))
            .expect("fake host should accept background mask probe requests");

        assert!(matches!(
            host.calls().as_slice(),
            [HostBridgeCall::BackgroundAllowedMask { .. }]
        ));
    }

    #[test]
    fn ensure_namespace_id_allocates_through_the_host_port_once() {
        reset_host_bridge_shell_state_for_test();
        let host = FakeHostBridgePort::default();
        host.set_namespace_id(NamespaceId::new(/*value*/ 33));

        let first = ensure_namespace_id_with(&host)
            .expect("fake host namespace allocation should update shell state");
        let second = ensure_namespace_id_with(&host)
            .expect("cached namespace id should not call the fake host again");
        let cached_namespace_id =
            namespace_id().expect("shell state should be readable after namespace allocation");

        assert_eq!(
            (first, second, cached_namespace_id, host.calls()),
            (
                NamespaceId::new(/*value*/ 33),
                NamespaceId::new(/*value*/ 33),
                Some(NamespaceId::new(/*value*/ 33)),
                vec![HostBridgeCall::CreateNamespace {
                    name: "nvimrs_smear_cursor".to_string(),
                }],
            )
        );
    }

    #[test]
    fn host_bridge_script_contract() {
        let cases: &[(&str, &[&str], &[&str])] = &[
            (
                "entrypoints present",
                &[
                    HOST_BRIDGE_REVISION_FUNCTION_NAME,
                    DISPATCH_AUTOCMD_FUNCTION_NAME,
                    DISPATCH_TIMER_FUNCTION_NAME,
                    START_TIMER_ONCE_FUNCTION_NAME,
                    STOP_TIMER_FUNCTION_NAME,
                    INSTALL_PROBE_HELPERS_FUNCTION_NAME,
                    CURSOR_COLOR_AT_CURSOR_FUNCTION_NAME,
                    BACKGROUND_ALLOWED_MASK_FUNCTION_NAME,
                ],
                &[],
            ),
            (
                "autocmd bridge forwards explicit payload fields",
                &[
                    "dispatch_autocmd(event, buffer, match) abort",
                    ".on_autocmd_payload(_A)",
                    "{'event': a:event, 'buffer': a:buffer, 'match': a:match}",
                ],
                &[],
            ),
            (
                "timer bridge delegated to builtin vim timers",
                &[
                    "dispatch_timer(host_callback_id, timer_id) abort",
                    "let Callback = function(",
                    "'nvimrs_smear_cursor#host_bridge#dispatch_timer'",
                    "[a:host_callback_id]",
                    "return timer_start(a:timeout, Callback)",
                    "return timer_stop(a:timer_id)",
                    ".on_core_timer_fired(_A[1], _A[2])",
                ],
                &[
                    "require('nvimrs_smear_cursor.host_bridge')",
                    "uv.new_timer()",
                    "reset_timers",
                    "dispatch_animation_timer",
                    "dispatch_ingress_timer",
                    "dispatch_recovery_timer",
                    "dispatch_cleanup_timer",
                    "dispatch_timer(timer_name, timer_id)",
                    "timer_callbacks",
                    "timer_callback_name",
                    "on_core_timer_slot",
                    "a:timer_name",
                    "unknown smear cursor timer slot",
                    "token_generation",
                    "timer_payloads",
                ],
            ),
            (
                "runtime-module loading shape",
                &["require('nvimrs_smear_cursor.probes')"],
                &[],
            ),
        ];

        for &(case_name, required, forbidden) in cases {
            assert_substring_contract(
                "host bridge script",
                HOST_BRIDGE_SCRIPT,
                case_name,
                required,
                forbidden,
            );
        }
    }

    #[test]
    fn probe_helpers_contract() {
        let cases: &[SubstringCase<'_>] = &[
            (
                "host bridge script",
                HOST_BRIDGE_SCRIPT,
                "cursor-color probe bridge forwards the explicit fallback flag",
                &[
                    "cursor_color_at_cursor(allow_extmark_fallback) abort",
                    ".cursor_color_at_cursor(_A)",
                ],
                &[],
            ),
            (
                "probe helpers script",
                PROBE_HELPERS_SCRIPT,
                "extmark fallback gate present",
                &[
                    "function M.cursor_color_at_cursor(allow_extmark_fallback)",
                    "if not allow_extmark_fallback then",
                ],
                &[],
            ),
            (
                "probe helpers script",
                PROBE_HELPERS_SCRIPT,
                "fallback usage field present",
                &["used_extmark_fallback"],
                &[],
            ),
            (
                "probe helpers script",
                PROBE_HELPERS_SCRIPT,
                "extmark fallback stays bounded",
                &[
                    "local EXTMARK_OVERLAP_TRUSTED_LIMIT = 32",
                    "local EXTMARK_OVERLAP_PROBE_LIMIT = EXTMARK_OVERLAP_TRUSTED_LIMIT + 1",
                    "local function overlapping_extmarks_at_cursor(cursor)",
                    "{ details = true, overlap = true, limit = EXTMARK_OVERLAP_PROBE_LIMIT }",
                    "if #extmarks > EXTMARK_OVERLAP_TRUSTED_LIMIT then",
                    "priority = math.huge",
                ],
                &[
                    "{ details = true, overlap = true }",
                    "if #extmarks >= EXTMARK_OVERLAP_PROBE_LIMIT then",
                ],
            ),
            (
                "cursor-color extmarks regression harness",
                CURSOR_COLOR_EXTMARKS_TEST_SCRIPT,
                "extmark regression harness uses the single-argument probe contract",
                &[
                    "cursor_color_at_cursor(true)",
                    "test_extmark_probe_stays_bounded_when_overlap_limit_saturates",
                    "test_extmark_probe_trusts_exact_overlap_limit",
                ],
                &[
                    "cursor_color_at_cursor(0, true)",
                    "cursor_color_at_cursor(1, true)",
                ],
            ),
        ];

        for &(script_name, script, case_name, required, forbidden) in cases {
            assert_substring_contract(script_name, script, case_name, required, forbidden);
        }
    }
}

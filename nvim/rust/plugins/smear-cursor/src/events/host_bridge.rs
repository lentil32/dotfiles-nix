use super::EngineAccessError;
use super::HostBridgeRevision;
use super::HostBridgeState;
use super::runtime::mutate_engine_state;
use super::runtime::read_engine_state;
use super::timer_protocol::HostCallbackId;
use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::api;
use thiserror::Error;

const HOST_BRIDGE_REVISION_FUNCTION_NAME: &str = "nvimrs_smear_cursor#host_bridge#revision";
pub(super) const DISPATCH_AUTOCMD_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#dispatch_autocmd";
#[cfg(test)]
const DISPATCH_TIMER_FUNCTION_NAME: &str = "nvimrs_smear_cursor#host_bridge#dispatch_timer";
const START_TIMER_ONCE_FUNCTION_NAME: &str = "nvimrs_smear_cursor#host_bridge#start_timer_once";
const STOP_TIMER_FUNCTION_NAME: &str = "nvimrs_smear_cursor#host_bridge#stop_timer";
const INSTALL_PROBE_HELPERS_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#install_probe_helpers";
const CURSOR_COLOR_AT_CURSOR_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#cursor_color_at_cursor";
const BACKGROUND_ALLOWED_MASK_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#background_allowed_mask";
#[cfg(test)]
const HOST_BRIDGE_SCRIPT: &str = include_str!("../../autoload/nvimrs_smear_cursor/host_bridge.vim");
#[cfg(test)]
const PROBE_HELPERS_SCRIPT: &str = include_str!("../../lua/nvimrs_smear_cursor/probes.lua");
#[cfg(test)]
const CURSOR_COLOR_EXTMARKS_TEST_SCRIPT: &str =
    include_str!("../../scripts/test_cursor_color_probe_extmarks.lua");

fn host_bridge_state() -> Result<HostBridgeState, EngineAccessError> {
    read_engine_state(|state| state.shell.host_bridge_state())
}

fn note_host_bridge_verified(revision: HostBridgeRevision) -> Result<(), EngineAccessError> {
    mutate_engine_state(|state| {
        state.shell.note_host_bridge_verified(revision);
    })
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
    #[cfg(not(test))]
    #[error("failed to stop smear cursor host bridge timer: {0}")]
    StopTimer(#[source] nvim_oxi::Error),
    #[error("failed to install smear cursor probe helpers: {0}")]
    InstallProbeHelpers(#[source] nvim_oxi::Error),
    #[error("failed to call smear cursor cursor-color probe: {0}")]
    CursorColorProbe(#[source] nvim_oxi::Error),
    #[error("failed to call smear cursor background-mask probe: {0}")]
    BackgroundAllowedMask(#[source] nvim_oxi::Error),
    #[error("engine state access failed while resolving host bridge state: {0}")]
    EngineAccess(#[from] EngineAccessError),
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
        let args = Array::from_iter([
            Object::from(host_callback_id.get()),
            Object::from(timeout_ms),
        ]);
        api::call_function(START_TIMER_ONCE_FUNCTION_NAME, args)
            .map_err(|error| HostBridgeError::StartTimerOnce(error.into()))
    }

    #[cfg(not(test))]
    pub(super) fn stop_timer(self, timer_id: i64) -> HostBridgeResult<()> {
        let args = Array::from_iter([Object::from(timer_id)]);
        let _: i64 = api::call_function(STOP_TIMER_FUNCTION_NAME, args)
            .map_err(|error| HostBridgeError::StopTimer(error.into()))?;
        Ok(())
    }

    pub(super) fn install_probe_helpers(self) -> HostBridgeResult<()> {
        let _: i64 = api::call_function(INSTALL_PROBE_HELPERS_FUNCTION_NAME, Array::new())
            .map_err(|error| HostBridgeError::InstallProbeHelpers(error.into()))?;
        Ok(())
    }

    pub(super) fn cursor_color_at_cursor(
        self,
        allow_extmark_fallback: bool,
    ) -> HostBridgeResult<Object> {
        let args = Array::from_iter([Object::from(allow_extmark_fallback)]);
        api::call_function(CURSOR_COLOR_AT_CURSOR_FUNCTION_NAME, args)
            .map_err(|error| HostBridgeError::CursorColorProbe(error.into()))
    }

    pub(super) fn background_allowed_mask(self, request: Array) -> HostBridgeResult<Object> {
        let args = Array::from_iter([Object::from(request)]);
        api::call_function(BACKGROUND_ALLOWED_MASK_FUNCTION_NAME, args)
            .map_err(|error| HostBridgeError::BackgroundAllowedMask(error.into()))
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

fn query_host_bridge_revision() -> HostBridgeResult<HostBridgeRevision> {
    let revision: i64 = api::call_function(HOST_BRIDGE_REVISION_FUNCTION_NAME, Array::new())
        .map_err(|error| HostBridgeError::RevisionQuery(error.into()))?;
    let revision =
        u32::try_from(revision).map_err(|_| HostBridgeError::RevisionDecode { value: revision })?;
    Ok(HostBridgeRevision(revision))
}

struct HostBridge;

impl HostBridge {
    fn verify() -> HostBridgeResult<InstalledHostBridge> {
        match installed_host_bridge_from_state() {
            Ok(host_bridge) => {
                host_bridge.install_probe_helpers()?;
                Ok(host_bridge)
            }
            Err(_) => {
                let revision = query_host_bridge_revision()?;
                if revision != HostBridgeRevision::CURRENT {
                    return Err(HostBridgeError::RevisionMismatch {
                        expected: HostBridgeRevision::CURRENT,
                        found: revision,
                    });
                }

                let host_bridge = InstalledHostBridge;
                host_bridge.install_probe_helpers()?;
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

pub(super) fn ensure_namespace_id() -> Result<u32, EngineAccessError> {
    if let Some(namespace_id) = read_engine_state(|state| state.shell.namespace_id())? {
        return Ok(namespace_id);
    }

    let created = api::create_namespace("nvimrs_smear_cursor");
    mutate_engine_state(|state| {
        state.shell.namespace_id().unwrap_or_else(|| {
            state.shell.set_namespace_id(created);
            created
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    type SubstringCase<'a> = (&'a str, &'a str, &'a str, &'a [&'a str], &'a [&'a str]);

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
                "uncapped retry path present",
                &[
                    "local EXTMARK_OVERLAP_PROBE_SOFT_LIMIT = 32",
                    "local function overlapping_extmarks_at_cursor(cursor)",
                    "{ details = true, overlap = true, limit = EXTMARK_OVERLAP_PROBE_SATURATION_LIMIT }",
                    "if #extmarks < EXTMARK_OVERLAP_PROBE_SATURATION_LIMIT then",
                    "{ details = true, overlap = true }",
                ],
                &[],
            ),
            (
                "cursor-color extmarks regression harness",
                CURSOR_COLOR_EXTMARKS_TEST_SCRIPT,
                "extmark regression harness uses the single-argument probe contract",
                &["cursor_color_at_cursor(true)"],
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

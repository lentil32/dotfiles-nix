use super::EngineAccessError;
use super::HostBridgeRevision;
use super::HostBridgeState;
use super::runtime::mutate_engine_state;
use super::runtime::read_engine_state;
use crate::core::types::Generation;
use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::api;
use thiserror::Error;

const HOST_BRIDGE_REVISION_FUNCTION_NAME: &str = "nvimrs_smear_cursor#host_bridge#revision";
const START_TIMER_ONCE_FUNCTION_NAME: &str = "nvimrs_smear_cursor#host_bridge#start_timer_once";
const INSTALL_PROBE_HELPERS_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#install_probe_helpers";
const CURSOR_COLOR_AT_CURSOR_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#cursor_color_at_cursor";
const BACKGROUND_ALLOWED_MASK_FUNCTION_NAME: &str =
    "nvimrs_smear_cursor#host_bridge#background_allowed_mask";
#[cfg(test)]
const CORE_TIMER_CALLBACK_FUNCTION_NAME: &str = "nvimrs_smear_cursor#host_bridge#on_core_timer";
#[cfg(test)]
const HOST_BRIDGE_SCRIPT: &str = include_str!("../../autoload/nvimrs_smear_cursor/host_bridge.vim");
#[cfg(test)]
const PROBE_HELPERS_SCRIPT: &str = include_str!("../../lua/nvimrs_smear_cursor/probes.lua");

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
    #[error("failed to install smear cursor probe helpers: {0}")]
    InstallProbeHelpers(#[source] nvim_oxi::Error),
    #[error("failed to call smear cursor cursor-color probe: {0}")]
    CursorColorProbe(#[source] nvim_oxi::Error),
    #[error("failed to encode smear cursor colorscheme generation for host bridge: {value}")]
    CursorColorGenerationEncode { value: u64 },
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
    pub(super) fn start_timer_once(self, timeout_ms: i64) -> HostBridgeResult<i64> {
        let args = Array::from_iter([Object::from(timeout_ms)]);
        api::call_function(START_TIMER_ONCE_FUNCTION_NAME, args)
            .map_err(|error| HostBridgeError::StartTimerOnce(error.into()))
    }

    pub(super) fn install_probe_helpers(self) -> HostBridgeResult<()> {
        let _: i64 = api::call_function(INSTALL_PROBE_HELPERS_FUNCTION_NAME, Array::new())
            .map_err(|error| HostBridgeError::InstallProbeHelpers(error.into()))?;
        Ok(())
    }

    pub(super) fn cursor_color_at_cursor(
        self,
        colorscheme_generation: Generation,
        allow_extmark_fallback: bool,
    ) -> HostBridgeResult<Object> {
        let generation = i64::try_from(colorscheme_generation.value()).map_err(|_| {
            HostBridgeError::CursorColorGenerationEncode {
                value: colorscheme_generation.value(),
            }
        })?;
        let args = Array::from_iter([
            Object::from(generation),
            Object::from(allow_extmark_fallback),
        ]);
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
                    CORE_TIMER_CALLBACK_FUNCTION_NAME,
                    START_TIMER_ONCE_FUNCTION_NAME,
                    INSTALL_PROBE_HELPERS_FUNCTION_NAME,
                    CURSOR_COLOR_AT_CURSOR_FUNCTION_NAME,
                    BACKGROUND_ALLOWED_MASK_FUNCTION_NAME,
                ],
                &[],
            ),
            (
                "legacy callback surfaces absent",
                &[],
                &[
                    "vim.on_key(",
                    "set_on_key_listener",
                    "v:lua.",
                    "_G.__nvimrs_smear_cursor",
                ],
            ),
            (
                "timer callback shape",
                &[
                    "timer_start(",
                    "function('nvimrs_smear_cursor#host_bridge#on_core_timer')",
                ],
                &[],
            ),
            (
                "runtime-module loading shape",
                &["require('nvimrs_smear_cursor.probes')"],
                &[
                    "CURSOR_COLOR_LUAEVAL_EXPR",
                    "BACKGROUND_ALLOWED_MASK_LUAEVAL_EXPR",
                ],
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
        let cases: &[(&str, &str, &str, &[&str], &[&str])] = &[
            (
                "host bridge script",
                HOST_BRIDGE_SCRIPT,
                "colorscheme generation plumbed through",
                &[
                    "cursor_color_at_cursor(colorscheme_generation, ...) abort",
                    "let allow_extmark_fallback = a:0 > 0 ? a:1 : v:false",
                    ".cursor_color_at_cursor(_A[1], _A[2])",
                ],
                &[],
            ),
            (
                "probe helpers script",
                PROBE_HELPERS_SCRIPT,
                "extmark fallback gate present",
                &[
                    "function M.cursor_color_at_cursor(colorscheme_generation, allow_extmark_fallback)",
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
                "probe helpers script",
                PROBE_HELPERS_SCRIPT,
                "removed highlight-generation cache absent",
                &[],
                &[
                    "hl_color_cache_generation",
                    "reset_hl_color_cache(colorscheme_generation)",
                    "hl_fg_cache[group]",
                ],
            ),
        ];

        for &(script_name, script, case_name, required, forbidden) in cases {
            assert_substring_contract(script_name, script, case_name, required, forbidden);
        }
    }
}

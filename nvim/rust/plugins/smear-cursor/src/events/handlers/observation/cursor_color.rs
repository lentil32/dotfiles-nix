use super::base::current_buffer_changedtick;
use super::base::current_core_cursor_position;
use crate::core::effect::CursorPositionReadPolicy;
use crate::core::effect::ProbePolicy;
use crate::core::effect::RequestProbeEffect;
use crate::core::event::Event as CoreEvent;
use crate::core::event::ProbeReportedEvent;
use crate::core::state::CursorColorProbeWitness;
use crate::core::state::CursorColorSample;
use crate::core::state::ProbeFailure;
use crate::core::state::ProbeReuse;
use crate::core::types::CursorPosition;
use crate::events::cursor::mode_string;
use crate::events::cursor::sampled_cursor_color_at_current_position;
use crate::events::runtime::cached_cursor_color_sample_for_probe;
use crate::events::runtime::cursor_color_colorscheme_generation;
use crate::events::runtime::read_engine_state;
use crate::events::runtime::record_cursor_color_cache_hit;
use crate::events::runtime::record_cursor_color_cache_miss;
use crate::events::runtime::record_cursor_color_probe_reuse;
use crate::events::runtime::store_cursor_color_sample;
use nvim_oxi::Result;
use nvim_oxi::api;

pub(super) fn current_cursor_color_probe_witness(
    mode: &str,
    cursor_position: Option<CursorPosition>,
) -> Result<CursorColorProbeWitness> {
    let buffer = api::get_current_buf();
    if !buffer.is_valid() {
        return Err(nvim_oxi::api::Error::Other("current buffer invalid".into()).into());
    }

    let buffer_handle = i64::from(buffer.handle());
    let changedtick = current_buffer_changedtick(buffer_handle)?;
    let colorscheme_generation = cursor_color_colorscheme_generation()?;
    // cursor-color sampling can also drift via extmarks or semantic-token overlays without a
    // changedtick bump. Keep the cache tied to the cheap shell reads we can afford on every probe
    // edge for now; if telemetry still shows stale reuse, widen the key instead of collapsing the
    // deferred effect boundary back into a synchronous shell read.
    Ok(CursorColorProbeWitness::new(
        buffer_handle,
        changedtick,
        mode.to_owned(),
        cursor_position,
        colorscheme_generation,
    ))
}

pub(super) fn mode_requires_cursor_color_sampling(mode: &str) -> Result<bool> {
    read_engine_state(|state| {
        state
            .core_state()
            .runtime()
            .config
            .requires_cursor_color_sampling_for_mode(mode)
    })
    .map_err(Into::into)
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum CursorColorProbeValidation {
    Exact(CursorColorProbeWitness),
    Compatible(CursorColorProbeWitness),
    RefreshRequired,
}

impl CursorColorProbeValidation {
    const fn reuse(&self) -> ProbeReuse {
        match self {
            Self::Exact(_) => ProbeReuse::Exact,
            Self::Compatible(_) => ProbeReuse::Compatible,
            Self::RefreshRequired => ProbeReuse::RefreshRequired,
        }
    }

    fn cache_witness(&self) -> Option<&CursorColorProbeWitness> {
        match self {
            Self::Exact(witness) | Self::Compatible(witness) => Some(witness),
            Self::RefreshRequired => None,
        }
    }
}

fn validate_cursor_color_probe_witness(
    expected_witness: &CursorColorProbeWitness,
    probe_policy: ProbePolicy,
    current_witness: &CursorColorProbeWitness,
) -> ProbeReuse {
    if expected_witness.mode() != current_witness.mode()
        || expected_witness.buffer_handle() != current_witness.buffer_handle()
        || expected_witness.changedtick() != current_witness.changedtick()
        || expected_witness.colorscheme_generation() != current_witness.colorscheme_generation()
    {
        return ProbeReuse::RefreshRequired;
    }

    if expected_witness.cursor_position() == current_witness.cursor_position() {
        return ProbeReuse::Exact;
    }

    if probe_policy.allows_compatible_cursor_color_reuse()
        && let (Some(expected), Some(current)) = (
            expected_witness.cursor_position(),
            current_witness.cursor_position(),
        )
    {
        return if expected.row == current.row {
            ProbeReuse::Compatible
        } else {
            ProbeReuse::RefreshRequired
        };
    }

    ProbeReuse::RefreshRequired
}

fn cursor_color_probe_validation(
    expected_witness: &CursorColorProbeWitness,
    probe_policy: ProbePolicy,
    current_witness: CursorColorProbeWitness,
) -> CursorColorProbeValidation {
    match validate_cursor_color_probe_witness(expected_witness, probe_policy, &current_witness) {
        ProbeReuse::Exact => CursorColorProbeValidation::Exact(current_witness),
        ProbeReuse::Compatible => CursorColorProbeValidation::Compatible(current_witness),
        ProbeReuse::RefreshRequired => CursorColorProbeValidation::RefreshRequired,
    }
}

fn current_cursor_color_probe_validation(
    expected_witness: &CursorColorProbeWitness,
    policy: CursorPositionReadPolicy,
    probe_policy: ProbePolicy,
) -> Result<CursorColorProbeValidation> {
    let current_mode = mode_string();
    if current_mode != expected_witness.mode() {
        return Ok(CursorColorProbeValidation::RefreshRequired);
    }

    let current_position = current_core_cursor_position(&current_mode, policy, probe_policy)?;
    let current_buffer = api::get_current_buf();
    if !current_buffer.is_valid() {
        return Err(nvim_oxi::api::Error::Other("current buffer invalid".into()).into());
    }

    let current_buffer_handle = i64::from(current_buffer.handle());
    let current_changedtick = current_buffer_changedtick(current_buffer_handle)?;
    let current_colorscheme_generation = cursor_color_colorscheme_generation()?;
    let current_witness = CursorColorProbeWitness::new(
        current_buffer_handle,
        current_changedtick,
        current_mode,
        current_position.position,
        current_colorscheme_generation,
    );
    Ok(cursor_color_probe_validation(
        expected_witness,
        probe_policy,
        current_witness,
    ))
}

fn cursor_color_ready_event(
    payload: &RequestProbeEffect,
    reuse: ProbeReuse,
    sample: Option<CursorColorSample>,
) -> CoreEvent {
    CoreEvent::ProbeReported(ProbeReportedEvent::CursorColorReady {
        observation_id: payload.observation_basis.observation_id(),
        probe_request_id: payload.probe_request_id,
        reuse,
        sample,
    })
}

fn cursor_color_failed_event(payload: &RequestProbeEffect) -> CoreEvent {
    CoreEvent::ProbeReported(ProbeReportedEvent::CursorColorFailed {
        observation_id: payload.observation_basis.observation_id(),
        probe_request_id: payload.probe_request_id,
        failure: ProbeFailure::ShellReadFailed,
    })
}

pub(super) fn collect_cursor_color_report(
    payload: &RequestProbeEffect,
    same_reducer_wave: bool,
) -> CoreEvent {
    let Some(expected_witness) = payload.observation_basis.cursor_color_witness() else {
        crate::events::logging::warn("cursor color probe missing witness");
        return cursor_color_failed_event(payload);
    };
    let probe_policy = payload.probe_policy();
    let validation = if same_reducer_wave {
        CursorColorProbeValidation::Exact(expected_witness.clone())
    } else {
        match current_cursor_color_probe_validation(
            expected_witness,
            payload.cursor_position_policy,
            probe_policy,
        ) {
            Ok(validation) => validation,
            Err(err) => {
                crate::events::logging::warn(&format!(
                    "cursor color probe witness read failed: {err}"
                ));
                return cursor_color_failed_event(payload);
            }
        }
    };
    let reuse = validation.reuse();
    record_cursor_color_probe_reuse(reuse);
    if reuse == ProbeReuse::RefreshRequired {
        return cursor_color_ready_event(payload, ProbeReuse::RefreshRequired, None);
    }
    let Some(cache_witness) = validation.cache_witness() else {
        crate::events::logging::warn("cursor color probe reuse missing cache witness");
        return cursor_color_failed_event(payload);
    };

    match cached_cursor_color_sample_for_probe(cache_witness, probe_policy, reuse) {
        Ok(Some(cached)) => {
            record_cursor_color_cache_hit();
            return cursor_color_ready_event(payload, cached.reuse(), cached.sample());
        }
        Ok(None) => {
            record_cursor_color_cache_miss();
        }
        Err(err) => {
            crate::events::logging::warn(&format!("cursor color cache read failed: {err}"));
            return cursor_color_failed_event(payload);
        }
    }

    if reuse == ProbeReuse::Compatible {
        if let Some(sample) = payload.cursor_color_fallback_sample {
            return cursor_color_ready_event(payload, ProbeReuse::Compatible, Some(sample));
        }
        crate::events::logging::warn("compatible cursor color probe missing fallback sample");
        return cursor_color_ready_event(payload, ProbeReuse::RefreshRequired, None);
    }

    match sampled_cursor_color_at_current_position(
        expected_witness.colorscheme_generation(),
        probe_policy,
    ) {
        Ok(sample) => {
            let sample: Option<CursorColorSample> = sample.map(CursorColorSample::new);
            if let Err(err) = store_cursor_color_sample(cache_witness.clone(), sample) {
                crate::events::logging::warn(&format!("cursor color cache write failed: {err}"));
            }
            cursor_color_ready_event(payload, ProbeReuse::Exact, sample)
        }
        Err(err) => {
            crate::events::logging::warn(&format!("cursor color sampling failed: {err}"));
            cursor_color_failed_event(payload)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CursorColorProbeValidation;
    use super::cursor_color_probe_validation;
    use super::validate_cursor_color_probe_witness;
    use crate::core::effect::CursorColorFallbackMode;
    use crate::core::effect::CursorColorReuseMode;
    use crate::core::effect::CursorPositionProbeMode;
    use crate::core::effect::ProbePolicy;
    use crate::core::effect::ProbeQuality;
    use crate::core::state::CursorColorProbeWitness;
    use crate::core::state::ProbeReuse;
    use crate::core::types::CursorCol;
    use crate::core::types::CursorPosition;
    use crate::core::types::CursorRow;
    use crate::core::types::Generation;
    use pretty_assertions::assert_eq;

    fn cursor(row: u32, col: u32) -> CursorPosition {
        CursorPosition {
            row: CursorRow(row),
            col: CursorCol(col),
        }
    }

    fn witness(
        buffer_handle: i64,
        changedtick: u64,
        mode: &str,
        cursor_position: Option<CursorPosition>,
        colorscheme_generation: u64,
    ) -> CursorColorProbeWitness {
        CursorColorProbeWitness::new(
            buffer_handle,
            changedtick,
            mode.to_string(),
            cursor_position,
            Generation::new(colorscheme_generation),
        )
    }

    #[test]
    fn validate_cursor_color_probe_witness_reuses_captured_snapshot_when_shell_reads_match() {
        let expected = witness(22, 14, "n", Some(cursor(7, 8)), 3);

        assert_eq!(
            validate_cursor_color_probe_witness(
                &expected,
                ProbePolicy::new(ProbeQuality::Exact),
                &witness(22, 14, "n", Some(cursor(7, 8)), 3),
            ),
            ProbeReuse::Exact,
        );
    }

    #[test]
    fn validate_cursor_color_probe_witness_requires_refresh_when_snapshot_goes_stale() {
        let expected = witness(22, 14, "n", Some(cursor(7, 8)), 3);

        assert_eq!(
            validate_cursor_color_probe_witness(
                &expected,
                ProbePolicy::new(ProbeQuality::FastMotion),
                &witness(22, 15, "n", Some(cursor(7, 8)), 3),
            ),
            ProbeReuse::RefreshRequired,
        );
        assert_eq!(
            validate_cursor_color_probe_witness(
                &expected,
                ProbePolicy::new(ProbeQuality::FastMotion),
                &witness(22, 14, "i", Some(cursor(7, 8)), 3),
            ),
            ProbeReuse::RefreshRequired,
        );
    }

    #[test]
    fn validate_cursor_color_probe_witness_returns_compatible_for_same_line_column_drift() {
        let expected = witness(22, 14, "n", Some(cursor(7, 8)), 3);
        let exact_compatible_policy = ProbePolicy::from_modes(
            CursorPositionProbeMode::Exact,
            CursorColorReuseMode::CompatibleWithinLine,
            CursorColorFallbackMode::SyntaxThenExtmarks,
        );

        assert_eq!(
            validate_cursor_color_probe_witness(
                &expected,
                exact_compatible_policy,
                &witness(22, 14, "n", Some(cursor(7, 9)), 3),
            ),
            ProbeReuse::Compatible,
        );
    }

    #[test]
    fn validate_cursor_color_probe_witness_requires_refresh_for_cross_line_motion() {
        let expected = witness(22, 14, "n", Some(cursor(7, 8)), 3);
        let exact_compatible_policy = ProbePolicy::from_modes(
            CursorPositionProbeMode::Exact,
            CursorColorReuseMode::CompatibleWithinLine,
            CursorColorFallbackMode::SyntaxThenExtmarks,
        );

        assert_eq!(
            validate_cursor_color_probe_witness(
                &expected,
                exact_compatible_policy,
                &witness(22, 14, "n", Some(cursor(8, 1)), 3),
            ),
            ProbeReuse::RefreshRequired,
        );
    }

    #[test]
    fn validate_cursor_color_probe_witness_requires_refresh_for_position_drift_in_exact_mode() {
        let expected = witness(22, 14, "n", Some(cursor(7, 8)), 3);

        assert_eq!(
            validate_cursor_color_probe_witness(
                &expected,
                ProbePolicy::new(ProbeQuality::Exact),
                &witness(22, 14, "n", Some(cursor(7, 9)), 3),
            ),
            ProbeReuse::RefreshRequired,
        );
    }

    #[test]
    fn cursor_color_probe_validation_tracks_current_witness_for_same_line_fast_motion_reuse() {
        let expected = witness(22, 14, "n", Some(cursor(7, 8)), 3);
        let current = witness(22, 14, "n", Some(cursor(7, 9)), 3);
        let exact_compatible_policy = ProbePolicy::from_modes(
            CursorPositionProbeMode::Exact,
            CursorColorReuseMode::CompatibleWithinLine,
            CursorColorFallbackMode::SyntaxThenExtmarks,
        );

        assert_eq!(
            cursor_color_probe_validation(&expected, exact_compatible_policy, current.clone()),
            CursorColorProbeValidation::Compatible(current),
        );
    }

    #[test]
    fn cursor_color_probe_validation_requires_refresh_for_cross_line_fast_motion_reuse() {
        let expected = witness(22, 14, "n", Some(cursor(7, 8)), 3);
        let current = witness(22, 14, "n", Some(cursor(8, 1)), 3);
        let exact_compatible_policy = ProbePolicy::from_modes(
            CursorPositionProbeMode::Exact,
            CursorColorReuseMode::CompatibleWithinLine,
            CursorColorFallbackMode::SyntaxThenExtmarks,
        );

        assert_eq!(
            cursor_color_probe_validation(&expected, exact_compatible_policy, current),
            CursorColorProbeValidation::RefreshRequired,
        );
    }
}

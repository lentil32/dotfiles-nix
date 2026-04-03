use super::base::CurrentEditorSnapshot;
use super::base::current_core_cursor_position;
use crate::core::effect::CursorColorFallback;
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
use crate::events::cursor::sampled_cursor_color_at_current_position;
use crate::events::runtime::cached_cursor_color_sample_for_probe;
use crate::events::runtime::cursor_color_cache_generation;
use crate::events::runtime::cursor_color_colorscheme_generation;
use crate::events::runtime::read_engine_state;
use crate::events::runtime::record_cursor_color_cache_hit;
use crate::events::runtime::record_cursor_color_cache_miss;
use crate::events::runtime::record_cursor_color_probe_reuse;
use crate::events::runtime::store_cursor_color_sample;
use nvim_oxi::Result;

pub(super) fn current_cursor_color_probe_witness(
    editor: &CurrentEditorSnapshot,
    cursor_position: Option<CursorPosition>,
) -> Result<CursorColorProbeWitness> {
    let window = editor.current_window()?;
    let buffer = editor.current_buffer()?;
    let mode = editor.mode();
    let window_handle = i64::from(window.handle());
    let buffer_handle = i64::from(buffer.handle());
    let changedtick = editor.current_changedtick()?;
    let colorscheme_generation = cursor_color_colorscheme_generation()?;
    let cache_generation = cursor_color_cache_generation()?;
    // Cursor-color reuse stays observation-scoped until the plugin has a true highlight
    // invalidation signal that covers extmarks, semantic tokens, and ad-hoc highlight writes.
    Ok(CursorColorProbeWitness::new(
        window_handle,
        buffer_handle,
        changedtick,
        mode.into_owned(),
        cursor_position,
        colorscheme_generation,
        cache_generation,
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
        || expected_witness.window_handle() != current_witness.window_handle()
        || expected_witness.buffer_handle() != current_witness.buffer_handle()
        || expected_witness.changedtick() != current_witness.changedtick()
        || expected_witness.colorscheme_generation() != current_witness.colorscheme_generation()
        || expected_witness.cache_generation() != current_witness.cache_generation()
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
        // Compatible reuse is intentionally line-scoped. Column drift can reuse
        // the carried sample, but a row change must request a refresh.
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

fn compatible_cursor_color_fallback_sample(
    fallback: Option<&CursorColorFallback>,
    probe_policy: ProbePolicy,
    current_witness: &CursorColorProbeWitness,
) -> Option<CursorColorSample> {
    let fallback = fallback?;
    match validate_cursor_color_probe_witness(fallback.witness(), probe_policy, current_witness) {
        ProbeReuse::Exact | ProbeReuse::Compatible => Some(fallback.sample()),
        ProbeReuse::RefreshRequired => None,
    }
}

fn current_cursor_color_probe_validation(
    expected_witness: &CursorColorProbeWitness,
    policy: CursorPositionReadPolicy,
    probe_policy: ProbePolicy,
) -> Result<CursorColorProbeValidation> {
    let editor = CurrentEditorSnapshot::capture()?;
    let mode = editor.mode();
    if mode.as_ref() != expected_witness.mode() {
        return Ok(CursorColorProbeValidation::RefreshRequired);
    }

    let current_position = current_core_cursor_position(&editor, policy, probe_policy)?;
    let current_buffer = editor.current_buffer()?;
    let current_window = editor.current_window()?;
    let current_window_handle = i64::from(current_window.handle());
    let current_buffer_handle = i64::from(current_buffer.handle());
    let current_changedtick = editor.current_changedtick()?;
    let current_colorscheme_generation = cursor_color_colorscheme_generation()?;
    let current_cache_generation = cursor_color_cache_generation()?;
    let current_witness = CursorColorProbeWitness::new(
        current_window_handle,
        current_buffer_handle,
        current_changedtick,
        mode.into_owned(),
        current_position.position,
        current_colorscheme_generation,
        current_cache_generation,
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
        // Compatible fallback is safe only when the carried sample still validates against the
        // current observation boundary. A fresh boundary invalidates the runtime color cache, so a
        // carried sample without a matching witness must force an exact refresh instead.
        if let Some(sample) = compatible_cursor_color_fallback_sample(
            payload.cursor_color_fallback.as_ref(),
            probe_policy,
            cache_witness,
        ) {
            return cursor_color_ready_event(payload, ProbeReuse::Compatible, Some(sample));
        }
        crate::events::logging::warn(
            "compatible cursor color probe missing boundary-matching fallback sample",
        );
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
    use super::compatible_cursor_color_fallback_sample;
    use super::cursor_color_probe_validation;
    use super::validate_cursor_color_probe_witness;
    use crate::core::effect::CursorColorFallback;
    use crate::core::effect::CursorColorFallbackMode;
    use crate::core::effect::CursorColorReuseMode;
    use crate::core::effect::CursorPositionProbeMode;
    use crate::core::effect::ProbePolicy;
    use crate::core::effect::ProbeQuality;
    use crate::core::state::CursorColorSample;
    use crate::core::state::ProbeReuse;
    use crate::test_support::cursor;
    use crate::test_support::cursor_color_probe_witness_with_cache_generation as witness_with_cache_generation;
    use pretty_assertions::assert_eq;

    fn exact_compatible_policy() -> ProbePolicy {
        ProbePolicy::from_modes(
            CursorPositionProbeMode::Exact,
            CursorColorReuseMode::CompatibleWithinLine,
            CursorColorFallbackMode::SyntaxThenExtmarks,
        )
    }

    #[test]
    fn cursor_color_probe_validation_smoke_keeps_the_current_witness_for_same_line_reuse() {
        let expected = witness_with_cache_generation(11, 22, 14, "n", Some(cursor(7, 8)), 3, 5);
        let current = witness_with_cache_generation(11, 22, 14, "n", Some(cursor(7, 9)), 3, 5);

        assert_eq!(
            validate_cursor_color_probe_witness(
                &expected,
                ProbePolicy::new(ProbeQuality::Exact),
                &current,
            ),
            ProbeReuse::RefreshRequired,
        );
        assert_eq!(
            cursor_color_probe_validation(&expected, exact_compatible_policy(), current.clone()),
            CursorColorProbeValidation::Compatible(current),
        );
    }

    #[test]
    fn compatible_cursor_color_fallback_sample_smoke_requires_a_matching_boundary() {
        let sample = CursorColorSample::new(42);
        let fallback_witness =
            witness_with_cache_generation(11, 22, 14, "n", Some(cursor(7, 8)), 3, 5);
        let fallback = CursorColorFallback::new(sample, fallback_witness);
        let same_line_current =
            witness_with_cache_generation(11, 22, 14, "n", Some(cursor(7, 9)), 3, 5);
        let stale_boundary_current =
            witness_with_cache_generation(11, 22, 14, "n", Some(cursor(7, 9)), 3, 6);

        assert_eq!(
            compatible_cursor_color_fallback_sample(
                Some(&fallback),
                ProbePolicy::new(ProbeQuality::FastMotion),
                &same_line_current,
            ),
            Some(sample),
        );
        assert_eq!(
            compatible_cursor_color_fallback_sample(
                Some(&fallback),
                ProbePolicy::new(ProbeQuality::FastMotion),
                &stale_boundary_current,
            ),
            None,
        );
        assert_eq!(
            compatible_cursor_color_fallback_sample(
                Some(&fallback),
                ProbePolicy::new(ProbeQuality::Exact),
                &same_line_current,
            ),
            None,
        );
    }
}

use super::*;
use crate::config::DerivedConfigCache;
use crate::config::RuntimeConfig;

#[test]
fn commit_runtime_config_update_rebuilds_derived_config_from_runtime_config() {
    let mut state = RuntimeState::default();
    let initial_revision = state.config_revision;
    let initial_derived_config = state.derived_config.clone();
    state.config.color_levels = 32;
    state.config.hide_target_hack = false;
    state.config.max_kept_windows = 12;

    state.commit_runtime_config_update();

    let expected_derived_config = DerivedConfigCache::new(&state.config);
    let expected_static_config = expected_derived_config.static_render_config();

    assert_ne!(state.config_revision, initial_revision);
    assert_ne!(state.derived_config, initial_derived_config);
    pretty_assertions::assert_eq!(state.derived_config, expected_derived_config);
    pretty_assertions::assert_eq!(
        state.static_render_config().as_ref(),
        &expected_static_config
    );
}

#[test]
fn semantic_view_ignores_derived_config_cache_materialization() {
    let authoritative = RuntimeState::default();
    let mut cache_drifted = authoritative.clone();
    let mismatched_config = RuntimeConfig {
        color_levels: authoritative.config.color_levels.saturating_add(1),
        ..RuntimeConfig::default()
    };
    cache_drifted.derived_config = DerivedConfigCache::new(&mismatched_config);

    pretty_assertions::assert_eq!(cache_drifted.semantic_view(), authoritative.semantic_view());
}

#[test]
fn runtime_preview_change_detection_ignores_derived_config_cache_materialization() {
    let mut runtime = RuntimeState::default();
    let mut preview = RuntimePreview::new(&mut runtime);
    let preview_runtime = preview.runtime_mut();
    let mismatched_config = RuntimeConfig {
        color_levels: preview_runtime.config.color_levels.saturating_add(1),
        ..RuntimeConfig::default()
    };
    preview_runtime.derived_config = DerivedConfigCache::new(&mismatched_config);

    assert!(!preview.runtime_changed_since_preview());
}

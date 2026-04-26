use super::*;
use crate::config::DerivedConfigCache;

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

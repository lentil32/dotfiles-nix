use crate::test_support::options_dict;
use nvim_oxi::Array;
use nvim_oxi::Object;

fn cterm_colors_object(colors: &[i64]) -> Object {
    Object::from(Array::from_iter(colors.iter().copied().map(Object::from)))
}

mod buffer_perf_policy;
mod event_loop_metrics;
mod handler_decisions;
mod options_apply;
mod options_parse;
mod render_cleanup_delay_policy;

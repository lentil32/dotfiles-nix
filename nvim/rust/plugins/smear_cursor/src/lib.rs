mod animation;
mod config;
mod core;
mod draw;
mod events;
mod lua;
mod mutex;
mod octant_chars;
mod state;
mod step;
mod types;

use crate::core::event::EffectFailureSource;
use nvim_oxi::{Dictionary, Function, Result};

fn plugin_error(message: impl Into<String>) -> nvim_oxi::Error {
    nvim_oxi::api::Error::Other(message.into()).into()
}

fn guard_plugin_call<T>(
    function_name: &'static str,
    callback: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let Ok(result) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)) else {
        events::record_effect_failure(EffectFailureSource::PluginEntry, function_name);
        return Err(plugin_error(format!(
            "rs_smear_cursor.{function_name} panicked"
        )));
    };
    result
}

#[nvim_oxi::plugin]
fn rs_smear_cursor() -> Dictionary {
    let _ = core::reducer::phase0_smoke_fingerprint as fn() -> u64;

    let mut api = Dictionary::new();
    api.insert("ping", Function::<(), i64>::from_fn(|()| 1_i64));
    api.insert(
        "echo",
        Function::<Dictionary, Dictionary>::from_fn(|args| -> Result<Dictionary> { Ok(args) }),
    );
    api.insert(
        "step",
        Function::<Dictionary, Dictionary>::from_fn(|args| {
            guard_plugin_call("step", || step::step(args))
        }),
    );
    api.insert(
        "setup",
        Function::<Option<Dictionary>, ()>::from_fn(|opts| {
            guard_plugin_call("setup", || {
                let opts = opts.unwrap_or_else(Dictionary::new);
                events::setup(&opts)
            })
        }),
    );
    api.insert(
        "on_key",
        Function::<(), ()>::from_fn(|()| {
            guard_plugin_call("on_key", || {
                events::on_key_listener_event();
                Ok(())
            })
        }),
    );
    api.insert(
        "on_core_timer",
        Function::<i64, ()>::from_fn(|timer_id| {
            guard_plugin_call("on_core_timer", || {
                events::on_core_timer_event(timer_id);
                Ok(())
            })
        }),
    );
    api.insert(
        "on_autocmd",
        Function::<String, ()>::from_fn(|event| {
            guard_plugin_call("on_autocmd", || events::on_autocmd_event(&event))
        }),
    );
    api.insert(
        "toggle",
        Function::<Option<Dictionary>, ()>::from_fn(|_| {
            guard_plugin_call("toggle", events::toggle)
        }),
    );
    api.insert(
        "diagnostics",
        Function::<(), String>::from_fn(|()| guard_plugin_call("diagnostics", events::diagnostics)),
    );
    api
}

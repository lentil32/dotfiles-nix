//! Lua-facing entrypoints for the `nvimrs_smear_cursor` Neovim plugin.
//!
//! The Rust surface stays intentionally small: setup and event callbacks forward
//! into the state-machine runtime, while `step` remains available as a
//! deterministic particle-simulation harness for perf tooling and benchmarks.
//!
//! ```lua
//! local smear = require("nvimrs_smear_cursor")
//! -- `setup()` installs the Rust autocmd callbacks.
//! smear.setup({ enabled = true, fps = 120 })
//! ```

mod allocation_counters;
mod animation;
mod config;
mod core;
mod draw;
mod events;
mod lua;
#[cfg(test)]
mod mutex;
mod octant_chars;
mod state;
mod step;
#[cfg(test)]
mod test_support;
mod types;

use crate::core::event::EffectFailureSource;
use nvim_oxi::Dictionary;
use nvim_oxi::Function;
use nvim_oxi::Result;

#[cfg(feature = "perf-counters")]
#[global_allocator]
static GLOBAL_ALLOCATOR: allocation_counters::CountingAllocator =
    allocation_counters::CountingAllocator;

pub(crate) fn other_error(message: impl Into<String>) -> nvim_oxi::Error {
    nvim_oxi::api::Error::Other(message.into()).into()
}

fn guard_plugin_call<T>(
    function_name: &'static str,
    callback: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let Ok(result) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)) else {
        events::record_effect_failure(EffectFailureSource::PluginEntry, function_name);
        return Err(other_error(format!(
            "nvimrs_smear_cursor.{function_name} panicked"
        )));
    };
    result
}

#[nvim_oxi::plugin]
/// Registers the Lua-facing plugin functions exported to Neovim.
fn nvimrs_smear_cursor() -> Dictionary {
    allocation_counters::configure_from_env();

    let mut api = Dictionary::new();
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
        "on_core_timer",
        Function::<i64, ()>::from_fn(|timer_id| {
            guard_plugin_call("on_core_timer", || {
                events::on_core_timer_event(timer_id);
                Ok(())
            })
        }),
    );
    api.insert(
        "on_core_timer_slot",
        Function::<(i64, u64), ()>::from_fn(|(timer_id, generation)| {
            guard_plugin_call("on_core_timer_slot", || {
                events::on_core_timer_slot_event(timer_id, generation);
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
        Function::<(), String>::from_fn(|()| {
            guard_plugin_call("diagnostics", || Ok(events::diagnostics()))
        }),
    );
    api.insert(
        "validation_counters",
        Function::<(), String>::from_fn(|()| {
            guard_plugin_call("validation_counters", || Ok(events::validation_counters()))
        }),
    );
    api
}

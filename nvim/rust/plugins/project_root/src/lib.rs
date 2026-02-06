mod buffer;
mod config;
mod core;
mod plugin;
mod types;

use nvim_oxi::Dictionary;

#[nvim_oxi::plugin]
fn rs_project_root() -> Dictionary {
    plugin::build_api()
}

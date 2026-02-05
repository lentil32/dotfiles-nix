mod buffer;
mod config;
mod plugin;

use nvim_oxi::Dictionary;

#[nvim_oxi::plugin]
fn project_root() -> Dictionary {
    plugin::build_api()
}

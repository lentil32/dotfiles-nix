mod args;
pub mod core;
mod picker;

use nvim_oxi::{Dictionary, Function};

#[nvim_oxi::plugin]
fn rs_theme_switcher() -> Dictionary {
    let mut api = Dictionary::new();
    api.insert(
        "open",
        Function::<Dictionary, ()>::from_fn(|args| picker::open(&args)),
    );
    api.insert(
        "cycle_next",
        Function::<Dictionary, ()>::from_fn(|args| picker::cycle_next(&args)),
    );
    api.insert(
        "cycle_prev",
        Function::<Dictionary, ()>::from_fn(|args| picker::cycle_prev(&args)),
    );
    api.insert(
        "move_next",
        Function::<(), ()>::from_fn(|()| picker::move_next()),
    );
    api.insert(
        "move_prev",
        Function::<(), ()>::from_fn(|()| picker::move_prev()),
    );
    api.insert(
        "confirm",
        Function::<(), ()>::from_fn(|()| picker::confirm()),
    );
    api.insert("cancel", Function::<(), ()>::from_fn(|()| picker::cancel()));
    api.insert("close", Function::<(), ()>::from_fn(|()| picker::close()));
    api
}

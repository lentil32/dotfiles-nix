mod args;
mod bridge;
mod preview;
mod reducer;
mod state;

use nvim_oxi::{Dictionary, Function};

const LOG_CONTEXT: &str = "rs_snacks_preview";

#[nvim_oxi::plugin]
fn rs_snacks_preview() -> Dictionary {
    let mut api = Dictionary::new();
    api.insert(
        "on_doc_find",
        Function::<Dictionary, ()>::from_fn(|args| preview::on_doc_find(&args)),
    );
    api.insert(
        "attach_doc_preview",
        Function::<Dictionary, ()>::from_fn(|args| preview::attach_doc_preview_lua(&args)),
    );
    api.insert(
        "close_doc_preview",
        Function::<i64, ()>::from_fn(preview::close_doc_preview_lua),
    );
    api.insert(
        "reset_state",
        Function::<(), ()>::from_fn(|()| preview::reset_state_lua()),
    );
    api
}

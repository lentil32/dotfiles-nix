mod animation;
mod config;
mod draw;
mod events;
mod lua;
mod octant_chars;
mod step;
mod types;

use nvim_oxi::{Dictionary, Function, Result};

#[nvim_oxi::plugin]
fn rs_smear_cursor() -> Dictionary {
    let mut api = Dictionary::new();
    api.insert("ping", Function::<(), i64>::from_fn(|()| 1_i64));
    api.insert(
        "echo",
        Function::<Dictionary, Dictionary>::from_fn(|args| -> Result<Dictionary> { Ok(args) }),
    );
    api.insert(
        "step",
        Function::<Dictionary, Dictionary>::from_fn(step::step),
    );
    api.insert(
        "setup",
        Function::<Option<Dictionary>, ()>::from_fn(|opts| {
            events::setup(opts.unwrap_or_else(Dictionary::new))
        }),
    );
    api.insert(
        "on_key",
        Function::<(), ()>::from_fn(|()| events::on_key_event()),
    );
    api.insert(
        "toggle",
        Function::<Option<Dictionary>, ()>::from_fn(|_| events::toggle()),
    );
    api
}

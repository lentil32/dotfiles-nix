use std::path::Path;

use nvim_oxi::Dictionary;
use nvim_oxi::Result;
use nvim_oxi::String as NvimString;
use nvim_oxi::api;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::CreateAugroupOpts;
use nvim_oxi::api::opts::CreateAutocmdOpts;
use nvim_oxi::api::opts::OptionOpts;
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::schedule;
use nvimrs_nvim_oxi_utils::guard;
use nvimrs_nvim_oxi_utils::handles::BufHandle;
use nvimrs_nvim_oxi_utils::handles::WinHandle;
use nvimrs_nvim_oxi_utils::notify;

use crate::LOG_CONTEXT;
use crate::args::AttachDocPreviewArgs;
use crate::args::DocFindArgs;
use crate::bridge::filetype_for_path;
use crate::bridge::is_doc_preview_filetype;
use crate::bridge::reset_preview_state;
use crate::bridge::snacks_close_preview;
use crate::bridge::snacks_doc_find;
use crate::bridge::snacks_has_doc_preview;
use crate::bridge::snacks_open_preview;
use crate::reducer::PreviewCommand;
use crate::reducer::PreviewEffect;
use crate::reducer::PreviewEvent;
use crate::reducer::PreviewToken;
use crate::reducer::PreviewTransition;
use crate::reducer::RestoreNamePlan;
use crate::state::buf_key;
use crate::state::context;
use crate::state::win_key;

fn report_panic(label: &str, info: &guard::PanicInfo) {
    notify::error(LOG_CONTEXT, &format!("{label} panic: {}", info.render()));
}

fn get_buf_filetype(buf: &Buffer) -> String {
    let opt_opts = OptionOpts::builder().buf(buf.clone()).build();
    match api::get_option_value::<NvimString>("filetype", &opt_opts) {
        Ok(value) => value.to_string_lossy().into_owned(),
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("get filetype failed: {err}"));
            String::new()
        }
    }
}

fn set_buf_filetype(buf: &Buffer, ft: &str) -> Result<()> {
    let opt_opts = OptionOpts::builder().buf(buf.clone()).build();
    api::set_option_value("filetype", ft, &opt_opts)?;
    Ok(())
}

fn restore_doc_preview_name(buf_handle: BufHandle, plan: &RestoreNamePlan) {
    let Some(buf) = buf_handle.valid_buffer() else {
        return;
    };
    let Ok(name) = buf.get_name() else {
        notify::warn(
            LOG_CONTEXT,
            "restore preview name failed to read buffer name",
        );
        return;
    };
    if name.to_string_lossy() == plan.preview_name {
        let mut buf = buf;
        if let Err(err) = buf.set_name(Path::new(&plan.name)) {
            notify::warn(LOG_CONTEXT, &format!("restore preview name failed: {err}"));
        }
    }
}

fn run_preview_cleanup(cleanup_id: i64) {
    if let Err(err) = snacks_close_preview(cleanup_id) {
        notify::warn(LOG_CONTEXT, &format!("preview cleanup failed: {err}"));
    }
}

fn execute_effect(effect: PreviewEffect) {
    match effect {
        PreviewEffect::RestoreName { key, plan } => {
            let Some(buf_handle) = BufHandle::try_from_i64(key.raw()) else {
                return;
            };
            restore_doc_preview_name(buf_handle, &plan);
        }
        PreviewEffect::DeleteAugroup(group) => {
            if let Err(err) = api::del_augroup_by_id(group) {
                notify::warn(LOG_CONTEXT, &format!("delete augroup failed: {err}"));
            }
        }
        PreviewEffect::CloseCleanup(cleanup_id) => run_preview_cleanup(cleanup_id),
    }
}

fn execute_effects(effects: Vec<PreviewEffect>) {
    for effect in effects {
        execute_effect(effect);
    }
}

fn execute_transition(transition: PreviewTransition) -> Option<PreviewCommand> {
    execute_effects(transition.effects);
    transition.command
}

fn log_unexpected_command(context: &str, command: Option<&PreviewCommand>) {
    if command.is_some() {
        notify::warn(
            LOG_CONTEXT,
            &format!("unexpected preview command in {context}"),
        );
    }
}

fn close_doc_preview(buf_handle: BufHandle) -> bool {
    let Some(key) = buf_key(buf_handle) else {
        return false;
    };
    let transition = context().apply_event(PreviewEvent::Close { key });
    if transition.is_empty() {
        return false;
    }
    let command = execute_transition(transition);
    log_unexpected_command("close", command.as_ref());
    true
}

fn close_doc_preview_by_token(token: PreviewToken) -> bool {
    let transition = context().apply_event(PreviewEvent::CloseByToken { token });
    if transition.is_empty() {
        return false;
    }
    let command = execute_transition(transition);
    log_unexpected_command("close_by_token", command.as_ref());
    true
}

fn close_doc_preview_for_window(win_handle: WinHandle) -> bool {
    let Some(win) = win_key(win_handle) else {
        return false;
    };
    let Some(token) = context().token_for_win(win) else {
        return false;
    };
    close_doc_preview_by_token(token)
}

fn close_doc_preview_owners(buf_handle: BufHandle, win_handle: WinHandle) {
    let _ = close_doc_preview(buf_handle);
    let _ = close_doc_preview_for_window(win_handle);
}

fn run_scheduled_close_autocmd(label: &'static str, token: PreviewToken) {
    guard::with_panic(
        (),
        || {
            let _ = close_doc_preview_by_token(token);
        },
        |info| report_panic(label, &info),
    );
}

fn run_close_autocmd(label: &'static str, token: PreviewToken) -> bool {
    schedule(move |()| run_scheduled_close_autocmd(label, token));
    false
}

fn preview_target_is_current(buf_handle: BufHandle, win_handle: WinHandle) -> bool {
    win_handle
        .valid_window()
        .and_then(|window| window.get_buf().ok())
        .is_some_and(|buf| BufHandle::from_buffer(&buf) == buf_handle)
}

fn attach_doc_preview(buf_handle: BufHandle, path: &str, win_handle: WinHandle) -> Result<()> {
    close_doc_preview_owners(buf_handle, win_handle);

    let Some(buf) = buf_handle.valid_buffer() else {
        return Ok(());
    };

    let ft = filetype_for_path(path)?;
    if !is_doc_preview_filetype(&ft) {
        return Ok(());
    }

    if get_buf_filetype(&buf) != ft
        && let Err(err) = set_buf_filetype(&buf, &ft)
    {
        notify::warn(LOG_CONTEXT, &format!("set filetype failed: {err}"));
    }

    if !snacks_has_doc_preview() {
        return Ok(());
    }

    if !preview_target_is_current(buf_handle, win_handle) {
        return Ok(());
    }

    let Some(key) = buf_key(buf_handle) else {
        return Ok(());
    };
    let Some(win) = win_key(win_handle) else {
        return Ok(());
    };

    let original_name = buf.get_name()?.to_string_lossy().into_owned();
    let preview_name = format!("{path}.snacks-preview");
    let group_name = format!("snacks.doc_preview.{}", buf_handle.raw());
    let group = api::create_augroup(
        &group_name,
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;
    let restore_name_plan = if original_name.is_empty() {
        let mut named_buf = buf.clone();
        match named_buf.set_name(Path::new(&preview_name)) {
            Ok(()) => Some(RestoreNamePlan {
                name: original_name,
                preview_name,
            }),
            Err(err) => {
                notify::warn(LOG_CONTEXT, &format!("set preview name failed: {err}"));
                None
            }
        }
    } else {
        None
    };

    let transition = context().apply_event(PreviewEvent::Register {
        key,
        win,
        group,
        restore_name_plan,
    });
    let Some(PreviewCommand::RequestDocFind(token)) = execute_transition(transition) else {
        notify::warn(
            LOG_CONTEXT,
            "missing doc find token during preview registration",
        );
        let _ = close_doc_preview(buf_handle);
        return Ok(());
    };

    let token_for_buf = token;
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .buffer(buf)
        .callback(move |_args: AutocmdCallbackArgs| {
            run_close_autocmd("doc_preview_buf_close", token_for_buf)
        })
        .build();
    if let Err(err) = api::create_autocmd(["BufWipeout", "BufHidden"], &opts) {
        let _ = close_doc_preview_by_token(token);
        return Err(err.into());
    }

    let token_for_win = token;
    let win_id_str = win_handle.raw().to_string();
    let win_opts = CreateAutocmdOpts::builder()
        .group(group)
        .patterns([win_id_str.as_str()])
        .callback(move |_args: AutocmdCallbackArgs| {
            run_close_autocmd("doc_preview_win_close", token_for_win)
        })
        .build();
    if let Err(err) = api::create_autocmd(["WinClosed"], &win_opts) {
        let _ = close_doc_preview_by_token(token);
        return Err(err.into());
    }

    if let Err(err) = snacks_doc_find(buf_handle, token, win_handle) {
        notify::warn(LOG_CONTEXT, &format!("snacks doc find failed: {err}"));
        let _ = close_doc_preview_by_token(token);
    }

    Ok(())
}

fn create_preview_cleanup(win_handle: WinHandle, src: &str) -> Option<i64> {
    match snacks_open_preview(win_handle, src) {
        Ok(value) => value,
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("snacks open preview failed: {err}"));
            None
        }
    }
}

fn on_doc_find_inner(args: DocFindArgs) {
    let DocFindArgs {
        buf_handle,
        token,
        win_handle,
        img_src,
    } = args;
    let Some(key) = buf_key(buf_handle) else {
        return;
    };
    if !context().is_current_preview_token(key, token) {
        return;
    }
    let arrived_transition = context().apply_event(PreviewEvent::DocFindArrived { key, token });
    let command = execute_transition(arrived_transition);
    log_unexpected_command("doc_find_arrived", command.as_ref());

    let Some(src) = img_src else {
        return;
    };
    let src = src.into_string();

    schedule(move |()| {
        guard::with_panic(
            (),
            || {
                if !context().is_current_preview_token(key, token) {
                    return;
                }
                if !preview_target_is_current(buf_handle, win_handle) {
                    let _ = close_doc_preview_by_token(token);
                    return;
                }
                let Some(cleanup_id) = create_preview_cleanup(win_handle, &src) else {
                    return;
                };
                if !context().is_current_preview_token(key, token) {
                    run_preview_cleanup(cleanup_id);
                    return;
                }
                let cleanup_effects = context().apply_event(PreviewEvent::CleanupOpened {
                    key,
                    token,
                    cleanup_id,
                });
                let command = execute_transition(cleanup_effects);
                log_unexpected_command("cleanup_opened", command.as_ref());
            },
            |info| report_panic("doc_preview_schedule", &info),
        );
    });
}

pub fn on_doc_find(args: &Dictionary) {
    let parsed = match DocFindArgs::parse(args) {
        Ok(parsed) => parsed,
        Err(err) => {
            notify::warn(LOG_CONTEXT, &format!("doc find args invalid: {err}"));
            return;
        }
    };
    match guard::catch_unwind_result(|| on_doc_find_inner(parsed)) {
        Ok(()) => {}
        Err(info) => {
            report_panic("on_doc_find", &info);
        }
    }
}

pub fn attach_doc_preview_lua(args: &Dictionary) {
    let parsed = match AttachDocPreviewArgs::parse(args) {
        Ok(parsed) => parsed,
        Err(err) => {
            notify::warn(
                LOG_CONTEXT,
                &format!("attach doc preview args invalid: {err}"),
            );
            return;
        }
    };
    if let Err(err) = attach_doc_preview(parsed.buf_handle, parsed.path.as_str(), parsed.win_handle)
    {
        notify::warn(LOG_CONTEXT, &format!("attach doc preview failed: {err}"));
    }
}

pub fn close_doc_preview_lua(buf_handle: i64) {
    let Some(buf_handle) = BufHandle::try_from_i64(buf_handle) else {
        return;
    };
    let _ = close_doc_preview(buf_handle);
}

pub fn close_doc_preview_for_window_lua(win_handle: i64) {
    let Some(win_handle) = WinHandle::try_from_i64(win_handle) else {
        return;
    };
    let _ = close_doc_preview_for_window(win_handle);
}

pub fn reset_state_lua() {
    let transition = context().apply_event(PreviewEvent::Reset);
    let command = execute_transition(transition);
    log_unexpected_command("reset", command.as_ref());
    reset_preview_state();
}

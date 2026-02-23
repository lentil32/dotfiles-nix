use std::path::Path;

use nvim_oxi::api;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, OptionOpts};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::{Dictionary, Result, String as NvimString, schedule};
use nvim_oxi_utils::handles::{BufHandle, WinHandle};
use nvim_oxi_utils::{guard, notify};

use crate::LOG_CONTEXT;
use crate::args::{AttachDocPreviewArgs, DocFindArgs};
use crate::bridge::{
    filetype_for_path, is_doc_preview_filetype, reset_preview_state, snacks_close_preview,
    snacks_doc_find, snacks_has_doc_preview, snacks_open_preview,
};
use crate::reducer::{
    PreviewCommand, PreviewEffect, PreviewEvent, PreviewToken, PreviewTransition, RestoreNamePlan,
};
use crate::state::{buf_key, context, win_key};

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

fn execute_effect(buf_handle: BufHandle, effect: PreviewEffect) {
    match effect {
        PreviewEffect::RestoreName(plan) => restore_doc_preview_name(buf_handle, &plan),
        PreviewEffect::DeleteAugroup(group) => {
            if let Err(err) = api::del_augroup_by_id(group) {
                notify::warn(LOG_CONTEXT, &format!("delete augroup failed: {err}"));
            }
        }
        PreviewEffect::CloseCleanup(cleanup_id) => run_preview_cleanup(cleanup_id),
    }
}

fn execute_effects(buf_handle: BufHandle, effects: Vec<PreviewEffect>) {
    for effect in effects {
        execute_effect(buf_handle, effect);
    }
}

fn execute_transition(
    buf_handle: BufHandle,
    transition: PreviewTransition,
) -> Option<PreviewCommand> {
    execute_effects(buf_handle, transition.effects);
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
    let command = execute_transition(buf_handle, transition);
    log_unexpected_command("close", command.as_ref());
    true
}

fn close_doc_preview_by_token(buf_handle: BufHandle, token: PreviewToken) -> bool {
    let transition = context().apply_event(PreviewEvent::CloseByToken { token });
    if transition.is_empty() {
        return false;
    }
    let command = execute_transition(buf_handle, transition);
    log_unexpected_command("close_by_token", command.as_ref());
    true
}

fn close_doc_preview_for_window(buf_handle: BufHandle, win_handle: WinHandle) -> bool {
    let Some(win) = win_key(win_handle) else {
        return false;
    };
    let Some(token) = context().token_for_win(win) else {
        return false;
    };
    close_doc_preview_by_token(buf_handle, token)
}

fn close_preview_or_delete_group(buf_handle: BufHandle, group: u32) {
    if !close_doc_preview(buf_handle)
        && let Err(err) = api::del_augroup_by_id(group)
    {
        notify::warn(LOG_CONTEXT, &format!("delete augroup failed: {err}"));
    }
}

fn run_close_autocmd(label: &'static str, buf_handle: BufHandle, group: u32) -> bool {
    guard::with_panic(
        false,
        || {
            close_preview_or_delete_group(buf_handle, group);
            false
        },
        |info| report_panic(label, &info),
    )
}

fn attach_doc_preview(buf_handle: BufHandle, path: &str, win_handle: WinHandle) -> Result<()> {
    let Some(buf) = buf_handle.valid_buffer() else {
        return Ok(());
    };

    let ft = filetype_for_path(path)?;
    if !is_doc_preview_filetype(&ft) {
        let _ = close_doc_preview_for_window(buf_handle, win_handle);
        return Ok(());
    }

    if get_buf_filetype(&buf) != ft
        && let Err(err) = set_buf_filetype(&buf, &ft)
    {
        notify::warn(LOG_CONTEXT, &format!("set filetype failed: {err}"));
    }

    let _ = close_doc_preview_for_window(buf_handle, win_handle);

    if !snacks_has_doc_preview() {
        return Ok(());
    }

    if win_handle.valid_window().is_none() {
        return Ok(());
    }

    let group_name = format!("snacks.doc_preview.{}", buf_handle.raw());
    let group = api::create_augroup(
        &group_name,
        &CreateAugroupOpts::builder().clear(true).build(),
    )?;

    let buf_handle_for_event = buf_handle;
    let group_for_event = group;
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .buffer(buf.clone())
        .callback(move |_args: AutocmdCallbackArgs| {
            run_close_autocmd(
                "doc_preview_buf_close",
                buf_handle_for_event,
                group_for_event,
            )
        })
        .build();
    api::create_autocmd(["BufWipeout", "BufHidden"], &opts)?;

    let buf_handle_for_win = buf_handle;
    let group_for_win = group;
    let win_id_str = win_handle.raw().to_string();
    let win_opts = CreateAutocmdOpts::builder()
        .group(group)
        .patterns([win_id_str.as_str()])
        .callback(move |_args: AutocmdCallbackArgs| {
            run_close_autocmd("doc_preview_win_close", buf_handle_for_win, group_for_win)
        })
        .build();
    api::create_autocmd(["WinClosed"], &win_opts)?;

    let original_name = buf.get_name()?.to_string_lossy().into_owned();
    let preview_name = format!("{path}.snacks-preview");
    let restore_name_plan = (original_name != preview_name).then(|| RestoreNamePlan {
        name: original_name,
        preview_name: preview_name.clone(),
    });
    if restore_name_plan.is_some() {
        let mut buf = buf;
        if let Err(err) = buf.set_name(Path::new(&preview_name)) {
            notify::warn(LOG_CONTEXT, &format!("set preview name failed: {err}"));
        }
    }

    let Some(key) = buf_key(buf_handle) else {
        return Ok(());
    };
    let Some(win) = win_key(win_handle) else {
        return Ok(());
    };
    let transition = context().apply_event(PreviewEvent::Register {
        key,
        win,
        group,
        restore_name_plan,
    });
    let Some(PreviewCommand::RequestDocFind(token)) = execute_transition(buf_handle, transition)
    else {
        notify::warn(
            LOG_CONTEXT,
            "missing doc find token during preview registration",
        );
        let _ = close_doc_preview(buf_handle);
        return Ok(());
    };
    if let Err(err) = snacks_doc_find(buf_handle, token, win_handle) {
        notify::warn(LOG_CONTEXT, &format!("snacks doc find failed: {err}"));
        let _ = close_doc_preview(buf_handle);
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
    let arrived_transition = context().apply_event(PreviewEvent::DocFindArrived { key, token });
    if arrived_transition.is_empty() {
        return;
    }
    let command = execute_transition(buf_handle, arrived_transition);
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
                if win_handle.valid_window().is_none() {
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
                let command = execute_transition(buf_handle, cleanup_effects);
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

pub fn reset_state_lua() {
    reset_preview_state();
}

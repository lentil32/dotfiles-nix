use std::path::Path;

use nvim_oxi::api;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::{CreateAugroupOpts, CreateAutocmdOpts, OptionOpts};
use nvim_oxi::api::types::AutocmdCallbackArgs;
use nvim_oxi::{Dictionary, Result, String as NvimString, schedule};
use nvim_oxi_utils::handles::{BufHandle, WinHandle};
use nvim_oxi_utils::{guard, notify};
use snacks_preview_core::DocPreviewState;

use crate::LOG_CONTEXT;
use crate::args::{AttachDocPreviewArgs, DocFindArgs};
use crate::bridge::{
    filetype_for_path, is_doc_preview_filetype, snacks_close_preview, snacks_doc_find,
    snacks_has_doc_preview, snacks_open_preview,
};
use crate::state::{buf_key, state_lock, state_ok};

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

fn restore_doc_preview_name(buf_handle: BufHandle, state: &DocPreviewState) {
    if !state.restore_name {
        return;
    }
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
    if name.to_string_lossy() == state.preview_name {
        let mut buf = buf;
        if let Err(err) = buf.set_name(Path::new(&state.name)) {
            notify::warn(LOG_CONTEXT, &format!("restore preview name failed: {err}"));
        }
    }
}

fn close_doc_preview(buf_handle: BufHandle) {
    let Some(key) = buf_key(buf_handle) else {
        return;
    };
    let state = {
        let mut state = state_lock();
        state.registry.take_preview(key)
    };

    let Some(mut state) = state else {
        return;
    };

    restore_doc_preview_name(buf_handle, &state);

    if let Some(group) = state.group
        && let Err(err) = api::del_augroup_by_id(group)
    {
        notify::warn(LOG_CONTEXT, &format!("delete augroup failed: {err}"));
    }

    if let Some(cleanup_id) = state.cleanup.take()
        && let Err(err) = snacks_close_preview(cleanup_id)
    {
        notify::warn(LOG_CONTEXT, &format!("preview cleanup failed: {err}"));
    }
}

fn attach_doc_preview(buf_handle: BufHandle, path: &str, win_handle: WinHandle) -> Result<()> {
    let Some(buf) = buf_handle.valid_buffer() else {
        return Ok(());
    };

    let ft = filetype_for_path(path)?;
    if !is_doc_preview_filetype(&ft) {
        close_doc_preview(buf_handle);
        return Ok(());
    }

    if get_buf_filetype(&buf) != ft
        && let Err(err) = set_buf_filetype(&buf, &ft)
    {
        notify::warn(LOG_CONTEXT, &format!("set filetype failed: {err}"));
    }

    close_doc_preview(buf_handle);

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
    let opts = CreateAutocmdOpts::builder()
        .group(group)
        .buffer(buf.clone())
        .callback(move |_args: AutocmdCallbackArgs| {
            guard::with_panic(
                false,
                || {
                    close_doc_preview(buf_handle_for_event);
                    false
                },
                |info| report_panic("doc_preview_buf_close", &info),
            )
        })
        .build();
    api::create_autocmd(["BufWipeout", "BufHidden"], &opts)?;

    let buf_handle_for_win = buf_handle;
    let win_id_str = win_handle.raw().to_string();
    let win_opts = CreateAutocmdOpts::builder()
        .group(group)
        .patterns([win_id_str.as_str()])
        .callback(move |_args: AutocmdCallbackArgs| {
            guard::with_panic(
                false,
                || {
                    close_doc_preview(buf_handle_for_win);
                    false
                },
                |info| report_panic("doc_preview_win_close", &info),
            )
        })
        .build();
    api::create_autocmd(["WinClosed"], &win_opts)?;

    let original_name = buf.get_name()?.to_string_lossy().into_owned();
    let preview_name = format!("{path}.snacks-preview");
    let restore_name = original_name != preview_name;
    if restore_name {
        let mut buf = buf;
        if let Err(err) = buf.set_name(Path::new(&preview_name)) {
            notify::warn(LOG_CONTEXT, &format!("set preview name failed: {err}"));
        }
    }

    let Some(key) = buf_key(buf_handle) else {
        return Ok(());
    };
    let token = {
        let mut state = state_lock();
        let token = state.registry.next_token(key);
        state.registry.insert_preview(
            key,
            DocPreviewState {
                token,
                group: Some(group),
                cleanup: None,
                name: original_name,
                preview_name,
                restore_name,
            },
        );
        token
    };
    if let Err(err) = snacks_doc_find(buf_handle, token, win_handle) {
        notify::warn(LOG_CONTEXT, &format!("snacks doc find failed: {err}"));
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
    if !state_ok(key, token) {
        return;
    }

    if let Some(state) = {
        let state = state_lock();
        state.registry.get_preview(key).cloned()
    } {
        restore_doc_preview_name(buf_handle, &state);
    }

    let Some(src) = img_src else {
        return;
    };
    let src = src.into_string();

    schedule(move |()| {
        guard::with_panic(
            (),
            || {
                if !state_ok(key, token) {
                    return;
                }
                if win_handle.valid_window().is_none() {
                    return;
                }
                let Some(cleanup_id) = create_preview_cleanup(win_handle, &src) else {
                    return;
                };
                let mut cleanup_to_run = Some(cleanup_id);
                {
                    let mut state = state_lock();
                    if let Some(entry) = state.registry.get_preview_mut(key) {
                        entry.cleanup = cleanup_to_run.take();
                    }
                }
                if let Some(cleanup_id) = cleanup_to_run
                    && let Err(err) = snacks_close_preview(cleanup_id)
                {
                    notify::warn(LOG_CONTEXT, &format!("preview cleanup failed: {err}"));
                }
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
    close_doc_preview(buf_handle);
}

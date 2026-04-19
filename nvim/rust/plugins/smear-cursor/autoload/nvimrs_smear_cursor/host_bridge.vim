" Comment: versioned host bridge contract for the Rust smear cursor runtime.

let s:smear_module_expr = "(package.loaded['nvimrs_smear_cursor'] or require('nvimrs_smear_cursor'))"
let s:probe_module_expr = "(package.loaded['nvimrs_smear_cursor.probes'] or require('nvimrs_smear_cursor.probes'))"

function! nvimrs_smear_cursor#host_bridge#revision() abort
  return 15
endfunction

function! nvimrs_smear_cursor#host_bridge#dispatch_autocmd(event, buffer, match) abort
  call luaeval(s:smear_module_expr . ".on_autocmd_payload(_A)", {'event': a:event, 'buffer': a:buffer, 'match': a:match})
  return 0
endfunction

function! nvimrs_smear_cursor#host_bridge#dispatch_timer(host_callback_id, timer_id) abort
  call luaeval(s:smear_module_expr . ".on_core_timer_fired(_A[1], _A[2])", [a:host_callback_id, a:timer_id])
  return a:timer_id
endfunction

function! nvimrs_smear_cursor#host_bridge#start_timer_once(host_callback_id, timeout) abort
  let Callback = function(
        \ 'nvimrs_smear_cursor#host_bridge#dispatch_timer',
        \ [a:host_callback_id]
        \ )
  return timer_start(a:timeout, Callback)
endfunction

function! nvimrs_smear_cursor#host_bridge#stop_timer(timer_id) abort
  return timer_stop(a:timer_id)
endfunction

function! nvimrs_smear_cursor#host_bridge#install_probe_helpers() abort
  call luaeval("require('nvimrs_smear_cursor.probes')")
  return 1
endfunction

function! nvimrs_smear_cursor#host_bridge#cursor_color_at_cursor(allow_extmark_fallback) abort
  return luaeval(s:probe_module_expr . ".cursor_color_at_cursor(_A)", a:allow_extmark_fallback)
endfunction

function! nvimrs_smear_cursor#host_bridge#background_allowed_mask(request) abort
  return luaeval(s:probe_module_expr . ".background_allowed_mask(_A)", a:request)
endfunction

" Comment: versioned host bridge contract for the Rust smear cursor runtime.

function! nvimrs_smear_cursor#host_bridge#revision() abort
  return 8
endfunction

function! nvimrs_smear_cursor#host_bridge#on_core_timer(timer_id) abort
  call luaeval("require('nvimrs_smear_cursor').on_core_timer(_A)", a:timer_id)
endfunction

function! nvimrs_smear_cursor#host_bridge#start_timer_once(timer_slot, token_generation, timeout) abort
  return luaeval(
        \ "(package.loaded['nvimrs_smear_cursor.host_bridge'] or require('nvimrs_smear_cursor.host_bridge')).start_timer_once(_A[1], _A[2], _A[3])",
        \ [a:timer_slot, a:token_generation, a:timeout]
        \ )
endfunction

function! nvimrs_smear_cursor#host_bridge#stop_timer(timer_slot) abort
  return luaeval(
        \ "(package.loaded['nvimrs_smear_cursor.host_bridge'] or require('nvimrs_smear_cursor.host_bridge')).stop_timer(_A)",
        \ a:timer_slot
        \ )
endfunction

function! nvimrs_smear_cursor#host_bridge#install_probe_helpers() abort
  call luaeval("require('nvimrs_smear_cursor.probes')")
  return 1
endfunction

function! nvimrs_smear_cursor#host_bridge#cursor_color_at_cursor(colorscheme_generation, ...) abort
  let allow_extmark_fallback = a:0 > 0 ? a:1 : v:false
  return luaeval(
        \ "(package.loaded['nvimrs_smear_cursor.probes'] or require('nvimrs_smear_cursor.probes')).cursor_color_at_cursor(_A[1], _A[2])",
        \ [a:colorscheme_generation, allow_extmark_fallback]
        \ )
endfunction

function! nvimrs_smear_cursor#host_bridge#background_allowed_mask(request) abort
  return luaeval("(package.loaded['nvimrs_smear_cursor.probes'] or require('nvimrs_smear_cursor.probes')).background_allowed_mask(_A)", a:request)
endfunction

" Comment: versioned host bridge contract for the Rust smear cursor runtime.

function! nvimrs_smear_cursor#host_bridge#revision() abort
  return 7
endfunction

function! nvimrs_smear_cursor#host_bridge#on_core_timer(timer_id) abort
  call luaeval("require('nvimrs_smear_cursor').on_core_timer(_A)", a:timer_id)
endfunction

function! nvimrs_smear_cursor#host_bridge#start_timer_once(timeout) abort
  return timer_start(
        \ a:timeout,
        \ function('nvimrs_smear_cursor#host_bridge#on_core_timer')
        \ )
endfunction

function! nvimrs_smear_cursor#host_bridge#install_probe_helpers() abort
  call luaeval("require('nvimrs_smear_cursor.probes')")
  return 1
endfunction

function! nvimrs_smear_cursor#host_bridge#cursor_color_at_cursor(colorscheme_generation) abort
  return luaeval(
        \ "(package.loaded['nvimrs_smear_cursor.probes'] or require('nvimrs_smear_cursor.probes')).cursor_color_at_cursor(_A)",
        \ a:colorscheme_generation
        \ )
endfunction

function! nvimrs_smear_cursor#host_bridge#background_allowed_mask(request) abort
  return luaeval("(package.loaded['nvimrs_smear_cursor.probes'] or require('nvimrs_smear_cursor.probes')).background_allowed_mask(_A)", a:request)
endfunction

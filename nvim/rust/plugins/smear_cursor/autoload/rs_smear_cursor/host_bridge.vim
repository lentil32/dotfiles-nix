" Comment: versioned host bridge contract for the Rust smear cursor runtime.

function! rs_smear_cursor#host_bridge#revision() abort
  return 2
endfunction

function! rs_smear_cursor#host_bridge#on_core_timer(timer_id) abort
  call luaeval("require('rs_smear_cursor').on_core_timer(_A)", a:timer_id)
endfunction

function! rs_smear_cursor#host_bridge#start_timer_once(timeout) abort
  return timer_start(
        \ a:timeout,
        \ function('rs_smear_cursor#host_bridge#on_core_timer')
        \ )
endfunction

I reviewed the code paths in `smear_cursor`. I couldn’t run the Neovim perf harness in this container because `nvim` isn’t installed, so this is a code-path review rather than a measured profile.

My expected ROI order is: **background probes**, **conceal/cursor probe caching**, **render-planner allocation churn**, then **state-clone / apply-path micro-opts**.

A couple of fast no-code wins first: background sampling only turns on when `particles_enabled && !particles_over_text` in `src/config.rs:134-136`, and cursor-color probing only turns on when `cursor_color` or `cursor_color_insert_mode` is `"none"` in `src/config.rs:129-132`. So `particles_over_text = true` and avoiding `"none"` are the easiest immediate performance levers.

Here’s what I’d change in the code, in priority order:

- **Stop probing the whole viewport for background masking.**
  Right now background progress is row-chunked over the viewport (`src/core/state/observation.rs:546`, `631-647`), Lua does `vim.fn.screenchar(row, col)` for every cell (`lua/nvimrs_smear_cursor/probes.lua:76-95`), and Rust decodes every returned bool object one by one (`src/events/handlers/observation.rs:330-356`). That is expensive on both the Neovim side and the bridge side.
  The render path already deduplicates particle output per screen cell in `src/draw/render/particles.rs:65-80`, so I’d change the background probe to a **sparse probe over the active smear/particle cells** instead of a full-width viewport mask. Even if you keep chunking, return **packed bytes / row bitmasks** rather than `Vec<bool>`. Also, background probes currently disable projection-cache reuse entirely in `src/core/reducer/machine/planning.rs:254-259`; making the probe local or witness-bound would let more projections reuse.

- **Make cursor-color probes cheaper at the wire format and bridge layers.**
  Cursor-color probing currently goes Rust → Vimscript → `luaeval` → Lua (`src/events/host_bridge.rs:84-92`, `autoload/nvimrs_smear_cursor/host_bridge.vim:23-29`), and Lua formats colors as `"#RRGGBB"` strings (`lua/nvimrs_smear_cursor/probes.lua:3-8`, `11-73`), which Rust parses back as strings in `src/events/cursor.rs:555-567`.
  I’d return a **numeric `u32` color** instead of a hex string, and collapse the hot probe path to **one bridge hop**. Also cache `nvim_get_hl()` results by highlight group in Lua for the current colorscheme generation; the Rust side already has colorscheme invalidation hooks in `src/events/probe_cache.rs:156-159`.

- **Fix the cache sizes and cache shape.**
  `CURSOR_COLOR_CACHE_CAPACITY` is only `4`, linearly scanned in a `VecDeque`, and conceal caching stores exactly **one line** (`src/events/probe_cache.rs:6`, `81-85`, `102-154`). That is almost guaranteed to thrash during normal navigation.
  I’d make cursor-color caching a real small LRU, around **16–32 entries**, and change conceal caching to a **multi-line LRU keyed by `(buffer, changedtick, line)`**. Right now a partial conceal miss copies cached regions back to a `Vec`, rescans columns with `synconcealed`, and then still calls `screenpos()` for region boundaries (`src/events/cursor.rs:256-277`, `340-375`, `393-460`). Caching **prefix conceal deltas** or **region-boundary screen cells** would remove a lot of repeated work.

- **Reduce repeated observation reads.**
  `collect_observation_basis()` already reads mode, cursor position, viewport, text context, and cursor-color witness (`src/events/handlers/observation.rs:239-289`), but `collect_cursor_color_report()` immediately re-reads mode, cursor position, and witness before probing (`359-409`).
  Some of that is for correctness, but there is probably room for a fast path: reuse the already-captured snapshot when the probe executes in the same reducer wave, and only revalidate cheap fields when there was an actual queue hop. Also, `current_cursor_text_context()` does `get_lines()` plus string allocation every observation (`93-176`); that wants a small cache keyed by `(buffer_handle, changedtick, cursor_line, tracked_line)`.

- **Add reusable scratch for the render planner and stop rebuilding ordered maps everywhere.**
  `decode_compiled_frame()` rebuilds `cell_candidates`, resamples the centerline, and rebuilds `next_cells` every frame (`src/draw/render_plan/lifecycle.rs:136-183`). `build_cell_candidates()`, `decode_locally()`, and `non_empty_candidates()` allocate fresh `BTreeMap` / `Vec`s each time (`src/draw/render_plan/infra.rs:1136-1194`), and `resample_centerline()` allocates three vectors per call (`src/draw/render_plan/solver.rs:150-234`).
  The compiled-field path already has a good scratch reuse pattern in `src/draw/render/latent_field.rs:895-920`; I’d do the same for planner decode. Internally, I’d strongly consider `HashMap` or dense scratch buffers instead of `BTreeMap` for hot accumulation, then sort only at the edge if deterministic order is required for tests or output.

- **Avoid doing three full swept-occupancy builds per frame sample.**
  `stage_deposited_samples()` calls `deposit_swept_occupancy()` once for each tail band—sheath, core, filament—on every sample (`src/draw/render_plan/solver.rs:1546-1587`). But `deposit_swept_occupancy()` recomputes row intervals, column intervals, nested microtile loops, and allocates a fresh `BTreeMap` every time (`src/draw/render/latent_field.rs:665-753`).
  That should become a two-phase path: **compute sweep geometry once**, then materialize the three bands from shared coverage data. Even a partial refactor that shares intervals and output buffers across bands should help a lot.

- **Cut whole-state clones in the reducer path.**
  `EngineState::core_state()` clones `CoreState` (`src/events.rs:135-138`), `runtime::core_state()` exposes that clone (`src/events/runtime.rs:664-665`), and `dispatch_core_event()` clones again for every reducer call (`src/events/handlers/core_dispatch.rs:365-389`). `CoreState` includes scene and realization payloads in `src/core/state/protocol.rs:224-237`, so those clones are not free.
  Longer-term, I’d move the reducer toward **in-place mutation** or split large payloads behind `Arc`/copy-on-write so hot events stop copying the whole world.

- **Trim apply-path overhead.**
  `prepare_apply_plan()` hashes every span payload every frame (`src/draw/apply.rs:100-107`, `151-170`), then `draw_span()` checks whether the target window already has that payload (`260-318`). I’d move hash generation upstream into realization/projection so the apply phase just reads it.
  Also, `redraw()` does a `luaeval` capability check every time before falling back to `redraw!` (`src/draw/apply.rs:49-67`). Resolve that once during setup and keep a direct fast path.

If I were only going to try **three** changes first, I’d do: **(1) sparse/bitpacked background probes**, **(2) multi-line conceal LRU**, and **(3) shared planner scratch plus single-sweep occupancy generation**.

The repo already has the right validation hooks for this: `scripts/run_perf_window_switch.sh:4-49`, `scripts/compare_particle_probe_perf.sh:4-22`, and per-probe timing around `src/events/runtime.rs:894-899`.

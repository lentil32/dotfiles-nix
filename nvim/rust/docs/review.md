I did a static code review of `plugins/smear_cursor`. I couldn’t run `cargo test` or benchmarks here because `cargo` is not installed in this environment, so this is a source-based review rather than an execution-based one.

Overall: **yes, it is well written**. It is much more disciplined than the average Neovim plugin. I do see **real performance hotspots**, but they are mostly in a few specific host-facing or render-facing paths, not signs of generally sloppy code.

## What looks strong

The architecture is good.

- The reducer/effect split is clean and intentional: `src/core/reducer/mod.rs:1-18` keeps the core transition logic separate from side effects, and `src/events/handlers/core_dispatch.rs:365-389` stages effects after the state transition instead of mixing everything together.
- Reentrancy and panic handling are better than usual for plugin code. Engine state is guarded with an `InUse` slot in `src/events.rs:200-257`, runtime state is restored even across panics in `src/events/runtime.rs:629-643`, and plugin entrypoints are wrapped by `guard_plugin_call` in `src/lib.rs:32-43`.
- The draw/apply side already has useful reuse mechanisms. `src/draw/apply.rs:151-170` prepares span hashes, and `src/draw/apply.rs:260-318` skips `set_extmark` when cached payloads already match. That is exactly the kind of optimization that matters in Neovim host code.
- The project clearly takes performance seriously. You have diagnostics in `src/events/runtime.rs:413-510`, perf scripts under `scripts/`, and internal investigation notes like `investigations/latent-field-compile.md` and `investigations/scheduled-drain-current-state.md`.
- The workspace lint policy is strong. `Cargo.toml:39-70` denies a lot of sloppy patterns, including `unwrap_used` and `redundant_clone`.

My main quality concern is not the architecture; it is **size/complexity**. Several modules are very large, especially `src/events/handlers/core_dispatch.rs`, `src/draw/render_plan/solver.rs`, `src/core/state/observation.rs`, `src/draw/render/latent_field.rs`, `src/draw/mod.rs`, and `src/events/runtime.rs`. That does not make the plugin bad, but it does increase regression risk.

## The main performance issues I see

### 1. Conceal-aware cursor position resolution is probably expensive on long lines

`src/events/cursor.rs:235-320` is the sharpest host-side hotspot I found.

It does this:

- loops from column `1..=cursor_column`
- calls `synconcealed()` for each column
- computes replacement widths
- then calls `screenpos()` again for conceal region boundaries

That is a lot of Neovim function traffic for one cursor read. On long lines, or files using conceal heavily, this can get expensive fast.

### 2. Cursor-color sampling is expensive when enabled

This path is conditional, which is good: it only matters when `cursor_color` or `cursor_color_insert_mode` is `"none"` in `src/config.rs:128-130`.

But when it is enabled, the probe is not cheap:

- `CURSOR_COLOR_LUAEVAL_EXPR` in `src/events.rs:39-110` does syntax lookup, Treesitter capture probing, and extmark inspection.
- It is invoked through `luaeval` in `src/events/cursor.rs:412-425`.
- Cache reuse is exact-match only in `src/events/handlers/observation.rs:379-429`.

One extra detail: the current probe cache is only a **single entry** in `src/events/probe_cache.rs:17-59`. So even modest cursor movement can churn the cache.

### 3. Background probing can be costly for particle mode

Again, this is conditional: only when `particles_enabled && !particles_over_text` in `src/config.rs:133-135`.

The actual probe in `src/events/handlers/observation.rs:313-377` runs a Lua loop calling `screenchar()` over a chunk of cells. You do chunk the work with `MAX_BACKGROUND_PROBE_CELLS_PER_EDGE = 2048` in `src/core/state/observation.rs:546-648`, which is a good mitigation, but this is still the kind of shell-side scan that can show up in latency.

### 4. Latent-field recompilation is the biggest renderer-side hotspot

This matches your own investigation note, and I agree with it.

- `src/draw/render_plan/lifecycle.rs:104-133` recompiles whenever the latent revision changes.
- `src/draw/render/latent_field.rs:369-387` bumps revision on every inserted slice.
- `src/draw/render/latent_field.rs:665-752` rasterizes swept occupancy over the covered cell product.
- `src/draw/render/latent_field.rs:895-920` recompiles from the retained cache every time the compiled revision is stale.

That is the most plausible “plugin feels slow while moving” path in normal use.

### 5. Scheduler slowdown looks like churn, not a broken queue

I do **not** see a catastrophic scheduler algorithm bug.

The queue itself is a `VecDeque` in `src/events/handlers/core_dispatch.rs:204-230`, and the drain logic is bounded in `src/events/handlers/core_dispatch.rs:566-760`. Your note in `investigations/scheduled-drain-current-state.md` is consistent with the code: the issue is repeated hot-drain convergence under churn, not accidental quadratic behavior.

### 6. A few smaller issues

These are secondary, but worth cleaning up:

- Disabled filetype checks are linear scans over a `Vec<String>` in `src/events/runtime.rs:195-203`.
- Logging flushes every line in `src/events/logging.rs:60-99`, which can distort perf when file logging is on.
- Every dispatched event clones `CoreState` through `core_state()` in `src/events/handlers/core_dispatch.rs:365-385` and `src/events/runtime.rs:663-670`. I would **measure** this, not optimize it blindly, because some big pieces are already shared, like `Arc<LogicalRaster>` in `src/core/state/scene.rs:346-356`.

## What I would change first

### Highest priority

1. **Cache conceal information by `(buffer, changedtick, line)`** and skip conceal correction entirely when conceal cannot matter. Right now the per-column `synconcealed()` walk is the most obvious avoidable host cost.
2. **Replace repeated `luaeval` probe strings with setup-installed Lua helpers** using your existing host-bridge approach. That would help both cursor-color and background probes. It reduces parse/bridge overhead and makes those probe paths easier to evolve.
3. **Upgrade cursor-color caching from a single-entry cache to a small LRU** keyed by something like `(buffer, changedtick, line, col, mode, colorscheme_generation)`. `src/events/probe_cache.rs:17-59` is too narrow for real cursor motion patterns.

### Next

4. **Make latent-field compilation incremental or dirty-region based.** That is the biggest likely win in active motion. Even partial dirty-tile recompilation would be valuable.
5. **Keep the current queue model, but tune it adaptively** based on observed backlog/churn rather than only the current thermal snapshots. I would not rewrite the queue structure.
6. **Convert disabled filetypes to a set**. Small win, easy change.

### Maintainability

7. **Split the giant files.** `core_dispatch.rs` especially wants decomposition into queueing, budgeting, draining, metrics, and probe retry logic. This is mostly about keeping future perf regressions from creeping in.

## One thing I would _not_ chase first

The recursive ribbon solver in `src/draw/render_plan/solver.rs:789-889` looks scary at first glance, but it is tightly capped by constants in `src/draw/render_plan/infra.rs:581-583`. I would profile it, but I would not put it ahead of conceal scanning, color probing, or latent-field recompilation.

## Bottom line

**Verdict:** well-written, thoughtful, and robust.

**Performance verdict:** no obvious disaster, but yes, there are meaningful hotspots:

- conceal-aware cursor reads,
- cursor/background host probes,
- latent-field rebuilds during active motion.

**Best enhancements:**
cache conceal work, turn the heavy `luaeval` probes into installed helpers plus better caching, and make latent-field compile more incremental.

The next profiling pass in your repo should focus on those three areas first.

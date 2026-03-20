# Smear Cursor Ribbon State Enumeration Investigation

Scope: render-plan ribbon solve complexity in the current `smear_cursor` implementation.

## What This Note Covers

This note isolates the most combinatorial algorithm in the renderer: the pre-DP slice-state
enumeration in [`solver.rs`](../src/draw/render_plan/solver.rs) and
[`infra.rs`](../src/draw/render_plan/infra.rs).

The key question is not "what is the hottest callback on every frame?" but "which algorithm has
the highest time/space complexity if its inputs approach their configured limits?"

## The Algorithm

The relevant path is:

1. [`build_slice_states`](../src/draw/render_plan/solver.rs#L680) iterates every possible run
   start in one ribbon slice.
2. For each start, it iterates every possible run end up to
   [`RIBBON_MAX_RUN_LENGTH`](../src/draw/render_plan/infra.rs#L562).
3. [`enumerate_run_candidate_states`](../src/draw/render_plan/solver.rs#L720) recursively
   materializes every candidate combination for that run before sorting and truncating.

This work happens before the dynamic-programming pass in
[`solve_ribbon_dp`](../src/draw/render_plan/solver.rs#L906).

## Complexity

Let:

- `S` = number of centerline slices
- `m` = cells in one slice cross-section
- `C` = non-empty candidates per cell
- `L` = maximum run length

Then the transient enumeration cost per slice is:

```text
Time  = O(sum_{r=1..min(L,m)} (m - r + 1) * C^r)
Space = O(sum_{r=1..min(L,m)} (m - r + 1) * C^r)
```

The same order appears in space because the code materializes candidate states into a `Vec` before
sorting and truncating them in [`build_slice_states`](../src/draw/render_plan/solver.rs#L711).

With current caps:

- [`RIBBON_MAX_CROSS_SECTION_CELLS = 12`](../src/draw/render_plan/infra.rs#L561)
- [`RIBBON_MAX_RUN_LENGTH = 4`](../src/draw/render_plan/infra.rs#L562)
- [`RIBBON_MAX_STATES_PER_SLICE = 16`](../src/draw/render_plan/infra.rs#L563)
- [`top_k_per_cell` clamps to `2..=8`](../src/draw/render_plan/solver.rs#L1302), so non-empty
  candidates per cell are at most `7`

The worst-case transient state count per slice is therefore:

```text
1 + 12*7 + 11*7^2 + 10*7^3 + 9*7^4 = 25,663
```

That state explosion is then followed by a sort before truncation.

## Why This Is The Highest-Complexity Algorithm

The later ribbon DP in [`solve_ribbon_dp`](../src/draw/render_plan/solver.rs#L906) is nested but
bounded by the post-truncation state count, so its cost is roughly:

```text
O(S * K^2 * O)
```

Where:

- `K <= 16` states per slice after truncation
- `O` is the overlap-pair count between adjacent slices

That is materially smaller than the exponential pre-truncation enumeration above.

## What Makes It Pathological

This path gets expensive when all of the following happen together:

- slice support stays on the DP path instead of falling back
- each slice keeps many cells
- each cell keeps many non-empty candidates
- the centerline produces many slices

The DP path is skipped once support is oversized in
[`ribbon_support_is_oversized`](../src/draw/render_plan/solver.rs#L508) and
[`select_decode_path`](../src/draw/render_plan/solver.rs#L546), so this issue is specifically the
"still narrow enough for ribbon DP, but rich enough to enumerate many states" case.

## Practical Interpretation

This is the renderer's most time/space-complex algorithm in the strict asymptotic sense.

It is not guaranteed to be the dominant wall-clock cost on every frame, because the current caps
keep it bounded and other linear passes can dominate large active fields. But if the question is
"what code path has the nastiest growth curve?", this is the answer.

## Bottom Line

The single most combinatorial algorithm in `smear_cursor` is the ribbon slice-state enumeration in
[`build_slice_states`](../src/draw/render_plan/solver.rs#L680) and
[`enumerate_run_candidate_states`](../src/draw/render_plan/solver.rs#L720).

Comment: this path is intentionally capped hard enough that "worst asymptotic algorithm" and
"most common real-world slowdown" are not necessarily the same investigation result.

# Cursor Trail Animation Rendering Strategy Specification

Version: 1.0
Status: Normative production specification
Audience: Engine authors, renderer authors, and future maintainers

---

## 1. Purpose

This document defines the animation rendering strategy for a Rust Neovim plugin that renders a cursor trail inside a terminal cell lattice.

The specification is designed to be deterministic, production-oriented, and implementation-ready. It covers:

- motion and path formation
- latent field deposition and compilation
- comet-shaped tail control
- ribbon-aware terminal decode on a terminal grid
- temporal stability rules
- production calibration constants
- tuning order and failure modes

This specification assumes the renderer already uses:

- deterministic state-machine workflow
- pure reducers
- deferred and isolated async effects
- tokenized render planning and apply stages
- a global abstract grid solved before shell-specific window emission

---

## 2. Visual goals

The renderer must prioritize the following visual result:

`bulb -> neck -> filament -> pointed tip`

Required properties:

1. Strong comet read under ordinary editor motion.
2. No ribbon-like same-width tail.
3. Minimal 1-cell re-widening caused by quantization.
4. Stable decoding on terminal glyph lattices.
5. Similar look across 60 Hz and 120 Hz simulation rates.
6. Robust response when `tail_duration_ms` changes.

Non-goals:

- pixel-perfect reconstruction of a continuous ribbon
- preserving backward-compatible heuristics
- exact cursor-footprint width in the far tail

---

## 3. System and terminal constraints

### 3.1 Engine constraints

- reducer state transitions define truth
- reducers are pure
- effects are deferred and isolated by async dispatch
- failure and retry are modeled as normal lifecycle transitions
- stale render tokens are deterministic no-ops

### 3.2 Terminal constraints

- output device is terminal cells, not pixels
- available glyph basis is finite
- shade and color levels are quantized
- cells are anisotropic; `block_aspect_ratio` is part of the rendering metric
- global coherence must be solved before Unicode quantization, not after

### 3.3 Operational perf modes

Adaptive performance remains automatic by default, but production operation may expose one small
top-level override for debugging or forced recovery behavior:

- `auto`: derive the perf class from buffer size, callback slowness, and skip predicates
- `full`: keep the full-quality path for otherwise supported buffers
- `fast`: force the fast-motion policy for otherwise supported buffers
- `off`: skip smear-cursor work entirely

Hard skip predicates such as unsupported special buffers or explicitly disabled filetypes still win
over the non-off override modes.

---

## 4. Coordinate systems and notation

### 4.1 Display metric

All geometric measurements that drive rendering quality must use display-space metric.

For point `(x, y)` in cell coordinates:

```text
x_display = x
y_display = block_aspect_ratio * y
```

Distance in display metric:

```text
metric_distance(a, b) = sqrt((ax - bx)^2 + (block_aspect_ratio * (ay - by))^2)
```

This metric must be used for:

- speed thresholds
- centerline arc length
- curvature
- taper reasoning
- glyph profile fitting where directional bias matters

### 4.2 Tail coordinate

Let `u in [0, 1]` be normalized tail position.

```text
u = 0 -> head (newest)
u = 1 -> tip  (oldest)
```

If centerline samples are stored oldest -> newest, reverse them before assigning `u`.

### 4.3 Helper function

Use this helper wherever a smooth bounded ramp is required:

```text
smooth01(a, b, x):
  if x <= a: 0
  else if x >= b: 1
  else:
    t = (x - a) / (b - a)
    t * t * (3 - 2 * t)
```

---

## 5. End-to-end rendering pipeline

```text
external cursor events + timer events
                |
                v
      pure reducer updates motion/trail truth
                |
                v
      simulation tick advances motion filter
                |
                v
      deposited latent slices are appended
                |
                v
      presentation tick requests RenderPlan(token)
                |
                v
      plan_frame(snapshot) [pure]
          1. compile latent field from banded slice history
          2. build centerline slices
          3. compute target comet widths per slice
          4. generate local glyph candidates
          5. solve global ribbon DP / fallback solver
          6. diff vs previous committed frame
                |
                v
      ApplyRenderPlan(token) [effectful shell]
                |
                v
      Neovim draw operations
```

Normative boundary:

- Everything up to abstract cell-state diffing is pure core math.
- Window allocation, z-indexing, draw application, retries, and shell recovery are effectful shell concerns.

---

## 6. Normative rendering model

### 6.1 Motion model

The cursor head is filtered by a second-order pose model in display metric. The head model defines the centerline used by the trail.

The trail itself is not a deformed quad. It is the compiled result of sparse deposited slices of swept footprint occupancy.

### 6.2 Path segmentation boundary

The renderer must decide whether new motion continues the current stroke or starts a new stroke.

Default policy:

- keep head motion visually continuous when motion is otherwise allowed by the runtime
- start a new trail stroke when the old and new poses are separated by a real disconnect
- let the old tail fade independently instead of synthesizing one continuous bridge across the disconnect

This boundary is about trail history, not about forcing the head to snap.

At minimum, discontinuities must be raised for:

- window changes
- buffer changes
- target-position jumps that would otherwise synthesize an impossible bridge

The best default is therefore:

- inter-window or inter-buffer head motion may remain smooth
- the deposited trail history must restart at the disconnect
- the renderer must not draw one geometric ribbon through split borders, unrelated buffer space, or other non-traversed regions

### 6.3 Multi-band latent deposition

Each simulation step deposits up to three bands:

- `Sheath`
- `Core`
- `Filament`

Each deposited slice stores at minimum:

- stroke id
- step index
- band id
- per-band support step count
- per-band intensity q16
- sparse coverage tiles

Band roles:

- `Sheath`: wide and bright near the head, short-lived
- `Core`: visible body, medium width and lifetime
- `Filament`: narrow persistent tail, longest-lived

#### 6.3.1 Width scales

Effective per-band width is derived from the cursor footprint and band width scale:

```text
band_width = base_width * BAND_WIDTH_SCALE
```

Band width scales define overlap near the head and automatic collapse toward the tail because short-lived wide bands disappear first.

#### 6.3.2 Intensity scales

Base intensity per band is:

```text
band_intensity = BAND_INTENSITY * speed_gain_for_band
```

Only intensity is speed-coupled. Width and support duration remain fixed. This avoids visible breathing.

#### 6.3.3 Support duration scaling

Support durations must scale by real time, not by number of simulation steps.

Normative equations:

```text
duration_ratio = clamp(
  tail_duration_ms / DEFAULT_TAIL_DURATION_MS,
  DURATION_SCALE_MIN,
  DURATION_SCALE_MAX
)

support_scale = duration_ratio ^ DURATION_SCALE_EXPONENT

sheath_support_steps =
  max(SHEATH_MIN_SUPPORT_STEPS,
      round(SHEATH_BASE_LIFETIME_MS * support_scale / step_ms))

core_support_steps =
  max(CORE_MIN_SUPPORT_STEPS,
      round(CORE_BASE_LIFETIME_MS * support_scale / step_ms))

filament_support_steps =
  max(FILAMENT_MIN_SUPPORT_STEPS,
      round(FILAMENT_BASE_LIFETIME_MS * support_scale / step_ms))
```

This keeps the tail visually similar at 60 Hz and 120 Hz.

### 6.4 Speed coupling

Speed coupling must be measured in display-metric cells per second:

```text
speed_cps = metric_distance(start.center, end.center) / max(dt_seconds, 1e-6)
```

Recommended policy:

- couple `Sheath`
- couple `Core` weakly
- do not couple `Filament`

Normative formula:

```text
sheath_gain =
  SPEED_SHEATH_MIN_GAIN
  + (1 - SPEED_SHEATH_MIN_GAIN)
    * smooth01(SPEED_SHEATH_START_CPS, SPEED_SHEATH_FULL_CPS, speed_cps)

core_gain =
  SPEED_CORE_MIN_GAIN
  + (1 - SPEED_CORE_MIN_GAIN)
    * smooth01(SPEED_CORE_START_CPS, SPEED_CORE_FULL_CPS, speed_cps)

filament_gain = 1.0
```

Rationale:

- the bulb should respond to speed
- the body may respond weakly
- the far tail must remain stable and not breathe with velocity

### 6.5 Compile pass decay

Each deposited slice contributes to the compiled latent field while inside its support window.

Let:

```text
a = clamp(normalized_age, 0, 1)
head_weight = 1 - a
s = a * a * (3 - 2 * a)
tail_weight = (1 - s) ^ TAIL_WEIGHT_EXPONENT
```

Then blend weights as:

```text
combined_weight = COMBINED_HEAD_MIX * head_weight + COMBINED_TAIL_MIX * tail_weight
recent_weight   = RECENT_HEAD_MIX   * head_weight + RECENT_TAIL_MIX   * tail_weight
```

Interpretation:

- `combined_weight` drives the persistent tail body
- `recent_weight` keeps the newest slices crisp near the head

The compile pass may store both or may collapse them into a single final weight if the implementation chooses a one-field form.

### 6.6 Comet taper target

For each ribbon slice, define target comet width over normalized tail coordinate `u`.

#### 6.6.1 Base head and tip widths

```text
head_width = clamp(2 * slice_band_half_width(frame), 1.0, RIBBON_MAX_RUN_LENGTH)

tip_width = clamp(
  head_width * COMET_TIP_WIDTH_RATIO,
  COMET_MIN_RESOLVABLE_WIDTH,
  head_width
)
```

#### 6.6.2 Taper progression

Normative taper progression:

```text
v = clamp((u - COMET_NECK_FRACTION) / (1 - COMET_NECK_FRACTION), 0, 1)
s = v * v * (3 - 2 * v)
taper_progress = 1 - (1 - s) ^ COMET_TAPER_EXPONENT
```

This yields:

- a preserved bulb / neck near the head
- faster-than-linear width collapse after the neck
- a longer stable filament toward the tip

#### 6.6.3 Target width

```text
target_width(u) = max(
  COMET_MIN_RESOLVABLE_WIDTH,
  head_width + (tip_width - head_width) * taper_progress
)
```

### 6.7 Tip cap prior

The tip must sharpen harder than the mid-tail.

If `u >= COMET_TIP_ZONE_START`, apply:

```text
tip_width_cap = min(
  target_width(u),
  COMET_MIN_RESOLVABLE_WIDTH * COMET_TIP_CAP_MULTIPLIER
)

E_tip += COMET_TIP_WEIGHT * max(0, width - tip_width_cap)^2
```

### 6.8 Monotonic anti-rewidening prior

The monotonic prior must penalize widening toward the tip.

Let `prev_width` be the slice closer to the head, and `next_width` be the next slice farther toward the tip.

Normative equation:

```text
E_mono += COMET_MONO_WEIGHT
          * max(0, next_width - prev_width - COMET_MONO_EPSILON_CELLS)^2
```

This sign convention is required. The opposite sign penalizes taper itself and is incorrect.

### 6.9 Transverse-width prior

A mild transverse prior may be applied per slice to discourage chunky runs that are wider than visually justified.

Normative guidance:

- this prior should be weaker than taper, monotonicity, and tip cap
- it should trim quantization bulges, not define the overall shape

Example form:

```text
E_transverse += COMET_TRANSVERSE_WEIGHT * age_factor(u) * transverse_excess(width)
```

The exact `age_factor` and `transverse_excess` implementation may vary, but the weight relationship in Section 10 must be preserved.

### 6.10 Curvature compression

Curvature compression should reduce only the width above the minimum resolvable floor.

Compute smoothed curvature in display metric from neighboring centerline samples:

```text
kappa_smoothed = min(0.25 * kappa_prev + 0.50 * kappa + 0.25 * kappa_next,
                     COMET_CURVATURE_KAPPA_CAP)
```

Then apply:

```text
w_excess = max(0, w - COMET_MIN_RESOLVABLE_WIDTH)

w_eff = COMET_MIN_RESOLVABLE_WIDTH
        + w_excess / (1 + COMET_CURVATURE_COMPRESS_FACTOR * kappa_smoothed * w_excess)
```

This keeps bends from turning into sausages while preserving the far-tail filament floor.

### 6.11 Ribbon-constrained decode objective

The production solve is hybrid.

Local candidate generation produces per-cell glyph / shade candidates. A ribbon-constrained dynamic program then operates on ribbon slices and chooses contiguous non-empty runs across those slices. The implementation is not required to perform a full global top-K glyph / shade search over all cells simultaneously.

Ribbon width should be evaluated in projected cross-track span, not raw count of terminal cells touched inside a slice. This avoids axis-aligned slices appearing artificially thicker just because the terminal lattice contributes multiple along-tangent cells at the same normal offset.

Recommended energy decomposition:

```text
E_total = E_unary
        + E_seam
        + E_temporal
        + E_taper
        + E_mono
        + E_tip
        + E_transverse
```

Where:

- `E_unary` = local glyph / shade fit to compiled latent field
- `E_seam` = strip consistency between adjacent slices
- `E_temporal` = resistance to unnecessary frame-to-frame glyph changes
- `E_taper` = target-width prior
- `E_mono` = anti-rewidening prior
- `E_tip` = tip sharpening prior
- `E_transverse` = mild width cleanup prior

### 6.12 Terminal decode and temporal stability

Terminal decode must use a shared glyph / shade codebook abstraction and solve coherence before emission.

Recommended decode properties:

- fit all supported glyph families through one codebook abstraction
- keep top-K local candidates per cell
- solve globally at the ribbon-run level for any connected support that can still be interpreted as one stroke, including thick straight horizontal or vertical bodies
- merge ribbon decisions back into per-cell states after the solve
- reserve pairwise spatial fallback for disconnected support, oversized support, or cases with too little usable slice structure for the ribbon solve
- use previous committed frame only as a weak transition cost, not as the main source of temporal memory

Temporal stability should mostly come from:

1. continuous latent field persistence
2. strong monotonic and tip priors
3. small discrete transition costs

---

## 7. Production profile

The following production profile is the recommended default.

### 7.1 ProductionProfile

| Constant | Value |
|---|---:|
| SHEATH_BASE_LIFETIME_MS | 40.0 |
| CORE_BASE_LIFETIME_MS | 112.0 |
| FILAMENT_BASE_LIFETIME_MS | 252.0 |
| SHEATH_WIDTH_SCALE | 1.18 |
| CORE_WIDTH_SCALE | 0.58 |
| FILAMENT_WIDTH_SCALE | 0.28 |
| SHEATH_INTENSITY | 0.90 |
| CORE_INTENSITY | 0.80 |
| FILAMENT_INTENSITY | 0.78 |
| DEFAULT_TAIL_DURATION_MS | 198.0 |
| DURATION_SCALE_MIN | 0.40 |
| DURATION_SCALE_MAX | 2.50 |
| DURATION_SCALE_EXPONENT | 0.85 |
| SHEATH_MIN_SUPPORT_STEPS | 2 |
| CORE_MIN_SUPPORT_STEPS | 4 |
| FILAMENT_MIN_SUPPORT_STEPS | 7 |
| SPEED_SHEATH_START_CPS | 7.0 |
| SPEED_SHEATH_FULL_CPS | 28.0 |
| SPEED_SHEATH_MIN_GAIN | 0.08 |
| SPEED_CORE_START_CPS | 10.0 |
| SPEED_CORE_FULL_CPS | 34.0 |
| SPEED_CORE_MIN_GAIN | 0.78 |
| TAIL_WEIGHT_EXPONENT | 0.90 |
| COMBINED_HEAD_MIX | 0.20 |
| COMBINED_TAIL_MIX | 0.80 |
| RECENT_HEAD_MIX | 0.82 |
| RECENT_TAIL_MIX | 0.18 |
| COMET_NECK_FRACTION | 0.14 |
| COMET_TIP_WIDTH_RATIO | 0.26 |
| COMET_TAPER_EXPONENT | 1.90 |
| COMET_MIN_RESOLVABLE_WIDTH | 0.26 |
| COMET_TIP_ZONE_START | 0.80 |
| COMET_TIP_CAP_MULTIPLIER | 1.00 |
| COMET_MONO_EPSILON_CELLS | 0.10 |
| COMET_CURVATURE_COMPRESS_FACTOR | 1.05 |
| COMET_CURVATURE_KAPPA_CAP | 1.25 |
| COMET_TAPER_WEIGHT | 3400 |
| COMET_MONO_WEIGHT | 7800 |
| COMET_TIP_WEIGHT | 11800 |
| COMET_TRANSVERSE_WEIGHT | 900 |

---

## 8. Solver weight relationships

If absolute unary scale changes in the future, preserve the following approximate ratio:

```text
taper : mono : tip : transverse  ~=  1.0 : 2.3 : 3.5 : 0.26
```

Interpretation:

- taper defines the intended width envelope
- monotonicity prevents local quantized re-widening
- tip sharpening must be stronger than taper
- transverse cleanup must remain secondary

---

## 9. Implementation notes

### 9.1 Determinism

Recommended core behavior:

- use fixed-point for positions, weights, and costs where feasible
- clamp every normalized quantity to `[0, 1]`
- use deterministic iteration order for cell maps and slice lists
- break cost ties by preferring previous committed state, then lower glyph id, then lower shade id

### 9.2 Stable support windows

At very low `tail_duration_ms`, min support steps keep the bands from degenerating to single-step flicker.

At very high `tail_duration_ms`, `DURATION_SCALE_EXPONENT < 1.0` prevents the far tail from becoming too broad or too persistent.

### 9.3 Simulation-rate stability

Support windows must be converted from milliseconds to step counts using the actual fixed simulation step.

Do not tune support windows directly in number of steps.

### 9.4 Speed coupling guidance

Do not speed-couple:

- band width scales
- band lifetimes
- filament intensity

Only couple the bright near-head envelope. This is critical for stable far-tail appearance.

### 9.5 Terminal floor

`COMET_MIN_RESOLVABLE_WIDTH` is a terminal-lattice floor, not an artistic preference.

Values below the floor destabilize:

- octant selection
- matrix selection
- non-empty vs empty transitions
- tip continuity

---

## 10. Tuning order

1. Fix formula correctness first.
   - anti-rewidening sign
   - speed in cells per second
   - taper progression
   - decay blend
   - curvature compression

2. Lock support-window scaling.
   - `DURATION_SCALE_MIN`
   - `DURATION_SCALE_MAX`
   - `DURATION_SCALE_EXPONENT`
   - min support steps

3. Lock the terminal floor.
   - `COMET_MIN_RESOLVABLE_WIDTH`
   - `FILAMENT_WIDTH_SCALE`

4. Tune the comet envelope.
   - `COMET_NECK_FRACTION`
   - `COMET_TIP_WIDTH_RATIO`
   - `COMET_TAPER_EXPONENT`
   - `COMET_TIP_ZONE_START`
   - `COMET_TIP_CAP_MULTIPLIER`

5. Tune band appearance.
   - band width scales
   - band lifetimes
   - band intensities
   - speed thresholds and minimum gains

6. Tune DP priors.
   - `COMET_MONO_WEIGHT` first
   - `COMET_TIP_WEIGHT` second
   - `COMET_TAPER_WEIGHT` third
   - `COMET_TRANSVERSE_WEIGHT` last

7. Tune curvature last.
   - `COMET_CURVATURE_COMPRESS_FACTOR`
   - `COMET_CURVATURE_KAPPA_CAP`

8. Touch decay mix only after the above are stable.
   - `TAIL_WEIGHT_EXPONENT`
   - `COMBINED_*`
   - `RECENT_*`

---

## 11. Failure modes

### 11.1 Taper weight too low

Symptoms:

- tail stays ribbon-flat
- mid-tail remains too broad
- local unary fit dominates intended comet envelope

### 11.2 Taper weight too high

Symptoms:

- forced thinning against real coverage
- kinks around bends
- head-to-neck transition looks synthetic

### 11.3 Monotonic weight too low

Symptoms:

- 1-cell re-widening bulges
- zipper texture across adjacent slices
- alternating chunky and thin cross-sections

### 11.4 Monotonic weight too high

Symptoms:

- tail over-thins and locks
- solver stops accommodating harmless quantization noise
- bulb may flatten too early

### 11.5 Tip weight too low

Symptoms:

- blunt or square tail end
- final slices read like a fading ribbon, not a pointed tip

### 11.6 Tip weight too high

Symptoms:

- tail pinches too early
- final cells pop to empty
- tip looks chopped instead of pointed

### 11.7 Transverse weight too low

Symptoms:

- filament becomes chunky
- 2-cell-wide local minima appear too often
- diagonal body looks brushy

### 11.8 Transverse weight too high

Symptoms:

- under-filled mid-body
- continuity breaks on medium diagonals
- holes or weak body segments appear

### 11.9 Minimum resolvable width too low

Symptoms:

- tip shimmer
- disappearing filament
- unstable glyph toggling near the end

### 11.10 Minimum resolvable width too high

Symptoms:

- tail end stays blunt
- no pointed tip
- overall effect regresses toward a ribbon

### 11.11 Curvature compression too low

Symptoms:

- bends form fat knots or sausages
- directional read is lost in turns

### 11.12 Curvature compression too high

Symptoms:

- bends get pinched too hard
- body looks segmented at corners
- continuity can weaken on tight curves

### 11.13 Speed thresholds too low or gains too high

Symptoms:

- sheath stays active during slow motion
- head remains too broad
- ribbon-flatness returns

### 11.14 Speed thresholds too high or gains too low

Symptoms:

- only very fast motion gets a bulb
- ordinary movement looks too thin and weak

### 11.15 Tail exponent too low

Symptoms:

- far tail lingers too brightly
- effect becomes smoky instead of sharp

### 11.16 Tail exponent too high

Symptoms:

- filament dies too early
- trail becomes bulb plus neck without a convincing tail

---

## 12. Normative summary

The production strategy is:

1. represent trail as sparse multi-band deposited slices in display metric
2. keep head motion continuous when motion is allowed, even across surface changes that should not share one trail stroke
3. restart deposited trail history at true disconnects instead of drawing a fake bridge across the gap
4. compile the latent field with real-time support windows and smooth decay
5. derive a taper-first target width along the centerline
6. enforce monotonic anti-rewidening and tip sharpening in the ribbon-constrained solve
7. speed-couple only near-head bands
8. compress excess width in high-curvature regions
9. keep the far-tail floor at the minimum stable terminal-resolvable width

The intended visual result is not a fading ribbon. It is a comet:

`bulb -> neck -> filament -> pointed tip`

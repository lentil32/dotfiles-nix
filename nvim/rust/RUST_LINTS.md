# Rust Lint Policy

This workspace enforces lint policy in one place:

- `Cargo.toml` under `[workspace.lints.rust]` and `[workspace.lints.clippy]`
- `clippy.toml` for shared clippy configuration values (only for lints that are not hard `allow`/`deny` in `Cargo.toml`)

## Required guarantees

- `unsafe` is forbidden.
- `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, and `dbg!` are denied.
- All crates opt into workspace lint configuration via:
  - `[lints]`
  - `workspace = true`

## Split of responsibilities

- Put lint levels (`allow`/`warn`/`deny`) in `Cargo.toml`.
- Put numeric/config knobs (for example argument thresholds) in `clippy.toml`.
- Do not keep thresholds for lints that are already `allow`ed in `Cargo.toml`, because those settings are inactive.

## Why this exists

- Keep lint behavior consistent across all plugin and core crates.
- Keep core crates predictable and panic-free in production paths.
- Catch regressions early with one command.

## Validation

Run from `nvim/rust`:

```bash
cargo clippy --workspace --all-targets
cargo test --workspace
```

# Porting Neovim plugins to Rust (nvim-oxi)

This doc captures the local workflow used in this repo to port a Lua plugin to
Rust using nvim-oxi and Nix. It is tuned for macOS + nix-darwin + home-manager.

## When to port
- The plugin logic is small but performance sensitive (e.g. repeated root scans)
- You want strong typing and safer refactors
- You need native integrations not exposed in Lua

## High level steps
1) Create a Rust crate under `nvim/rust/plugins/<plugin>` for Lua-facing plugins
   and name the package `rs_<plugin>`
2) Implement a `#[nvim_oxi::plugin]` entry point returning a Dictionary of
   functions
3) Compile as a `cdylib` and install the compiled library under `lua/` with
   the correct name
4) Add the built output to Neovim runtimepath (via Nix plugin packaging)
5) Provide a small Lua wrapper for ergonomic API + lazy loading

## 1) Crate layout
Example (mirrors `nvim/rust/plugins/project_root`):

```
nvim/rust/
  Cargo.toml            # workspace
  Cargo.lock
  .cargo/config.toml
  plugins/
    <plugin>/
      Cargo.toml
      src/lib.rs
```

`Cargo.toml`:

```
[package]
name = "rs_<plugin>"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
nvim-oxi = { version = "0.6", features = ["neovim-0-11"] }
```

macOS needs dynamic lookup for Neovim symbols. Configure
`nvim/rust/.cargo/config.toml`:

```
[target.x86_64-apple-darwin]
rustflags = [
  "-C", "link-arg=-undefined",
  "-C", "link-arg=dynamic_lookup",
]

[target.aarch64-apple-darwin]
rustflags = [
  "-C", "link-arg=-undefined",
  "-C", "link-arg=dynamic_lookup",
]
```

## 2) Plugin entry point
The plugin should expose a small API to Lua. Example pattern:

```
use nvim_oxi::{Dictionary, Function};

#[nvim_oxi::plugin]
fn rs_plugin() -> Dictionary {
    let mut api = Dictionary::new();
    api.insert("setup", Function::<(), ()>::from_fn(|()| ()));
    api.insert("do_thing", Function::<(), String>::from_fn(|()| "ok".to_string()));
    api
}
```

Each entry becomes a Lua-callable function on `require("rs_<plugin>")`.

## 3) Package it for Neovim (Nix)
This repo uses `buildRustPackage` to compile and install the compiled library
into `$out/lua/<plugin>.so` (or `.dll` on Windows). macOS still uses `.so` for
Lua modules even though Rust produces `.dylib`.

Key points:
- Copy the *dynamic* library only (`lib<plugin>.dylib` or `.so`)
- Do not copy `.rlib` or `.a` (they will fail to load with "not valid mach-o")
- Prefer `target/release` artifacts

The current pattern lives in `home/neovim.nix` under `mkRustPlugin`
and `rustPluginOrder`.

## 4) Ensure runtimepath contains the plugin
The plugin must be on Neovim runtimepath, and the compiled library must be at:

```
<rtp>/lua/<plugin>.so
```

Using Nix, adding the plugin to the `startupPlugins` list guarantees the rtp
entry is added.

## 5) Lua wrapper (recommended)
Keep a small Lua module that `require`s the Rust plugin and provides stable
API for the rest of your config. This also gives a place to handle lazy
load errors and extra UX.

Patterns used in this repo:
- Mandatory plugin: direct `require("<plugin>")` (e.g. `nvim/lua/myLuaConf/project.lua`)
- Optional plugin: `pcall(require, "<plugin>")` with fallback
  (e.g. `nvim/lua/myLuaConf/readline.lua`)

## Debugging tips
- If you see "slice is not valid mach-o file", you are loading a static
  artifact (like `.rlib` or `.a`) instead of the dynamic library.
- If `require` fails, log the error message from `pcall` and verify the
  runtimepath contains the package.
- For plugin-specific debugging, gate notifications on a global variable
  (e.g. `vim.g.<plugin>_debug = 1`) to avoid noisy logs.

## Performance notes
- Cache results in buffer variables where possible (e.g. `b:project_root`)
- Still refresh on demand if the cache is empty or invalid
- Keep filesystem traversal minimal (short list of root indicators)

## Checklist
- [ ] `crate-type = ["cdylib"]`
- [ ] `nvim/rust/.cargo/config.toml` has macOS dynamic lookup flags
- [ ] Nix installPhase copies `lib<plugin>.dylib` or `.so` to `lua/<plugin>.so`
- [ ] Plugin is in runtimepath (`startupPlugins` or `optionalPlugins`)
- [ ] Lua-facing plugin crate/module uses `rs_` prefix consistently
- [ ] Lua wrapper handles load errors gracefully

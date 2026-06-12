# Contributing

## Design contract

Every tool in this suite must follow these rules. New tools and PRs are evaluated against them.

**Output**
- JSON compact by default. No exceptions.
- `--out pretty` for human-readable JSON
- `--out table` for ASCII table (humans spot-checking)
- `--out ndjson` for streaming/pipe use
- Errors go to stderr as JSON: `{"error": "...", "path": "..."}`

**Performance**
- Rust only. No Python, no Node, no shell wrappers.
- Release profile: `opt-level=3`, `lto=true`, `strip=true`
- Static binary where possible — no dynamic deps the user has to install
- Startup must be sub-10ms for simple queries

**Timestamps**
- Always unix epoch integers. Never formatted strings in default output.
- Table mode may format them for readability.

**Naming**
- Tools are named `<concept>x` — one word, one letter suffix
- Binary name matches crate name

## Adding a new tool

1. Create `your-tool/` under the workspace root
2. Add to `[workspace.members]` in root `Cargo.toml`
3. Follow the output/error/timestamp rules above
4. Add a section to `README.md`
5. Open a PR with: what it replaces, why it's better for agents

## Building

```bash
cargo build --workspace --release
```

Binaries land in `target/release/`.

## Linting

```bash
cargo fmt --all
cargo clippy --workspace --release
```

CI enforces both. PRs won't merge with clippy warnings.

## Releasing

Push a tag matching `v*` (e.g. `git tag v0.1.0 && git push origin v0.1.0`) to trigger
the [release workflow](.github/workflows/release.yml). It builds binaries for Linux,
macOS and Windows, packages them to match `install.sh` / `install.ps1`, and publishes
them as GitHub release assets.

## aiux — single entry point

`aiux` is a thin dispatcher: `aiux <tool> [args...]` runs `<tool> [args...]` as if
you'd called it directly. It looks for `<tool>` next to the `aiux` binary first (the
release archives bundle all tools together), falling back to `PATH`. Useful for
agents that only want to remember one binary name. New tools are picked up by adding
their name to the `TOOLS` list in `aiux/src/main.rs`.

## Future consideration: single multi-call binary

`aiux` still requires every tool binary to be present alongside it. A
`busybox`/`uutils`-style multi-call binary would go further: one binary in `PATH`,
with each tool name (`lx`, `px`, `idx`, ...) as a symlink/copy that dispatches based
on `argv[0]`, and no separate binaries to ship at all.

This requires a non-trivial refactor and should be scoped as its own effort:

1. For each tool crate, split `src/main.rs` into a `src/lib.rs` exposing
   `pub fn run(args: Vec<String>) -> i32` (returning the process exit code), plus a
   thin `src/main.rs` that calls it — so standalone per-tool binaries keep working.
2. Have `aiux` depend on all tool crates as libraries and dispatch to `run()`
   directly instead of spawning a subprocess.
3. Tools using `#[tokio::main]` need to construct their own `tokio::Runtime` inside
   `run()` instead of relying on the attribute macro (dispatch is sequential, so
   only one runtime is ever active).
4. Update the packaging step to create symlinks (Unix) or copies (Windows, where
   symlinks need elevated privileges) named after each tool pointing at `aiux`.
5. Roll out incrementally — pilot with 2-3 tools (e.g. `lx`, `px`, `idx`) to validate
   before converting the rest.

Not started — flagging here so it's not lost.

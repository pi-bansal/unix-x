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

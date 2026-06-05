# lx

Structured filesystem metadata. Built for AI agents.

`ls`, `find`, `stat`, `du` — collapsed into one fast binary with JSON output.

## Build

```bash
cargo build --release
# binary at ./target/release/lx
cp target/release/lx /usr/local/bin/lx
```

## Usage

```bash
# Current dir, depth 2 (default)
lx

# Specific path, pretty-printed
lx ./src --pretty

# Deeper traversal
lx . --depth 4

# Files only, specific extension
lx . --files-only --ext rs

# Include hidden files, skip gitignore
lx . --all --no-gitignore

# Newline-delimited JSON (pipe-friendly)
lx . --ndjson | jq 'select(.type == "file" and .size > 10000)'

# No git status (faster on large repos)
lx . --no-git
```

## Output

Compact JSON by default. `--pretty` for humans.

```json
{
  "root": "/home/user/project",
  "depth": 2,
  "count": 14,
  "entries": [
    {
      "name": "main.rs",
      "path": "/home/user/project/src/main.rs",
      "type": "file",
      "size": 4821,
      "modified": 1748123456,
      "created": 1747000000,
      "permissions": "644",
      "git_status": "modified",
      "extension": "rs"
    },
    {
      "name": "src",
      "path": "/home/user/project/src",
      "type": "dir",
      "size": 48210,
      "modified": 1748123400,
      "permissions": "755",
      "git_status": "clean",
      "children": 4
    }
  ]
}
```

## Design principles

- **JSON first**: compact by default, `--pretty` is the flag
- **Timestamps as integers**: unix epoch, let callers format
- **Errors are structured**: stderr JSON, never raw text blobs
- **Recursive sizes for dirs**: agents need real sizes, not dir inode size
- **Git-aware by default**: status per entry, disable with `--no-git`
- **`--ndjson` for streaming**: pipe into `jq`, `grep`, etc.
- **Static binary**: no runtime deps, deploy anywhere

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `path` | `.` | Root path to inspect |
| `--depth` / `-d` | `2` | Max traversal depth |
| `--all` / `-a` | off | Include hidden files |
| `--no-gitignore` | off | Don't respect .gitignore |
| `--no-git` | off | Skip git status (faster) |
| `--pretty` / `-p` | off | Pretty-print JSON |
| `--ndjson` | off | One JSON object per line |
| `--files-only` / `-f` | off | Omit directories |
| `--ext` / `-e` | none | Filter by extension |

## Part of a suite

`lx` is the first in a planned suite of agent-optimized Unix primitives:

- `lx` — filesystem metadata ✓
- `px` — process & network inspection (planned)
- `logx` — structured log querying (planned)
- `dx` — semantic diff with structured output (planned)
- `arcx` — unified archive inspection (planned)

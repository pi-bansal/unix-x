# confx

Structured config reader. Built for AI agents.

YAML, TOML, INI, `.properties` (and JSON) — parsed into JSON so an agent can read
any config uniformly and pipe it straight into `jsonx`.

## Build

```bash
cargo build --release
# binary at ./target/release/confx
cp target/release/confx /usr/local/bin/confx
```

## Usage

```bash
# Parse by extension (auto-detected)
confx app.yaml
confx Cargo.toml --out pretty
confx settings.ini
confx db.properties

# Force a format (e.g. a file with no/unknown extension)
confx mystery --format toml

# --raw: emit just the parsed value, ideal for piping into jsonx
confx app.yaml --raw | jsonx '.server.port'

# Parse several files at once
confx a.yaml b.toml c.ini
```

## Output

Compact JSON by default (auto-pretty when stdout is a terminal). A single file
yields `{path, format, data}`; with `--raw`, just the parsed value:

```json
{
  "path": "app.yaml",
  "format": "yaml",
  "data": {
    "server": { "port": 8080, "host": "localhost" }
  }
}
```

Multiple files are wrapped:

```json
{
  "count": 2,
  "files": [
    {"path": "a.yaml", "format": "yaml", "data": {"x": 1}},
    {"path": "b.toml", "format": "toml", "data": {"y": 2}}
  ]
}
```

Failures are structured — a missing file, parse error, or unknown format yields an
`error` field (per file when several are given) and a non-zero exit code.

## Formats

| Format | Extensions | Notes |
|--------|-----------|-------|
| YAML | `.yaml`, `.yml` | Multiple `---` docs become a JSON array |
| TOML | `.toml` | Native types preserved |
| INI | `.ini`, `.cfg`, `.conf` | One object per `[section]`; values are strings |
| `.properties` | `.properties` | Flat object of strings; `=`/`:`, `#`/`!` comments, `\` continuation |
| JSON | `.json` | Pass-through (uniform front door) |

INI and `.properties` carry no type information, so their values are emitted as
strings — format them downstream if you need numbers or booleans.

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `files` | — | One or more config files to parse (required) |
| `--format` / `-f` | auto | Force input format: yaml, toml, ini, properties, json |
| `--raw` / `-r` | off | Emit just the parsed data (single file) |
| `--out` / `-o` | `auto` | Output mode: auto, json, pretty, table, ndjson |

## Design principles

- **JSON first**: compact by default, `--out pretty` for humans
- **Uniform front door**: every config format becomes JSON, queryable by `jsonx`
- **Errors are structured**: stderr/`error` field, never raw text blobs
- **`--raw` for composability**: pipe a parsed config straight into `jsonx`
- **Static binary**: no runtime deps, deploy anywhere

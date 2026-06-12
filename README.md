# aiutilx

Agent-optimized replacements for classic Unix tools. Fast, structured, JSON-first.

Built for AI agents that call shell tools. Humans can use them too — `--out table` renders a readable ASCII view, and output is pretty-printed automatically when stdout is a terminal.

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/pi-bansal/aiutilx/main/install.sh | bash

# Windows (PowerShell)
iwr https://raw.githubusercontent.com/pi-bansal/aiutilx/main/install.ps1 | iex
```

---

## Why

Unix tools were designed for humans. Output is formatted text meant to be read in a terminal. When an AI agent calls `ps aux` it gets a column-aligned table it has to parse back into structured data — slow, brittle, and wasteful.

`aiutilx` tools output JSON by default. Timestamps are integers. Errors are structured. No parsing required.

Inspired by how `ripgrep` reimagined `grep` — not a wrapper, a ground-up rethink with the right defaults.

---

## The suite

| Tool | Replaces | What it does |
|------|----------|-------------|
| [`lx`](#lx) | ls, find, stat, du | Filesystem metadata |
| [`px`](#px) | ps, lsof, netstat | Process & network inspection |
| [`logx`](#logx) | grep, tail, awk | Structured log querying |
| [`dx`](#dx) | diff, patch | Semantic diff with structured output |
| [`arcx`](#arcx) | tar, zip, gzip | Unified archive inspection |
| [`envx`](#envx) | printenv, cat .env | Environment & secrets inspection |
| [`netx`](#netx) | curl, wget | Structured HTTP client |
| [`jsonx`](#jsonx) | jq, python -m json.tool | Fast JSON query & transform |
| [`procx`](#procx) | crontab -l, systemctl, launchctl | Scheduled job inspection |
| [`idx`](#idx) | find + inotifywait/FSEvents | Persistent columnar index + bloom filter |
| [`diffx`](#diffx) | diff3, merge, patch | Structured three-way merge |
| [`memx`](#memx) | pmap, vmmap, cat /proc/smaps | Process memory inspection |
| [`statx`](#statx) | vmstat, iostat, sar, top | Time-series system stats |
| [`hashx`](#hashx) | md5sum, sha1sum, sha256sum, b3sum | Multi-algorithm file hashing |
| [`termx`](#termx) | tput, stty, env heuristics | Terminal & TTY inspection |
| [`astx`](#astx) | ctags, tree-sitter CLI | Source-code AST & symbol extraction |
| [`dnsx`](#dnsx) | dig, nslookup, host | Structured DNS lookups |
| [`confx`](#confx) | yq, manual config parsing | YAML/TOML/INI/.properties to JSON |

`aiux` is a dispatcher: `aiux <tool> [args...]` runs `<tool> [args...]` directly —
useful if you only want to remember one binary name.

---

## Design contract

Every tool follows the same rules:

- **JSON compact by default** — `--out pretty` for humans, or pipe to a terminal (auto-detected)
- **`--out table`** — ASCII table for spot-checking
- **`--out ndjson`** — newline-delimited, one object per line, pipe-friendly
- **Timestamps as integers** — unix epoch seconds, always
- **Errors to stderr as JSON** — `{"error": "...", "path": "..."}`
- **Platform fallbacks are structured** — unavailable features return `{"unavailable": {"reason": "...", "suggestion": "..."}}` not silence
- **Static binary** — no runtime dependencies

---

## Build

The install scripts at the top of this README download prebuilt binaries from the
[latest release](https://github.com/pi-bansal/aiutilx/releases/latest) — that's the
fastest way to get started and requires no Rust toolchain.

To build from source instead:

```bash
git clone https://github.com/pi-bansal/aiutilx
cd aiutilx
cargo build --workspace --release

# All binaries land in target/release/
cp target/release/{lx,px,logx,dx,arcx,envx,netx,jsonx,procx,idx,diffx,memx,statx,hashx,termx,astx,dnsx,confx} /usr/local/bin/
```

You can force `install.sh` to build from source instead of fetching a release:

```bash
BUILD_FROM_SOURCE=1 curl -fsSL https://raw.githubusercontent.com/pi-bansal/aiutilx/main/install.sh | bash
```

### Releases

Pushing a tag matching `v*` (e.g. `v0.1.0`) triggers the [release workflow](.github/workflows/release.yml),
which builds binaries for Linux (x86_64, aarch64), macOS (x86_64, aarch64) and Windows (x86_64),
packages them to match what `install.sh` / `install.ps1` expect, and publishes them as
GitHub release assets.

---

## Tools

### lx

**Replaces:** `ls`, `find`, `stat`, `du`, `tree`

Walks a directory tree and returns structured metadata for every entry. Recursive directory sizes, git status per file, extension filtering — all in one call.

```bash
lx ./src                              # inspect directory, depth 2
lx . --depth 4                        # deeper traversal
lx . --ext rs                         # only .rs files
lx . --all                            # include hidden files
lx . --no-git                         # skip git status (faster on large repos)
lx . --files-only                     # omit directories
lx . --out ndjson | jsonx '[?(@.size > 10000)]'
lx . --out table                      # human-readable
```

**Output fields:** `name`, `path`, `type`, `size` (bytes, recursive for dirs), `modified` (epoch), `created` (epoch), `permissions` (octal), `git_status`, `children` (dir count), `extension`

---

### px

**Replaces:** `ps`, `ps aux`, `lsof`, `netstat`, `ss`, `top` (for snapshots)

Collects process and network state in one structured call. Sorted by CPU descending by default.

```bash
px                                    # all processes
px --name nginx                       # filter by process name
px --pid 1234                         # specific PID
px --port 3000                        # what process is holding port 3000?
px --network --listen                 # all listening sockets
px --system                           # add CPU/memory/load summary
px --env                              # include environment variables
px --out table                        # human-readable
```

**Output fields:** `pid`, `ppid`, `name`, `exe`, `cmd`, `status`, `cpu_percent`, `memory_bytes`, `virtual_memory_bytes`, `started_at` (epoch), `run_time_secs`, `user`, `cwd`

**Network fields:** `protocol`, `local_addr`, `local_port`, `remote_addr`, `remote_port`, `state`, `pid`, `process_name`

**Platform note:** Network inspection via `/proc/net` is Linux-only. On macOS/Windows, a structured `unavailable` block is returned with platform-specific alternatives.

---

### logx

**Replaces:** `grep`, `tail -f`, `awk`, `sed` (for log parsing), `jq` (on JSON logs)

Auto-detects log format from the first line. Parses, filters, and normalizes across all formats.

**Detected formats:** JSON (`{"level":"info",...}`), logfmt (`level=info msg=...`), nginx combined, syslog RFC 3164, Rails, Go stdlib

```bash
logx app.log                          # parse and structure entire file
logx app.log --min-level warn         # warn, error only
logx app.log --level error            # error only
logx app.log --grep "timeout"         # substring filter
logx app.log --regex "user_id=\d+"    # regex filter
logx app.log --tail 100               # last 100 lines
logx app.log --head 50                # first 50 lines
logx app.log --stats                  # counts by level, no entries
logx app.log.gz                       # reads .gz natively
cat app.log | logx                    # stdin
logx app.log --out ndjson             # stream one entry per line
```

**Output fields:** `line` (number), `timestamp`, `level` (normalized: error/warn/info/debug), `message`, `fields` (extra structured fields), `raw`

**Level normalization:** `fatal`/`crit`/`alert`/`emerg` → `error`, `warning`/`notice` → `warn`, `verbose`/`trace` → `debug`

---

### dx

**Replaces:** `diff`, `diff -u`, `patch`

Structured diff with typed hunks. Works on files and directories.

```bash
dx old.rs new.rs                      # file diff
dx old/ new/                          # directory diff
dx old.rs new.rs --context 5          # more context lines
dx old.rs new.rs --no-equal           # changes only, no context
dx old/ new/ --summary                # totals only, no hunks
dx a b --out ndjson | jsonx '[?(@.added_lines > 0)]'
```

**Output fields:** `total_files`, `total_added`, `total_removed`, `files[]` each with `old_path`, `new_path`, `added_lines`, `removed_lines`, `binary`, `hunks[]`

**Hunk fields:** `old_start`, `old_lines`, `new_start`, `new_lines`, `changes[]` each with `kind` (added/removed/equal), `line`, `old_lineno`, `new_lineno`

---

### arcx

**Replaces:** `tar -tf`, `unzip -l`, `zipinfo`, `zcat`, per-format listing commands

Unified interface across all common archive formats.

**Supported formats:** `.zip`, `.tar`, `.tar.gz`/`.tgz`, `.tar.bz2`/`.tbz2`, `.tar.xz`/`.txz`, `.gz`

```bash
arcx archive.zip                      # inspect zip
arcx bundle.tar.gz                    # inspect tarball
arcx dist.tar.gz --summary            # totals only (no entry list)
arcx dist.tar.gz --sort size          # largest files first
arcx app.zip --filter ".rs"           # filter entries by path
arcx data.tar.gz --files-only         # skip directory entries
arcx release.tar.gz --out ndjson      # stream entries
```

**Output fields:** `format`, `total_entries`, `total_size_uncompressed`, `total_size_compressed`, `compression_ratio`, `entries[]`

**Entry fields:** `path`, `type` (file/dir/symlink), `size_compressed`, `size_uncompressed`, `modified` (epoch), `permissions`, `link_target`

---

### envx

**Replaces:** `printenv`, `env`, `cat .env`, manual secret scanning

Merges shell environment and `.env` files with precedence tracking and secret detection.

**Scanned files:** `.env`, `.env.local`, `.env.development`, `.env.production`, `.env.test`

```bash
envx                                  # .env files in current directory
envx --shell                          # include shell environment
envx --redact                         # mask secret values as ***
envx --secrets-only                   # only flagged secrets
envx --filter DATABASE                # filter by key substring
envx --out table                      # human-readable
```

**Secret detection:** AWS access keys (`AKIA...`), JWTs (`eyJ...`), PEM private keys, long hex strings (≥32 chars), long base64 blobs, key name heuristics (`SECRET`, `TOKEN`, `PASSWORD`, `API_KEY`, `PRIVATE`)

**Output fields:** `key`, `value`, `source` (.env/.env.local/shell), `secret` (kind), `redacted`

---

### netx

**Replaces:** `curl`, `wget`, `httpie`

Structured HTTP client. Auto-parses response body based on Content-Type.

```bash
netx https://api.example.com/users
netx https://api.example.com/users -X POST --data '{"name":"alice"}' --json
netx https://api.example.com -H "Authorization: Bearer token" -H "X-Custom: value"
netx https://example.com --head-only  # headers only, no body
netx https://example.com --timeout 5  # 5 second timeout
netx https://example.com --out table  # human-readable summary
```

**Body parsing:** `application/json` → structured object, `text/*` → string, binary → base64 with `{"encoding":"base64","data":"..."}`

**Output fields:** `url`, `method`, `status`, `status_text`, `ok` (bool, 200-299), `headers`, `content_type`, `body`, `body_bytes`, `timing.total_ms`

---

### jsonx

**Replaces:** `jq`, `python3 -c "import json"`, `fx`

Fast JSON query and transform. Path syntax is a readable subset of JSONPath.

```bash
jsonx '.users[].name' data.json       # extract field from array
jsonx '.items[0]' data.json           # array index
jsonx '.results[0:5]' data.json       # slice
jsonx '.items[?(@.price > 10)]' data.json   # filter
jsonx '.items[?(@.active == "true")]' data.json
cat data.json | jsonx '.users[]' --out ndjson  # stream
jsonx --keys data.json                # list top-level keys
jsonx --values data.json              # list top-level values
jsonx --count '.users[]' data.json    # count matches
jsonx '.name' data.json --raw         # raw string output (no quotes)
jsonx --ndjson-in '.name' logs.ndjson # process NDJSON input
```

**Path syntax:** `.key`, `.key.nested`, `.[0]`, `.[]` / `.*` (wildcard), `[1:3]` (slice), `[?(@.key == "val")]`, `[?(@.n > 10)]`, `[?(@.n < 10)]`

**Filter operators:** `==`, `!=`, `>`, `>=`, `<`, `<=`, combined with `&&` and `||` — e.g. `[?(@.git_status == "modified" && @.size > 5000)]` (`||` binds looser than `&&`)

---

### procx

**Replaces:** `crontab -l`, `systemctl list-timers`, `systemctl list-units`, `launchctl list`, `cat /etc/cron.d/*`

Unified view of all scheduled jobs across cron, systemd, and launchd.

```bash
procx                                 # all jobs (auto-detects platform)
procx --source cron                   # cron only
procx --source systemd                # systemd units only
procx --source launchd                # launchd jobs only (macOS)
procx --filter backup                 # filter by name/label
procx --active                        # running jobs only
procx --failed                        # failed jobs only
procx --out table                     # human-readable
```

**Platform support:** cron (Linux + macOS), systemd (Linux with systemd init), launchd (macOS). Non-applicable sources return a structured `unavailable` block — never silence.

**Cron fields:** `schedule`, `schedule_human` (English), `command`, `user`, `source`

**Systemd fields:** `name`, `active_state`, `unit_type`, `timer_schedule`, `last_trigger`, `next_trigger`, `exec_start`, `pid`, `memory_bytes`

**Launchd fields:** `label`, `program`, `program_args`, `schedule`, `run_at_load`, `keep_alive`, `pid`, `last_exit_status`, `domain`

---

### idx

**Replaces:** `find`, `fd`, inotifywait/FSEvents polling, ctags-style indexing

Persistent columnar filesystem index with bloom filter. Makes `lx`-style queries instant on large repos.

**Data structures:**
- Columnar index: separate `Vec` per field (paths, sizes, mtimes, ext_ids, git_status). A `--ext rs` query scans only a `Vec<u16>` (~200KB on a 100k-file repo) instead of touching all path strings.
- Bloom filters: over filenames, extensions, and path prefixes. Extension existence check in ~5µs before any index scan.
- Incremental updates: inotify/FSEvents/ReadDirectoryChangesW events debounced 300ms, applied as row-level patches. Full rebuild only if >100 paths change at once.

```bash
idx start ~/project               # start daemon, background with & or a service
idx query --ext rs                # instant, bloom pre-checked
idx query --git modified          # all modified files
idx query --ext ts --size-gt 5000 --sort size
idx query --filter src/           # path substring
idx status                        # indexed count, last build time
idx rebuild                       # trigger full rebuild
idx once --ext proto              # no daemon: build index + query in one shot
```

**Output fields:** `path`, `size`, `mtime` (epoch), `extension`, `git_status`, `is_dir`

**Query fields:** `ext`, `git_status`, `size_gt`, `size_lt`, `mtime_gt`, `path_contains`, `dirs_only`, `files_only`, `limit`, `sort` (path/size/mtime)

---

### diffx

**Replaces:** `diff3`, `merge`, `git merge-file`, manual conflict resolution

Three-way merge that returns structured conflict objects — not `<<<< ==== >>>>` text markers.

```bash
diffx base.rs ours.rs theirs.rs           # three-way merge
diffx base.rs ours.rs theirs.rs -o out.rs # write resolved file (if clean)
diffx base.rs ours.rs theirs.rs --conflicts-only
diffx base.rs ours.rs theirs.rs --summary
```

**Exit codes:** `0` = clean merge (no conflicts), `1` = conflicts remain

**Auto-resolution:** Identical change on both sides (deduplicated), one side unchanged from base (accept the changed side). Only truly ambiguous overlapping edits become conflicts.

**Output fields:** `clean`, `conflict_count`, `auto_resolved_count`, `ours_only_count`, `theirs_only_count`, `resolved` (full merged text if clean), `hunks[]`

**Hunk kinds:** `unchanged`, `ours_only`, `theirs_only`, `both_same` (auto-resolved), `conflict`

**Conflict fields:** `kind`, `base_lines`, `ours_lines`, `theirs_lines`, `resolved_lines` (null if conflict), `start_line`, `resolution_hint`

---

### memx

**Replaces:** `pmap`, `pmap -x`, `cat /proc/<pid>/smaps`, `vmmap`, `vmmap -wide`

Structured per-region memory breakdown for a process.

```bash
memx 1234                         # summary for pid 1234
memx 1234 --regions               # full region list
memx 1234 --kind heap             # heap regions only
memx 1234 --kind anon             # anonymous mappings (custom allocators, JIT)
memx 1234 --kind lib              # shared library mappings
memx 1234 --sort rss              # by resident size
memx 1234 --out table             # human-readable
```

**Linux** (via `/proc/<pid>/smaps`): RSS, PSS (proportional — most accurate for "how much RAM does this process really use"), private/shared clean+dirty, swap, referenced per region.

**macOS** (via `vmmap -wide`): virtual size, dirty, swap per region.

**Region kinds:** `heap`, `stack`, `lib` (.so/.dylib), `exe`, `anon` (no backing file — custom allocators, JIT), `vdso`, `file`, `text`, `data`

**Summary fields:** `total_vss`, `total_rss`, `total_pss`, `total_private`, `total_shared`, `total_swap`, `heap_bytes`, `stack_bytes`, `lib_bytes`, `anon_bytes`, `region_count`

---

### statx

**Replaces:** `vmstat`, `iostat`, `sar`, `top` (for programmatic use), `dstat`

Time-series system stats stored in a circular ring buffer. Query any window of the past instantly.

```bash
statx start                       # start daemon (samples every 1s, retains 3600s)
statx start --interval 5          # sample every 5 seconds
statx now                         # single snapshot
statx last 60                     # last 60 samples from ring buffer
statx last 300                    # last 5 minutes
statx watch                       # live stream to stdout
statx watch --interval 2 --count 30
statx summary 60                  # min/max/avg/p50/p95 over last 60 samples
```

**Ring buffer:** fixed-size circular buffer, O(1) push, O(1) access to last N samples. Default capacity = 3600 (1 hour at 1s). No unbounded memory growth.

**Sample fields:** `ts` (epoch), `cpu_total`, `cpu_user`, `cpu_system`, `cpu_iowait`, `cpu_cores[]`, `mem_total`, `mem_used`, `mem_free`, `mem_available`, `swap_total`, `swap_used`, `disk_read_bps`, `disk_write_bps`, `net_rx_bps`, `net_tx_bps`, `load_1m`, `load_5m`, `load_15m`, `procs_running`, `procs_total`

**Summary fields:** per-metric `min`, `max`, `avg`, `p50`, `p95`

---

### hashx

**Replaces:** `md5sum`, `sha1sum`, `sha256sum`, `sha512sum`, `b3sum`, `rhash`

Multi-algorithm file hashing in parallel. One call, all the hashes you need.

```bash
hashx file.tar.gz                        # sha256 + blake3 (default)
hashx file.tar.gz --algos sha256         # specific algorithm
hashx file.tar.gz --algos md5,sha1,sha256,blake3  # all four
hashx release.zip --verify sha256:abc123def...     # verify against known hash
hashx a.bin b.bin --compare              # are these files identical?
hashx *.rs                               # multiple files, parallel
hashx large.iso --algos blake3           # fast: blake3 on mmap'd file
hashx *.tar.gz --out table               # human-readable with short hashes
```

**Performance:** Files hashed in parallel (rayon). Files >1MB use mmap instead of buffered read. All requested algorithms computed in a single pass over the data.

**Exit codes:** `0` = all verifications passed (or no verify), `1` = any verification failed

**Output fields:** `path`, `size_bytes`, `elapsed_ms`, `md5`, `sha1`, `sha256`, `blake3`, `verified`, `error`

---

### termx

**Replaces:** `tput cols`, `tput lines`, `tput colors`, `stty size`, TTY environment heuristics

Complete terminal and TTY inspection in one call. Agents use this to decide whether to emit color, progress bars, or compact output.

```bash
termx                             # full inspection
termx --out table                 # human-readable
```

**Output fields:**

| Field | Description |
|-------|-------------|
| `is_tty` | stdout connected to a terminal |
| `tty_device` | e.g. `/dev/pts/0` |
| `cols` / `rows` | terminal dimensions |
| `color_depth` | `none` / `basic` (8) / `extended` (256) / `true_color` (24-bit) |
| `color_depth_bits` | 0 / 4 / 8 / 24 |
| `term` | `$TERM` value |
| `term_program` | iTerm2, vscode, WezTerm, Alacritty, Kitty, Hyper... |
| `shell` | `$SHELL` full path |
| `shell_name` | bash, zsh, fish, nushell... |
| `multiplexer` | tmux / screen / zellij / wezterm |
| `editor` | `$EDITOR` / `$VISUAL` |
| `pager` | `$PAGER` |
| `lang` | `$LANG` |
| `unicode` | true if LANG contains UTF |
| `ci` | true if running in CI |
| `ci_name` | GitHub Actions / GitLab CI / CircleCI / Travis / Jenkins / Buildkite |
| `interactive` | `is_tty && !ci` — the key field for agents |

**Color detection priority:** `$COLORTERM=truecolor` → known truecolor terminals → `$TERM=*256color` → `$TERM=xterm*` → basic → none

---

### astx

**Replaces:** `ctags`, the `tree-sitter` CLI, ad-hoc regex/grammar scraping of source files

Parses a source file into a structured JSON AST using tree-sitter. Agents get real syntax structure — declarations, ranges, node kinds — without screen-scraping a grammar tool's text output.

**Supported languages:** Rust, Python, JavaScript, TypeScript, TSX, Go (detected by extension, override with `--lang`)

```bash
astx src/main.rs                      # full AST as JSON
astx src/main.rs --symbols            # declarations only (ctags-style)
astx src/main.rs --symbols --out table
astx app.py --kind function_definition   # flat list of nodes by kind
astx src/lib.rs --depth 2             # cap AST depth
astx src/main.rs --query '(function_item name: (identifier) @fn)'  # tree-sitter query
astx script --lang python --symbols   # force a language when there's no extension
```

**AST node fields:** `kind`, `named`, `start` / `end` (each `{row, col, byte}`), `text` (leaf nodes only), `field` (field name in parent, if any), `children`

**Symbol fields** (`--symbols`): `name`, `kind`, `start`, `end`

**Errors & fallbacks:** a missing file emits `{"error": "...", "path": "..."}` on stderr; an unsupported extension emits a structured `{"unavailable": {...}}` block listing the supported languages — never a crash.

---

### dnsx

**Replaces:** `dig`, `nslookup`, `host`

Resolves DNS records into structured JSON. TTLs are integers, records are typed and grouped, and the resolver used is reported — no scraping `dig`'s columnar output. Pure-Rust resolver (hickory), no `libresolv` or external binary required.

```bash
dnsx example.com                      # A records
dnsx example.com --type MX,TXT        # specific record types (comma-separated)
dnsx example.com --all                # common set: A, AAAA, MX, TXT, NS, CNAME, SOA
dnsx example.com --server 1.1.1.1     # query a specific resolver (or 8.8.8.8:53)
dnsx 93.184.216.34 --reverse          # reverse (PTR) lookup
dnsx example.com --type MX --out table
```

**Supported record types:** `A`, `AAAA`, `MX`, `TXT`, `CNAME`, `NS`, `SOA`, `PTR`, `SRV`, `CAA`

**Record fields:** `type`, `name`, `ttl` (integer seconds), `value`; plus `priority` (MX/SRV), `weight` / `port` (SRV), and `target` (MX/SRV/CNAME/NS/PTR).

**Top-level fields:** `query`, `resolver` (`"system"` or `ip:port`), `record_types`, `records`, `count`, `elapsed_ms`.

**Errors & fallbacks:** an unknown record type or malformed resolver/IP emits `{"error": "...", "query": "..."}` on stderr and exits non-zero. A name that simply has no records of the requested type is a *successful* empty result (`count: 0`), not an error.

---

### confx

**Replaces:** `yq`, and the hand-rolled parsing agents fall back to for non-JSON configs

`jsonx` queries JSON, but configs come as YAML, TOML, INI, and `.properties`. `confx` parses all of them into JSON so an agent can read any config uniformly — and pipe the result straight into `jsonx`. JSON itself passes through, making `confx` a single front door for every config format.

```bash
confx app.yaml                        # auto-detect by extension
confx Cargo.toml --out pretty
confx settings.ini                    # one object per [section]
confx db.properties                   # flat key/value object
confx mystery --format toml           # force a format when extension is unknown
confx app.yaml --raw | jsonx '.server.port'   # pipe parsed config into jsonx
confx a.yaml b.toml c.ini             # parse several files at once
```

**Supported formats:** YAML (`.yaml`/`.yml`, multi-doc → array), TOML (`.toml`), INI (`.ini`/`.cfg`/`.conf`), `.properties`, and JSON (`.json`, pass-through). INI and `.properties` are untyped, so their values are emitted as strings.

**Output fields:** single file → `path`, `format`, `data` (or `error`); with `--raw`, just the parsed value. Multiple files → `count`, `files[]`.

**Errors & fallbacks:** a missing file, parse error, or unknown extension (with no `--format`) yields a structured `error` — on stderr for `--raw`, otherwise an `error` field per file — and a non-zero exit code.

---

## Composing tools

```bash
# Large modified Rust files
lx . --out ndjson --ext rs | jsonx '[?(@.git_status == "modified" && @.size > 5000)]'

# What process is holding port 8080?
px --port 8080 --out pretty

# Errors in the last 200 log lines
logx app.log --tail 200 --min-level error --out ndjson

# Largest files in a release archive
arcx release.tar.gz --files-only --sort size --out ndjson

# All secrets in current project, redacted
envx --secrets-only --redact --out table

# Failing systemd timers
procx --source systemd --failed --out table

# Query an API and extract a field
netx https://api.example.com/data | jsonx '.results[0].id' --raw

# Find all .proto files instantly (idx daemon running)
idx query --ext proto

# Did these two builds produce identical binaries?
hashx build-a/app build-b/app --compare

# Is this terminal capable of rendering rich UI?
termx | jsonx '.interactive' --raw

# What's been happening to CPU in the last 5 minutes?
statx summary 300 --out pretty

# How much heap is this process actually using?
memx $(pgrep myapp) --kind heap --out table

# Three-way merge for an agent patch application
diffx base.py ours.py theirs.py --out pretty

# Read a YAML config's port without a YAML parser
confx app.yaml --raw | jsonx '.server.port' --raw
```

---

## Platform support

| Tool | Linux | macOS | Windows |
|------|-------|-------|---------|
| lx | ✓ | ✓ | ✓ |
| px (processes) | ✓ | ✓ | ✓ |
| px (network) | ✓ | structured fallback | structured fallback |
| logx | ✓ | ✓ | ✓ |
| dx | ✓ | ✓ | ✓ |
| arcx | ✓ | ✓ | ✓ |
| envx | ✓ | ✓ | ✓ |
| netx | ✓ | ✓ | ✓ |
| jsonx | ✓ | ✓ | ✓ |
| procx (cron) | ✓ | ✓ | structured fallback |
| procx (systemd) | ✓ | structured fallback | structured fallback |
| procx (launchd) | structured fallback | ✓ | structured fallback |
| idx | ✓ | ✓ | ✓ |
| diffx | ✓ | ✓ | ✓ |
| memx | ✓ | ✓ | structured fallback |
| statx | ✓ | ✓ | ✓ |
| hashx | ✓ | ✓ | ✓ |
| termx | ✓ | ✓ | ✓ |
| astx | ✓ | ✓ | ✓ |
| dnsx | ✓ | ✓ | ✓ |
| confx | ✓ | ✓ | ✓ |

"Structured fallback" means the tool returns a JSON `unavailable` block with a reason and platform-specific alternative — never silence, never a crash.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

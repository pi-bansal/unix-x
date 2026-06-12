#!/usr/bin/env bash
# Raw performance comparison: aiutilx tools vs standard equivalents.
# Times N iterations of each command and reports average wall time (ms).
set -euo pipefail

BIN=target/release
N=${N:-20}
DATA=/tmp/bench

avg_ms() {
  local label="$1"; shift
  local start end total=0
  for i in $(seq 1 "$N"); do
    start=$(date +%s%N)
    "$@" >/dev/null 2>&1
    end=$(date +%s%N)
    total=$((total + (end - start)))
  done
  printf "%-32s %8.2f ms\n" "$label" "$(echo "$total / $N / 1000000" | bc -l)"
}

echo "=== Directory listing (50 subdirs, 150 files) ==="
avg_ms "lx (json, depth 2)"      "$BIN/lx" "$DATA" -d 2 --no-git -o json
avg_ms "ls -laR"                 ls -laR "$DATA"
avg_ms "find + stat"             bash -c "find '$DATA' -maxdepth 2 -exec stat -c '%n %s %Y' {} +"
echo

echo "=== File hashing ==="
avg_ms "hashx (sha256+blake3)"   "$BIN/hashx" "$DATA/dir1/file1.json"
avg_ms "sha256sum"               sha256sum "$DATA/dir1/file1.json"
echo

echo "=== JSON query ==="
avg_ms "jsonx '.id'"             "$BIN/jsonx" '.id' "$DATA/dir1/file1.json"
avg_ms "jq '.id'"                jq '.id' "$DATA/dir1/file1.json"
echo

echo "=== Process listing ==="
avg_ms "px (json)"               "$BIN/px" -o json
avg_ms "ps aux"                  ps aux
echo

echo "=== System stats snapshot ==="
avg_ms "statx now"                "$BIN/statx" now
avg_ms "vmstat 1 1"               vmstat 1 1

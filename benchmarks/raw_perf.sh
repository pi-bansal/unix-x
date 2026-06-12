#!/usr/bin/env bash
# Raw performance comparison: aiutilx tools vs standard equivalents.
# Times N iterations of each command and reports average wall time (ms).
set -uo pipefail

BIN=target/release
N=${N:-20}
DATA=/tmp/bench
MYPID=$$

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
echo

echo "=== File diff (100-line files, 1 change) ==="
avg_ms "dx"                       "$BIN/dx" "$DATA/a.txt" "$DATA/b.txt"
avg_ms "diff -u"                  diff -u "$DATA/a.txt" "$DATA/b.txt"
echo

echo "=== Log filtering (500 lines) ==="
avg_ms "logx --grep request"      bash -c "$BIN/logx --grep request '$DATA/app.log'"
avg_ms "grep request"             grep request "$DATA/app.log"
echo

echo "=== Environment inspection ==="
avg_ms "envx -e"                  "$BIN/envx" -e
avg_ms "printenv"                 printenv
echo

echo "=== Memory inspection (self) ==="
avg_ms "memx (self pid)"          "$BIN/memx" "$MYPID"
avg_ms "cat /proc/self/status"    cat /proc/$MYPID/status
echo

echo "=== Archive listing ==="
avg_ms "arcx"                     "$BIN/arcx" "$DATA/test.tar.gz"
avg_ms "tar -tzf"                 tar -tzf "$DATA/test.tar.gz"
echo

echo "=== Terminal inspection ==="
avg_ms "termx"                    "$BIN/termx" -o json
avg_ms "tput cols + stty -a"      bash -c "tput cols; stty -a"
echo

echo "=== DNS resolution ==="
avg_ms "dnsx example.com"         "$BIN/dnsx" example.com
avg_ms "getent ahosts"            getent ahosts example.com
echo

echo "=== HTTP request ==="
avg_ms "netx (json)"              "$BIN/netx" https://example.com -o json
avg_ms "curl -s"                  curl -s https://example.com
echo

echo "=== Tools with no standard equivalent (startup/baseline cost) ==="
avg_ms "astx --help"              "$BIN/astx" --help
avg_ms "confx --help"             "$BIN/confx" --help
avg_ms "diffx --help"             "$BIN/diffx" --help
avg_ms "idx --help"               "$BIN/idx" --help
avg_ms "procx -o json"            "$BIN/procx" -o json
avg_ms "aiux lx --help"           "$BIN/aiux" lx --help

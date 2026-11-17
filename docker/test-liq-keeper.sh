#!/bin/sh
set -eu

keeper_script=${1:-docker/liq-keeper.sh}
test_root=$(mktemp -d)
keeper_pid=""
cleanup() {
  if [ -n "$keeper_pid" ]; then
    kill -TERM "$keeper_pid" 2>/dev/null || true
    wait "$keeper_pid" 2>/dev/null || true
  fi
  rm -rf -- "${test_root:?}"
}
trap cleanup EXIT INT TERM
mkdir -p "$test_root/bin" "$test_root/conf/test"
export FAKE_LIQ_ROOT="$test_root"
cat > "$test_root/bin/liquidsoap" <<'SH'
#!/bin/sh
echo launch >> "$FAKE_LIQ_ROOT/launches"
if [ -f "$FAKE_LIQ_ROOT/steady" ]; then exec sleep 30; fi
sleep 0.02
exit 1
SH
chmod +x "$test_root/bin/liquidsoap"
touch "$test_root/conf/test/station.liq" "$test_root/conf/test/ON"
PATH="$test_root/bin:$PATH" LIQ_CONF_DIR="$test_root/conf" LIQ_POLL_SECS=0.15 LIQ_CRASH_LIMIT=3 sh "$keeper_script" &
keeper_pid=$!

wait_for_file() {
  n=0
  while [ ! -f "$1" ]; do
    n=$((n + 1))
    [ "$n" -le 100 ] || { echo "timed out waiting for $1" >&2; exit 1; }
    sleep 0.05
  done
}
wait_for_file "$test_root/conf/test/.failed"
sleep 0.3
[ "$(wc -l < "$test_root/launches")" -eq 3 ]
[ ! -f "$test_root/conf/test/.pid" ]

rm "$test_root/conf/test/ON"
sleep 0.3
[ ! -f "$test_root/conf/test/.failed" ]
touch "$test_root/steady" "$test_root/conf/test/ON"
n=0
while [ -f "$test_root/conf/test/.failed" ] || [ -f "$test_root/conf/test/.crash" ] || [ ! -f "$test_root/conf/test/.pid" ]; do
  n=$((n + 1))
  [ "$n" -le 100 ] || { echo 'stable restart did not recover' >&2; exit 1; }
  sleep 0.05
done
[ ! -f "$test_root/conf/test/.crash" ]
[ "$(wc -l < "$test_root/launches")" -eq 4 ]
echo 'keeper: retry limit and stable restart passed'

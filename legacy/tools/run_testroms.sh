#!/bin/bash
# ---------------------------------------------------------------------------
# Run the outside world's test ROMs.
#
# Every other test in this project compares the emulator against itself. This
# is the only one that asks whether it is *right*.
#
#   ./tools/run_testroms.sh
#
# The ROMs are not in the repository. Get them from
# https://github.com/christopherpow/nes-test-roms and put nestest in
# packages/fc-core/tests/data/testroms/ as described in the README.
# ---------------------------------------------------------------------------
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA="$ROOT/packages/fc-core/tests/data/testroms"

if [ ! -f "$DATA/nestest.nes" ]; then
    echo "no test ROMs in $DATA" >&2
    echo "see the README: they are copyrighted, and they are not committed" >&2
    exit 2
fi

echo "=== nestest ==="
"$ROOT/build/fc_testrom" "$DATA/nestest.nes" --nestest 9000 > /tmp/nestest_ours.log 2>/dev/null
python3 "$ROOT/tools/compare_nestest.py" "$DATA/nestest.log" /tmp/nestest_ours.log
nestest_result=$?

echo
echo "=== blargg ==="
if [ ! -d "$DATA/blargg" ]; then
    echo "  no blargg ROMs; skipping"
else
    pass=0; other=0
    for rom in "$DATA"/blargg/*.nes; do
        name="$(basename "$rom" .nes)"
        if "$ROOT/build/fc_testrom" "$rom" --console 900 2>/dev/null | grep -q PASSED; then
            printf '  %-6s %s\n' PASS "$name"
            pass=$((pass + 1))
        else
            printf '  %-6s %s\n' "----" "$name"
            other=$((other + 1))
        fi
    done
    echo "  $pass passed, $other did not"
    echo
    echo "  The harness for these is not finished: the ROMs write their"
    echo "  signature and their text, but the status byte never leaves the"
    echo "  signature, so nothing here is a result yet. Do not read this list"
    echo "  as the emulator failing the tests."
fi

exit $nestest_result

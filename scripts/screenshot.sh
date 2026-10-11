#!/usr/bin/env bash
# Screenshot the app on a private X server (Xvfb), driven by a fake
# nanoKONTROL2 and a silent test stream. Nothing appears on your desktop and
# only the test stream's volume is touched.
#
#   scripts/screenshot.sh OUT_DIR [tab...]     tabs: mixer controller buttons profiles settings log doctor
#
# Env: SIZE=1440x900 (screen size), KEEP=1 (leave the app running on :77),
#      PRE_SHOT="xdotool ..." (run on the private display before each shot).
set -euo pipefail

cd "$(dirname "$0")/.."
OUT=${1:?usage: screenshot.sh OUT_DIR [tab...]}
shift || true
TABS=("${@:-mixer}")
SIZE=${SIZE:-1440x900}
DISP=:77
WORK=$(mktemp -d)
mkdir -p "$OUT"

cargo build --quiet --bin slidewire --example fake_nano

pids=()
cleanup() {
    for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
    rm -rf "$WORK"
}
trap cleanup EXIT

Xvfb $DISP -screen 0 "${SIZE}x24" -nolisten tcp >/dev/null 2>&1 &
pids+=($!)
paplay --raw --client-name=TestTone /dev/zero >/dev/null 2>&1 &
pids+=($!)
mkfifo "$WORK/nano"
export NANO="$WORK/nano"   # PRE_SHOT can send fake controller input: echo "b 59" >"$NANO"
sleep 1000 >"$WORK/nano" &
pids+=($!)
./target/debug/examples/fake_nano <"$WORK/nano" >"$WORK/nano.log" 2>&1 &
pids+=($!)
sleep 1

# Test profile: the nanoKONTROL2 layout on the fake device, with only the
# test stream assigned.
CFG="$WORK/cfg"
mkdir -p "$CFG/profiles"
# Let the app write its default profile with a short run, then adjust it.
env -u WAYLAND_DISPLAY DISPLAY=$DISP SLIDEWIRE_CONFIG="$CFG" SLIDEWIRE_INSTANCE=shot \
    timeout 3 ./target/debug/slidewire --minimized >/dev/null 2>&1 || true
python3 - "$CFG" <<'EOF'
import json, glob, sys
cfg = sys.argv[1]
path = glob.glob(cfg + "/profiles/*.json")[0]
p = json.load(open(path))
p["input"] = p["output"] = "FakeNano"
ids = {g["name"]: gid for gid, g in p["groups"].items()}
tone = {"type": "app", "m": {"pattern": "testtone"}, "label": "Test tone"}
p["assignments"] = {
    ids["Fader 1"]: {"targets": [tone]},
    ids["Knob 1"]: {"targets": [tone]},
    ids["Fader 3"]: {"targets": [{"type": "defaultOutput"}]},
}
# Bank 2: the test tone on fader 2.
p["banks"] = [{ids["Fader 2"]: {"targets": [tone]}}]
json.dump(p, open(path, "w"), indent=1)
EOF

for tab in "${TABS[@]}"; do
    env -u WAYLAND_DISPLAY DISPLAY=$DISP SLIDEWIRE_CONFIG="$CFG" SLIDEWIRE_INSTANCE=shot SLIDEWIRE_TAB="$tab" \
        ./target/debug/slidewire >"$WORK/app.log" 2>&1 &
    app=$!
    sleep 3.5
    # Some activity: fader 1 to 70%, knob 1, mute strip 1 twice -> unmuted, M2 lights nothing.
    # Fader 1 sweeps across the volume (soft takeover picks it up); knob 1 jumps, so it waits.
    { echo "f 1 0"; echo "f 1 127"; echo "f 1 90"; echo "k 1 60"; echo "f 2 40"; echo "b 48"; } >"$WORK/nano"
    sleep 0.25
    DISPLAY=$DISP xdotool search --name "Slidewire" windowsize $(( ${SIZE%x*} - 40 )) $(( ${SIZE#*x} - 40 )) windowmove 20 20 2>/dev/null || true
    sleep 0.8
    { echo "k 3 100"; } >"$WORK/nano"
    sleep 0.6
    # PRE_SHOT: commands to run on the private display first (e.g. xdotool drags).
    if [[ -n "${PRE_SHOT:-}" ]]; then
        DISPLAY=$DISP bash -c "$PRE_SHOT"
        sleep 0.5
    fi
    DISPLAY=$DISP import -window root "$OUT/$tab.png"
    echo "b 48" >"$WORK/nano"
    if [[ "${KEEP:-}" == 1 ]]; then
        echo "app left running on $DISP (pid $app); press enter to stop"
        read -r
    fi
    kill $app 2>/dev/null || true
    wait $app 2>/dev/null || true
done
grep -v "^$" "$WORK/app.log" | tail -5 || true
echo "screenshots in $OUT"

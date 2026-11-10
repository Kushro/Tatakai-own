#!/bin/bash
# usage: mute.sh <pid> [seconds=900]
# Mutes, for <seconds>, only the PulseAudio/PipeWire sink inputs that belong to process <pid> or its descendants
# (the Tatakai Next instance under test; libmpv plays inside it). Never touches other apps' streams (a standalone mpv
# the user is watching, browsers, games): matching by name ("mpv", "tatakai-next") muted the user's own mpv once.
pid="$1"; secs="${2:-900}"
[ -n "$pid" ] || { echo "usage: mute.sh <pid> [seconds]" >&2; exit 2; }
end=$((SECONDS+secs))
while [ $SECONDS -lt $end ] && kill -0 "$pid" 2>/dev/null; do
  pids=" $pid $(pgrep -d ' ' -P "$pid") "
  pactl -f json list sink-inputs 2>/dev/null | PIDS="$pids" python3 -I -c '
import json,os,sys,subprocess
mine=set(os.environ["PIDS"].split())
for s in json.load(sys.stdin):
  p=s.get("properties",{})
  # libmpv inside tatakai-next does not report a pid to PipeWire; its stream is titled "Tatakai" (a standalone mpv
  # the user runs is titled after its file), so that title also counts as ours.
  ours = str(p.get("application.process.id","")) in mine or (p.get("application.name")=="mpv" and p.get("media.name","").startswith("Tatakai"))
  if ours and not s.get("mute"):
    subprocess.run(["pactl","set-sink-input-mute",str(s["index"]),"1"]); print("muted",s["index"],flush=True)'
  sleep 0.2
done

#!/bin/bash
end=$((SECONDS+900))
while [ $SECONDS -lt $end ]; do
  pactl -f json list sink-inputs 2>/dev/null | python3 -c '
import json,sys,subprocess
for s in json.load(sys.stdin):
  p=s.get("properties",{})
  if p.get("application.name","")=="mpv" or "tatakai-next" in (p.get("application.process.binary","")+p.get("application.name","")).lower() or p.get("application.process.binary")=="tatakai-next":
    if not s.get("mute"): subprocess.run(["pactl","set-sink-input-mute",str(s["index"]),"1"]); print("muted",s["index"],flush=True)'
  sleep 0.2
done

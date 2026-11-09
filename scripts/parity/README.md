# Parity harness

Scripts used by the 2026-11-08 parity audit (`docs/redesign/parity/`) and by the parity plan (`docs/redesign/14-parity-plan.md`, §5). Node >= 22 (built-in `WebSocket`/`fetch`), no dependencies.

| Script | What |
|---|---|
| `cdp.mjs <port> <outdir> name=<route>[@ms] ...` | Classic Electron app relaunched with `--remote-debugging-port=<port>`: navigates by hash route and saves full-page JPEGs. Steps: `name=/route`, `name=CLICK:<text>`, `name=EVAL:<js>`, `name=SHOT`. |
| `wk.mjs <outdir> name=<step>[@ms] ...` | Tatakai Next launched with `WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:9334`: clicks by visible text, evaluates JS and saves PNGs with `Page.snapshotRect`. `name=FULL` expands scroll containers to capture the whole page; `name=SHOT` captures the viewport; `_` as name skips the capture. |
| `mute.sh` | Mutes every PulseAudio sink input of `tatakai-next` / mpv for 15 minutes, so playback tests stay silent. |
| `move.sh <resourceClass>` | KDE/KWin only: moves a window to monitor DP-2 at 1290x1080 and raises it. |

The mpv video is drawn under the web view, so `Page.snapshotRect` never contains it; capture the player from the desktop instead.

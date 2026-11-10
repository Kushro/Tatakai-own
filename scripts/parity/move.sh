#!/bin/bash
# usage: move.sh <resourceClass>  -> moves window to DP-2 left area, unminimized
S=$(cd "$(dirname "$0")" && pwd); cls="$1"  # absolute: KWin does not resolve relative script paths
cat > "$S/move.js" <<JS
var ws = workspace.windowList();
var out = null; var os = workspace.screens; for (var j=0;j<os.length;j++){ if(os[j].name=="DP-2") out=os[j]; }
for (var i=0;i<ws.length;i++){var w=ws[i]; if(w.resourceClass!="$cls"||!w.normalWindow) continue;
 if (w.fullScreen) w.fullScreen=false; w.setMaximize(false,false); w.minimized=false; if(out) workspace.sendClientToScreen(w,out);
 w.frameGeometry={x:0,y:72,width:1290,height:1080}; workspace.activeWindow=w; print("TKMOVE|"+w.caption+"|"+w.output.name);}
JS
id=$(qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$S/move.js" tkmove); qdbus org.kde.KWin /Scripting/Script$id org.kde.kwin.Script.run >/dev/null; sleep 0.5; qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript tkmove >/dev/null
journalctl --user --since "-5s" | grep TKMOVE

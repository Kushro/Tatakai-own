; ─────────────────────────────────────────────────────────────────────────────
;  Tatakai — one-click NSIS installer (Linear / Figma-style, dark + branded)
;  Loaded by electron-builder via the `nsis.include` config option.
;
;  We ship electron-builder's ONE-CLICK installer (nsis.oneClick=true): a single
;  branded window showing the Tatakai icon + a progress bar, installed per-user
;  (no UAC prompt), then auto-launched on finish. That's the clean modern flow
;  Linear / Figma / Discord ship — not the dated gray multi-page assisted wizard.
;
;  This file adds the visual polish on top of that minimal flow:
;    1. A branded full-bleed splash (build/installerSplash.bmp) shown for ~1.8s at
;       launch via the AdvSplash plugin — the "designed" first impression.
;    2. A dark theme for the progress window (MUI_BGCOLOR / MUI_TEXTCOLOR), set at
;       top level BEFORE MUI2.nsh is included (electron-builder prepends this file
;       ahead of installer.nsi, so these !defines land in time).
;    3. A cleaner window caption + branding line (customHeader).
;    4. The install-time cloudflared fetch (customInstall).
;
;  electron-builder wires the window/shortcut/taskbar icons straight from the
;  `nsis` block (installerIcon / installerHeaderIcon / uninstallerIcon) — do NOT
;  redefine MUI_ICON here or the duplicate `!define` will fail the compile.
;
;  Every macro below is best-effort by design: on any failure the install still
;  succeeds. Nothing here can fail the NSIS compile.
; ─────────────────────────────────────────────────────────────────────────────

; ── Dark theme ─────────────────────────────────────────────────────────────────
; These recolor the MUI install page (background + static text). The progress bar
; itself is a native Win32 control and keeps the system accent. Set as top-level
; !defines so they are seen before installer.nsi's `!include "MUI2.nsh"` — which is
; why they must live outside any macro. Colors mirror the app's brand tokens
; (src/index.css / desktop/splash.html): near-black bg, near-white text.
!define MUI_BGCOLOR   "08060A"
!define MUI_TEXTCOLOR "F7F3F5"

; ── Window chrome ────────────────────────────────────────────────────────────
; customHeader is inserted at top-level script scope (installer.nsi), so directives
; like Caption / BrandingText are valid here. Last-writer-wins overrides the
; defaults common.nsh set ("Tatakai Setup" / "Tatakai ${VERSION}").
!macro customHeader
  Caption "Tatakai"
  BrandingText "Tatakai ${VERSION} — Otaku Community"
!macroend

; ── Branded splash ───────────────────────────────────────────────────────────
; customInit runs inside .onInit (before the install window is shown), so a blocking
; splash here reads as "splash first, then the installer" — exactly the flow we want.
; AdvSplash ships in electron-builder's bundled NSIS distribution and only accepts an
; uncompressed 24-bit BMP, which scripts/generate-installer-assets.cjs produces at
; build/installerSplash.bmp. ${PROJECT_DIR} is a define electron-builder passes to
; makensis (the absolute project root), so the File path resolves regardless of the
; makensis working directory.
;
; Best-effort: if the BMP is missing or the plugin can't show it, AdvSplash pushes a
; negative code we simply Pop and ignore — the install proceeds untouched.
!macro customInit
  InitPluginsDir
  File "/oname=$PLUGINSDIR\tatakai-splash.bmp" "${PROJECT_DIR}\build\installerSplash.bmp"
  ; show <displayMs> <fadeInMs> <fadeOutMs> <transparentColor:-1=none> "<bmpPathNoExt>"
  advsplash::show 1800 500 400 -1 "$PLUGINSDIR\tatakai-splash"
  Pop $0
!macroend

; cloudflared is NOT downloaded at install time: the installer has no way to
; verify it. electron:build bundles the pinned, SHA-256-verified binary via
; extraResources (scripts/fetch-cloudflared.cjs); if it is ever missing,
; share-tunnel.cjs auto-installs the same pinned, verified release on first use.

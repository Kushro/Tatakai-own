#!/usr/bin/env bash
# Builds Tatakai_Next-<version>-x86_64.AppImage with linuxdeploy + linuxdeploy-plugin-gtk.
#
# Layout inside the AppDir is the one of install-tree.sh (usr/lib/tatakai-next/...), plus:
#   - libmpv and every other shared library the binary links (deployed by linuxdeploy from the build host;
#     glibc, libGL/EGL, fontconfig and the rest of linuxdeploy's excludelist come from the target system),
#   - GTK 3 modules, pixbuf loaders and schemas (gtk plugin),
#   - WebKitGTK's helper processes and injected bundle, with libwebkit2gtk patched to find them relative to
#     the working directory (AppRun runs the app from $APPDIR/usr),
#   - the glib-networking TLS module for the web view.
#
# Build host: the oldest distribution you want to support (CI uses Ubuntu 24.04: libmpv 2.x needs mpv >= 0.35).
# Needs: cargo (unless --bin), curl, patchelf, pkg-config, perl, webkit2gtk-4.1 + gtk3 + libmpv dev packages,
# glib-networking. linuxdeploy and its gtk plugin are downloaded into the tools cache unless $LINUXDEPLOY and
# $LINUXDEPLOY_PLUGIN_GTK point at local copies.
#
# Usage: build-appimage.sh [--bin <built tatakai-next>] [--out <dir>] [--deps <fetch-deps dest>]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app="$(cd "$here/../.." && pwd)"
id="app.tatakai.own.next"

bin=""
out="$app/target/appimage"
deps="$app/packaging/deps"
while [ $# -gt 0 ]; do
    case "$1" in
        --bin) bin="$2"; shift 2 ;;
        --out) out="$2"; shift 2 ;;
        --deps) deps="$2"; shift 2 ;;
        -h|--help) sed -n '2,19p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

for tool in curl patchelf pkg-config perl; do
    command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }
done

version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$app/Cargo.toml" | head -n1)"
[ -n "$version" ] || { echo "cannot read the workspace version from Cargo.toml" >&2; exit 1; }

if [ -z "$bin" ]; then
    (cd "$app" && cargo build -p tk-app --release --locked)
    bin="$app/target/release/tatakai-next"
fi
[ -f "$bin" ] || { echo "binary not found: $bin" >&2; exit 1; }

# A fresh AppDir per run; nothing is deleted (CI runners are ephemeral, locally remove old ones by hand).
mkdir -p "$out"
work="$(mktemp -d "$out/build.XXXXXX")"
appdir="$work/AppDir"
mkdir -p "$appdir"
echo "AppDir: $appdir"

"$here/install-tree.sh" --bin "$bin" --root "$appdir" --prefix /usr --deps "$deps"

# --- WebKitGTK helpers -------------------------------------------------------------------------------------
webkit_libdir="$(pkg-config --variable=libdir webkit2gtk-4.1)"
helpers="$webkit_libdir/webkit2gtk-4.1"
case "$helpers" in
    /usr/lib/*) ;;
    *) echo "unexpected WebKitGTK helper directory $helpers (the same-length path patch needs /usr/...)" >&2; exit 1 ;;
esac
[ -x "$helpers/WebKitWebProcess" ] || { echo "WebKitWebProcess not found in $helpers" >&2; exit 1; }
mkdir -p "$appdir$(dirname "$helpers")"
cp -a "$helpers" "$appdir$helpers"

# --- TLS for libsoup (glib-networking) ---------------------------------------------------------------------
gio_modules="$(pkg-config --variable=giomoduledir gio-2.0)"
[ -f "$gio_modules/libgiognutls.so" ] || { echo "glib-networking is missing ($gio_modules/libgiognutls.so)" >&2; exit 1; }
install -Dm0755 "$gio_modules/libgiognutls.so" "$appdir/usr/lib/gio/modules/libgiognutls.so"

# --- linuxdeploy ---------------------------------------------------------------------------------------------
tools="$app/target/appimage-tools"
mkdir -p "$tools"
fetch_tool() {
    local name="$1" url="$2"
    if [ ! -x "$tools/$name" ]; then
        curl -fsSL --retry 3 -o "$tools/$name.part" "$url"
        chmod +x "$tools/$name.part"
        mv -f -- "$tools/$name.part" "$tools/$name"
    fi
    printf '%s\n' "$tools/$name"
}
linuxdeploy="${LINUXDEPLOY:-$(fetch_tool linuxdeploy-x86_64.AppImage \
    https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage)}"
plugin_gtk="${LINUXDEPLOY_PLUGIN_GTK:-$(fetch_tool linuxdeploy-plugin-gtk.sh \
    https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh)}"
# linuxdeploy finds plugins on PATH by their name.
plugin_dir="$work/plugins"
mkdir -p "$plugin_dir"
ln -sfn "$plugin_gtk" "$plugin_dir/linuxdeploy-plugin-gtk.sh"
export PATH="$plugin_dir:$PATH"
# No FUSE on CI runners (and inside containers).
export APPIMAGE_EXTRACT_AND_RUN=1
export DEPLOY_GTK_VERSION=3
export ARCH=x86_64

icon="$work/$id.png"
cp "$app/../../resources/icon-256.png" "$icon"

"$linuxdeploy" --appdir "$appdir" \
    --desktop-file "$appdir/usr/share/applications/$id.desktop" \
    --icon-file "$icon" \
    --custom-apprun "$here/AppRun" \
    --deploy-deps-only "$appdir/usr/lib/tatakai-next/tatakai-next" \
    --deploy-deps-only "$appdir$helpers" \
    --deploy-deps-only "$appdir$helpers/injected-bundle" \
    --deploy-deps-only "$appdir/usr/lib/gio/modules" \
    --plugin gtk

# --- Point WebKit at the bundled helpers ---------------------------------------------------------------------
# libwebkit2gtk embeds the absolute helper directory; "/usr" -> "././" keeps the string length, so the path
# becomes relative to the working directory, which AppRun sets to $APPDIR/usr.
relative="././${helpers#/usr}"
patched=0
while IFS= read -r -d '' lib; do
    if grep -qaF "$helpers" "$lib"; then
        perl -0777 -pi -e 'BEGIN { $o = shift; $n = shift } s/\Q$o\E/$n/g' "$helpers" "$relative" "$lib"
        patched=$((patched + 1))
    fi
done < <(find "$appdir/usr/lib" -maxdepth 1 -type f -name 'libwebkit2gtk-4.1.so*' -print0)
[ "$patched" -gt 0 ] || { echo "libwebkit2gtk-4.1 was not deployed or does not embed $helpers" >&2; exit 1; }

# Library search paths for the files linuxdeploy did not place in usr/bin or usr/lib.
# origin_up <levels> -> $ORIGIN followed by <levels> "/..", i.e. usr/lib as seen from <levels> directories below.
origin_up() {
    # shellcheck disable=SC2016
    local r='$ORIGIN' i
    for ((i = 0; i < $1; i++)); do r="$r/.."; done
    printf '%s\n' "$r"
}
rel_helpers="${helpers#/usr/lib/}"            # webkit2gtk-4.1 or x86_64-linux-gnu/webkit2gtk-4.1
slashes="${rel_helpers//[^\/]/}"
helpers_depth=$(( ${#slashes} + 1 ))
patchelf --set-rpath "$(origin_up 1)" "$appdir/usr/lib/tatakai-next/tatakai-next"
for f in "$appdir$helpers"/*; do
    if [ -f "$f" ] && head -c4 "$f" | grep -q 'ELF'; then
        patchelf --set-rpath "$(origin_up "$helpers_depth")" "$f"
    fi
done
for f in "$appdir$helpers"/injected-bundle/*.so; do
    if [ -f "$f" ]; then patchelf --set-rpath "$(origin_up $((helpers_depth + 1)))" "$f"; fi
done
patchelf --set-rpath "$(origin_up 2)" "$appdir/usr/lib/gio/modules/libgiognutls.so"

# --- Pack ----------------------------------------------------------------------------------------------------
target="$out/Tatakai_Next-$version-x86_64.AppImage"
export LDAI_OUTPUT="$target" OUTPUT="$target" LINUXDEPLOY_OUTPUT_VERSION="$version"
"$linuxdeploy" --appdir "$appdir" --output appimage
echo "AppImage: $target"

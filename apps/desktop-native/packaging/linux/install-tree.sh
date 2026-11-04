#!/usr/bin/env bash
# Installs a built tatakai-next into a staging root with the "app folder" layout of docs/redesign/09 section 8.
# Shared by the PKGBUILD (root = $pkgdir) and build-appimage.sh (root = the AppDir):
#
#   <prefix>/lib/tatakai-next/tatakai-next                     the binary (current_exe resolves the symlink)
#   <prefix>/lib/tatakai-next/assets/fonts/*                   paths.rs::fonts_dir
#   <prefix>/lib/tatakai-next/shaders/*.glsl                   paths.rs::shaders_dir (when fetched)
#   <prefix>/lib/tatakai-next/mpv-anime-build/<zip>            anime_build.rs::zip_path (+ SHA256SUMS, LICENSE)
#   <prefix>/lib/tatakai-next/ffmpeg/bin/{ffmpeg,ffprobe}      -> /usr/bin (paths.rs::ffmpeg_path, Link sessions)
#   <prefix>/bin/tatakai-next -> ../lib/tatakai-next/tatakai-next
#   <prefix>/share/{applications,metainfo,icons/hicolor}       desktop entry, AppStream, icons
#   <prefix>/share/licenses/tatakai-next/                      AGPL text + third-party notices and licenses
#
# Usage: install-tree.sh --bin <built binary> --root <staging root> [--prefix /usr] [--deps <fetch-deps dest>]
#                        [--no-ffmpeg-links]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app="$(cd "$here/../.." && pwd)"            # apps/desktop-native
repo="$(cd "$app/../.." && pwd)"            # repository root

bin=""
root=""
prefix="/usr"
deps="$app/packaging/deps"
ffmpeg_links=1

while [ $# -gt 0 ]; do
    case "$1" in
        --bin) bin="$2"; shift 2 ;;
        --root) root="$2"; shift 2 ;;
        --prefix) prefix="$2"; shift 2 ;;
        --deps) deps="$2"; shift 2 ;;
        --no-ffmpeg-links) ffmpeg_links=0; shift ;;
        -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
[ -n "$bin" ] && [ -f "$bin" ] || { echo "--bin <path to the built tatakai-next> is required" >&2; exit 2; }
[ -n "$root" ] || { echo "--root <staging root> is required" >&2; exit 2; }

id="app.tatakai.own.next"
base="$root$prefix"
lib="$base/lib/tatakai-next"
anime="$app/vendor/mpv-anime-build"
zip_name="mpv-anime-build-v5.6.zip"

install -Dm0755 "$bin" "$lib/tatakai-next"
install -d "$base/bin"
ln -sfn ../lib/tatakai-next/tatakai-next "$base/bin/tatakai-next"

# Fonts: the ones committed in tk-ui plus whatever fetch-deps.sh downloaded (verified).
install -d "$lib/assets/fonts"
for f in "$app"/crates/tk-ui/assets/fonts/*; do
    if [ -f "$f" ]; then install -m0644 "$f" "$lib/assets/fonts/"; fi
done
if [ -d "$deps/fonts" ]; then
    for f in "$deps"/fonts/*; do
        if [ -f "$f" ]; then install -m0644 "$f" "$lib/assets/fonts/"; fi
    done
fi

# Shaders are optional: without them the Anime4K / FSR presets report the missing files.
if [ -d "$deps/shaders" ]; then
    install -d "$lib/shaders"
    for f in "$deps"/shaders/*; do
        if [ -f "$f" ]; then install -m0644 "$f" "$lib/shaders/"; fi
    done
else
    echo "note: $deps/shaders not found (run fetch-deps.sh); shipping without Anime4K/FSR shaders" >&2
fi

# Vendored mpv-anime-build release: the app verifies the zip against its compiled-in SHA256SUMS and extracts it
# into <data>/mpv-anime-build/5.6 on first use, so the zip is shipped as-is.
(cd "$anime" && sha256sum -c --quiet SHA256SUMS)
install -Dm0644 "$anime/$zip_name" "$lib/mpv-anime-build/$zip_name"
install -m0644 "$anime/SHA256SUMS" "$anime/LICENSE" "$anime/README.md" "$lib/mpv-anime-build/"

# Tatakai Link looks for <exe>/ffmpeg/bin/ffmpeg; on Linux that is the system ffmpeg.
if [ "$ffmpeg_links" -eq 1 ]; then
    install -d "$lib/ffmpeg/bin"
    ln -sfn /usr/bin/ffmpeg "$lib/ffmpeg/bin/ffmpeg"
    ln -sfn /usr/bin/ffprobe "$lib/ffmpeg/bin/ffprobe"
fi

install -Dm0644 "$here/$id.desktop" "$base/share/applications/$id.desktop"
install -Dm0644 "$here/$id.metainfo.xml" "$base/share/metainfo/$id.metainfo.xml"
for size in 16 32 48 64 128 256 512; do
    install -Dm0644 "$repo/resources/icon-$size.png" "$base/share/icons/hicolor/${size}x${size}/apps/$id.png"
done

lic="$base/share/licenses/tatakai-next"
install -Dm0644 "$app/LICENSE" "$lic/LICENSE"
install -m0644 "$app/THIRD_PARTY_NOTICES.md" "$lic/THIRD_PARTY_NOTICES.md"
install -m0644 "$anime/LICENSE" "$lic/mpv-anime-build-LICENSE"
for f in "$lib"/assets/fonts/OFL-*.txt; do
    if [ -f "$f" ]; then install -m0644 "$f" "$lic/"; fi
done
echo "Installed tatakai-next into $root (prefix $prefix)"

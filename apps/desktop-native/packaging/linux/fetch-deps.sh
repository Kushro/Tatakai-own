#!/usr/bin/env bash
# Linux counterpart of packaging/fetch-deps.ps1: downloads the runtime pieces that are NOT system packages on
# Linux and verifies every file against the same SHA-256 locks. libmpv and ffmpeg come from the distribution
# (Arch: `mpv`, `ffmpeg`; Ubuntu: `libmpv2`, `ffmpeg`), so only shaders and fonts are fetched here.
#
#   deps/shaders/*.glsl (+ anime4k.lock.json)   anime4k.lock.json, fsr.lock.json
#   deps/fonts/*.woff2 + OFL texts               fonts.lock.json
#
# A lock entry whose sha256 is null is skipped with a warning (never installed unverified). --pin downloads
# those entries into the cache and prints their hashes; it writes nothing into the lock: review and paste.
#
# Usage: fetch-deps.sh [--only anime4k,fsr,fonts] [--dest DIR] [--cache DIR] [--anime4k-source DIR] [--pin]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
packaging="$(cd "$here/.." && pwd)"
repo_fonts="$(cd "$packaging/../crates/tk-ui/assets/fonts" && pwd)"

only="anime4k,fsr,fonts"
dest="$packaging/deps"
cache=""
anime4k_source=""
pin=0

while [ $# -gt 0 ]; do
    case "$1" in
        --only) only="$2"; shift 2 ;;
        --dest) dest="$2"; shift 2 ;;
        --cache) cache="$2"; shift 2 ;;
        --anime4k-source) anime4k_source="$2"; shift 2 ;;
        --pin) pin=1; shift ;;
        -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
[ -n "$cache" ] || cache="$dest/.cache"

for tool in curl sha256sum python3; do
    command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }
done

# Prints "<path>\t<url>\t<sha256 or empty>" for each file of a lock.
lock_entries() {
    python3 -I -c '
import json, sys
for f in json.load(open(sys.argv[1], encoding="utf-8"))["files"]:
    print("\t".join([f["path"], f["url"], (f.get("sha256") or "").lower()]))
' "$packaging/$1"
}

sha_of() { sha256sum "$1" | cut -d' ' -f1; }

# fetch_verified <url> <sha256> <label> -> prints the cached file path (empty when skipped)
fetch_verified() {
    local url="$1" sha="$2" label="$3" name cached
    name="$(basename "${url%%\?*}")"
    mkdir -p "$cache"
    if [ -z "$sha" ]; then
        if [ "$pin" -eq 1 ]; then
            cached="$cache/pin-$name"
            curl -fsSL --retry 3 -o "$cached" "$url"
            echo "  PIN $label sha256 $(sha_of "$cached")  (review the file, then paste the hash into the lock)" >&2
        else
            echo "  warning: $label has no pinned SHA-256 in its lock; skipped (run with --pin to get one)" >&2
        fi
        return 0
    fi
    cached="$cache/$sha-$name"
    if [ ! -f "$cached" ] || [ "$(sha_of "$cached")" != "$sha" ]; then
        curl -fsSL --retry 3 -o "$cached.part" "$url"
        local actual
        actual="$(sha_of "$cached.part")"
        if [ "$actual" != "$sha" ]; then
            rm -f -- "$cached.part"
            echo "SHA-256 mismatch for $label: expected $sha, got $actual" >&2
            return 1
        fi
        mv -f -- "$cached.part" "$cached"
    fi
    printf '%s\n' "$cached"
}

install_shaders() {
    local lock="$1" label="$2" local_source="$3" out="$dest/shaders" path url sha src
    echo "== $label"
    mkdir -p "$out"
    while IFS=$'\t' read -r path url sha; do
        if [ -n "$local_source" ] && [ -f "$local_source/$path" ]; then
            if [ -n "$sha" ] && [ "$(sha_of "$local_source/$path")" != "$sha" ]; then
                echo "SHA-256 mismatch for local $path" >&2
                return 1
            fi
            install -m 0644 "$local_source/$path" "$out/$path"
            echo "  copied $path (local, verified)"
            continue
        fi
        src="$(fetch_verified "$url" "$sha" "$path")"
        [ -n "$src" ] || continue
        install -m 0644 "$src" "$out/$path"
        echo "  $path"
    done < <(lock_entries "$lock")
    # tk-app/src/paths.rs::anime4k_lock looks for the lock next to the shaders.
    [ "$lock" != "anime4k.lock.json" ] || install -m 0644 "$packaging/anime4k.lock.json" "$out/anime4k.lock.json"
}

install_fonts() {
    local out="$dest/fonts" path url sha src n
    echo "== fonts"
    mkdir -p "$out"
    while IFS=$'\t' read -r path url sha; do
        src="$(fetch_verified "$url" "$sha" "$path")"
        [ -n "$src" ] || continue
        install -m 0644 "$src" "$out/$path"
        echo "  $path"
    done < <(lock_entries fonts.lock.json)
    # JetBrains Mono ships in the repo; copy it next to the downloaded ones so one folder is complete.
    for n in JetBrainsMono.woff2 OFL-JetBrainsMono.txt; do
        install -m 0644 "$repo_fonts/$n" "$out/$n"
    done
}

mkdir -p "$dest"
IFS=',' read -r -a parts <<< "$only"
for what in "${parts[@]}"; do
    case "$what" in
        anime4k) install_shaders anime4k.lock.json "Anime4K shaders" "$anime4k_source" ;;
        fsr) install_shaders fsr.lock.json "FSR shader" "" ;;
        fonts) install_fonts ;;
        *) echo "unknown component: $what (expected anime4k, fsr, fonts)" >&2; exit 2 ;;
    esac
done
echo "Done. Output: $dest"

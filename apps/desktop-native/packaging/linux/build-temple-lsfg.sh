#!/usr/bin/env bash
# Builds the temple-lsfg helper (native/temple-lsfg, C++/Vulkan) that Tatakai Link runs for the LSFG profiles.
# Needs cmake (3.20+) and a C++20 compiler (GCC 11+ / Clang 14+); uses ninja when it is installed. No Vulkan
# headers or libvulkan-dev: they are bundled, and libvulkan.so.1 is only loaded at run time.
# Prints the path of the built binary on the last line of stdout.
#
# Usage: build-temple-lsfg.sh [--out <build dir>]   (default: <app>/target/temple-lsfg)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app="$(cd "$here/../.." && pwd)"
src="$app/native/temple-lsfg"
out="$app/target/temple-lsfg"
while [ $# -gt 0 ]; do
    case "$1" in
        --out) out="$2"; shift 2 ;;
        -h|--help) sed -n '2,7p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
command -v cmake >/dev/null || { echo "cmake is required to build temple-lsfg" >&2; exit 1; }

generator=()
if command -v ninja >/dev/null; then generator=(-G Ninja); fi
cmake -S "$src" -B "$out" "${generator[@]}" -DCMAKE_BUILD_TYPE=Release >&2
cmake --build "$out" --parallel >&2
[ -x "$out/temple-lsfg" ] || { echo "temple-lsfg was not built in $out" >&2; exit 1; }
printf '%s\n' "$out/temple-lsfg"

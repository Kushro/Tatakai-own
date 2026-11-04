# mpv-anime-build (vendored)

Upstream: https://github.com/Chinna95P/mpv-anime-build, release v5.6 (2026-10-13), MIT (see LICENSE).
The release zip is kept unmodified; SHA256SUMS pins it. The app extracts it into its data directory
the first time the "Embedded enhanced" player mode is used.

To update: replace the zip with a newer release, regenerate SHA256SUMS (sha256sum <zip> > SHA256SUMS)
and update the version above and in the code that references the file name.

# pe-parse (vendored subset)

- Upstream: https://github.com/trailofbits/pe-parse
- Commit: 31ac5966503689d5693cd9fb520bd525a8710e17 (as pinned by lsfg-vk-android `thirdparty/pe-parse`)
- License: MIT (see `LICENSE`, Copyright (c) 2013 Andrew Ruef)

Copia sin modificaciones; ver design.md R13 (copy without modifications; frame generation).
Source mapping: `pe-parser-library/include/pe-parse/*` -> `include/pe-parse/`,
`pe-parser-library/src/{buffer.cpp,parse.cpp}` -> `src/`, root `LICENSE` -> `LICENSE`.
Not included: `unicode_codecvt.cpp`, `unicode_winapi.cpp`, tools, tests and bindings.

## Files copied

- `LICENSE`
- `include/pe-parse/nt-headers.h`
- `include/pe-parse/parse.h`
- `include/pe-parse/to_string.h`
- `src/buffer.cpp`
- `src/parse.cpp`

# Vendored: @material/material-color-utilities 0.4.0

Compiled ESM files copied unchanged from the npm tarball `@material/material-color-utilities@0.4.0`
(source: https://github.com/material-foundation/material-color-utilities, directory `typescript`),
minus `*.d.ts`, tests and source maps (the `sourceMappingURL` comments were removed).
License: Apache-2.0, see `LICENSE` in this directory.

Vendored instead of added as an npm dependency so `node packages/tokens/generate.mjs` needs no `npm install`
(the repo-wide lockfile and `package.json` are not touched by the native redesign).

Local modification: a few relative imports lacked the `.js` extension required by Node ESM (e.g. in `dynamiccolor/color_spec_2025.js`); `.js` was appended.

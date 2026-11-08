# Source provenance

The extension server used to live in a separate repository (TatakaiServer-own,
previously the `tatakai-server` submodule) that carried a byte-for-byte snapshot
of app code under `shared/tatakai`. Since the server moved into this repository
(`server/`), nothing is copied any more: the build bundles the app sources in
place.

- `server/src/interpreter.ts` imports the Sora / Mangayomi interpreter adapters
  from `src/core/extensions/{sora,mangayomi,compatibility}`; esbuild bundles them
  into `server/dist/interpreter.mjs`.
- The dashboard (`server/ui`) imports the UI primitives from `src/components/ui`
  through the `@` alias (`@` -> `<repo>/src`).
- The Java compatibility runtime (Aniyomi / Mihon / CloudStream broker) is
  required in place from `desktop/runtime/compatibility`, with its bootstrap
  sources at `desktop/runtime/compatibility/aniyomi-java` and `shared/aniyomi`.

All of that code is covered by the repository's `LICENSE` (MPL-2.0), the same as
`server/LICENSE`.

Native runtime engines are fetched separately from pinned upstream locations.
Their artifact manifests identify repositories, versions, checksums and licenses.
Existing runtime preparation retains upstream license notices; this repository
does not bundle those downloaded third-party engines.

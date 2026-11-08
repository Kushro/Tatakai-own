# Shared source provenance

`shared/tatakai` contains a source snapshot from
[Tatakai](https://github.com/snozxyx/tatakai): Sora/Mangayomi interpreter adapters,
the compatibility runtime, native bootstrap source and the UI primitives used by
the dashboard. The snapshot preserves source layout and licensing; see
`shared/tatakai/LICENSE` and the repository's `LICENSE` (MPL-2.0).

The Aniyomi launcher has one packaging adjustment: it checks bundled Java source
under `dist/compatibility/source` instead of reading files from a parent checkout.
The build retains the source files referenced by its compiled integrity manifest.

Native runtime engines are fetched separately from pinned upstream locations.
Their artifact manifests identify repositories, versions, checksums and licenses.
Existing runtime preparation retains upstream license notices; this repository
does not bundle those downloaded third-party engines.

To refresh the snapshot from a local Tatakai checkout, run:

```sh
node scripts/sync-tatakai.mjs /path/to/TatakaiV6
npm run build
npm test
```

Normal installs/builds never read or require a parent Tatakai checkout.

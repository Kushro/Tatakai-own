# tk-ext - dependency notes (B5)

No new workspace dependency is required.

* `md5` / `sha1` for `require('crypto')` are implemented in `src/hashing.rs` (verified against RFC vectors and the contract
  fixture token) because `md-5`/`sha1` are not in the frozen workspace. If the integrator prefers crates:
  `md-5 = "0.10"`, `sha1 = "0.10"` (both already in the lockfile transitively).
* `regex` is not used: the compat analysis (`src/compat.rs`) is a hand-written scanner that mirrors `contracts/extension-host/compat.mjs`.
* rquickjs 0.9 caveat: `Runtime::execute_pending_job` returns a `JobException(Context)` built without a context reference, and
  dropping it corrupts the QuickJS GC (assert `p->ref_count > 0`). `runtime.rs::pump` uses `Ctx::execute_pending_job` + `catch()`.

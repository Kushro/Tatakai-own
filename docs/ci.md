# CI runners and disabled platforms

## Runner provider

Every workflow starts with a `runner` job (`.github/workflows/runner.yml`, a reusable
workflow) whose outputs `linux` and `windows` are the `runs-on` labels used by all other
jobs:

| Provider | linux | windows |
|----------|-------|---------|
| Avrea (default) | `avrea-ubuntu-24.04-4-vcpu` | `avrea-windows-2025-4-vcpu` |
| GitHub-hosted | `ubuntu-24.04` | `windows-2025` |

Avrea runners require the Avrea GitHub App to be installed on the repository
(`Kushro/Tatakai-own`); without it, Avrea jobs stay queued forever. Until it is
installed, set the repository variable `RUNNER_PROVIDER=github`.

Repository variable `RUNNER_PROVIDER` (Settings > Secrets and variables > Actions >
Variables):

- `avrea` or `github`: force that provider.
- unset or `auto`: Avrea, unless status.avrea.com reports a major outage or is
  unreachable, or the pull request comes from another repository.

Known quirks:

- `gh` cannot verify api.github.com on Avrea's Windows image (x509). Windows jobs never
  call `gh`: they upload workflow artifacts and the Linux `stage-windows` job stages them
  into the draft release.
- Avrea Ubuntu 24.04 means a glibc 2.39 floor for Linux builds (AppImage/deb).
- Windows jobs that use `shell: bash` assume Git Bash on the Avrea image.
- ARM entries of `translate-engine.yml` (`windows-11-arm`, `ubuntu-24.04-arm`) stay on
  GitHub-hosted runners; no Avrea ARM label is known.
- Tools are installed by setup actions (Node, Java, Android SDK, Python, Rust, Bun)
  instead of relying on the image.

## macOS and iOS: disabled until further notice

- `build.yml`: no `mac` entry in the `build-desktop` matrix; `build-ios-unsigned` has
  `if: false`; `release` no longer needs it and no longer collects or documents macOS/iOS
  assets or generates `altstore-apps.json`.
- `translate-engine.yml`: the two `*-apple-darwin` matrix entries are commented out.
- `macos-rebuild.yml`: both jobs have `if: false`.

To re-enable: restore the `mac` matrix entry, the mac rust `targets`, the codesign verify
step and the `release` parts (see git history of `build.yml` before the "Disable macOS/iOS"
commit); remove `if: false` from `build-ios-unsigned` and add it back to `release.needs`
and its `if`; uncomment the macOS translate-engine entries; remove `if: false` in
`macos-rebuild.yml`.

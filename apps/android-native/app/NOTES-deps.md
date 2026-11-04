# B2 (android-shell) - dependency and integration notes

Catalog is frozen; everything below is requested for the integrator. B2 code is written so it compiles WITHOUT these (they are improvements).

## Dependencies not in the catalog

| Coordinates | Version | Module | Why |
|---|---|---|---|
| `androidx.lifecycle:lifecycle-viewmodel-navigation3` | 2.10.0 (same `lifecycle` ref) | `:app` | `rememberViewModelStoreNavEntryDecorator()` scopes each screen's ViewModel to its Navigation3 entry and clears it on pop. Without it ViewModels (`koinViewModel`) are Activity-scoped; `DetailsScreen` already passes a per-title `key`, so it is correct but instances are only released with the Activity. Add to `NavDisplay(entryDecorators = ...)` in `nav/TatakaiApp.kt`. |
| Font files (OFL): Space Grotesk, Outfit, JetBrains Mono + their `OFL.txt` | - | `:core:designsystem` `res/font/` | DONE in the android wave (worker b): variable `[wght]` builds fetched from github.com/google/fonts into `core/designsystem/src/main/res/font/`, OFL texts in `core/designsystem/src/main/assets/licenses/`. No Gradle dependency. |

## Assumptions about code owned by B4 (not present in this branch)

- `:app` imports `app.tatakai.next.extensions.extensionsModule`, `app.tatakai.next.player.playerModule`, `app.tatakai.next.link.linkModule` and `app.tatakai.next.feature.player.PlayerScreen(args: PlayerArgs, onBack: () -> Unit)`. Adjust the imports in `TatakaiApplication.kt` / `nav/TatakaiApp.kt` if B4 used other packages.
- `PlayerArgs.remoteSessionId` is the `RemoteSession.sessionId`; the Player resolves the playback URL itself.
- The Player is resolved by `PlayerArgs` only, so the source picked in Details cannot be passed to it: any picked row just starts playback (the player resolves + falls back by itself). "Enhance on PC" is the only path that uses the chosen source (`LinkSource.Url`).
- Link screen "Start session" has no media context, so it navigates with `PlayerArgs(media = MediaRef(), title = <url>, episode = 1, remoteSessionId = ...)`.

## Android wave (worker b, 12-android-wave-contract.md)

No new catalog entries. `:feature:settings` adds `libs.androidx.activity.compose` (already in the catalog) for `BackHandler`.
`:app` registers `copyThirdPartyNotices` (AGP `variant.sources.assets.addGeneratedSourceDirectory`) to ship THIRD_PARTY_NOTICES.md in the APK assets.

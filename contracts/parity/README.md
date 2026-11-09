# Checklist de paridad (plan `docs/redesign/14-parity-plan.md` §4)

`checklist.json` es la lista de **todas** las features visibles del Tatakai clásico (`src/`, `desktop/`, `server/`), una por entrada, con el dueño que la porta a Tatakai Next en la ola A o B. La escribió el contrato de la ola A (2026-11-09) a partir de `docs/redesign/parity/inventory-classic.md`, `inventory-next.md` y la tabla "Diferencial completo" de `docs/redesign/parity/audit-2026-11-08.html`, y la enmendó el mismo día con las decisiones del plan §0.1 (sin login propio, social solo AniList, torrents Android en la ola A, etc.). El contrato de la ola B puede **añadir** entradas; nadie borra ni renombra un `id`: un id que una decisión deja obsoleto pasa a `out` con `note` citando la decisión, y lo que lo sustituye es un id nuevo (p. ej. `library.cloud-sync` → `history.link-sync.*`). Un id cuyo significado se ajusta sin cambiar de feature conserva el id y lo dice en `note` (p. ej. `details.comments-mount`, hoy "actividad y reseñas de AniList").

`node --test contracts/test` valida el archivo (`parity-checklist.test.mjs`).

## Enmiendas del 2026-11-09 (plan §0.1) ya aplicadas

- **Sin login propio** (§0.1.9): `account.login`, `account.logout`, `account.session-persist`, `account.errors`, `library.cloud-sync`, `lists.supabase-mirror` y `social.migrations` están `out`. La identidad es `account.identity` (AniList / MAL, g/q) con `account.oauth.errors`; quitar Supabase es `account.supabase-removal` (f/q). El historial con posición exacta es `history.local-resume` y se sincroniza entre los dispositivos del usuario por Tatakai Link: `history.link-sync.route` (f, escritorio = servidor), `history.link-sync.client` (q, Android), `history.link-sync.merge`, `history.link-sync.status`. Regla *analyze before removing*: Supabase se quita cuando esos ids están cubiertos.
- **Fuera** (§0.1.10 y §0.1.14): registro, verificación, recuperar / cambiar contraseña, cambiar email, sesiones, `settings.cloud-sync`, comentarios propios (`social.comments.*`, los `*comments-mount` de manga y novela), salas (`social.rooms.*`, `details.watch-together`), perfiles propios (`social.profile.edit`, `social.settings.privacy`), `server.cloudflared`, `server.warp`, encuestas y GIFs.
- **Social solo AniList** (ola B, n / t): `social.profile.page`, `social.profile.public`, `social.follow`, `social.follow.lists`, `social.notifications.list`, `social.notifications.badge` (sondeo), `social.activity.feed`, `social.activity.interact`, `details.reviews`; `details.comments-mount` pasa a ser el punto de montaje de actividad y reseñas de AniList.
- **Entran en la ola A** (§0.1.11): `account.setup-wizard`, `system.reset-app` (f/q); `torrent.magnet-link-handler`, `torrent.session-history`, `torrent.trackers.status` (i/q). **Torrents y debrid de Android pasan a q** en la ola A (antes r, ola B): todos los `torrent.*`, `sources.debrid.*` y `player.torrent*` de la ola A tienen `android: "q"`.
- **Android** (§0.1.12): `player.android.background` con Media3 **y** libmpv (p); `player.android.upscaling-shaders` empaqueta todos los presets.
- **Ola B nueva** (§0.1.13): `sources.embed-sniffer`, `sources.embed-sniffer.ranking` (k / r); `settings.metadata.extra-providers` + `metadata.providers.{mal,shikimori,tenrai}` (b / r); `novel.tts.kitten` (m, solo escritorio).

## Ampliación del contrato de la ola B (2026-11-09)

El contrato de la ola B (agente de checklist) añadió 17 ids y corrigió 6 sin borrar ni renombrar ninguno ni tocar `status` / `found` de la ola A:

- **Sniffer** (k / r): `sources.embed-sniffer.adblock` (la WebView oculta no abre ventanas ni carga hosts de anuncios), `sources.embed-sniffer.timeout-setting` (interruptor + tiempo máximo en Ajustes › Fuentes).
- **Pendientes de `KNOWN-ISSUES.md` (ola A) con dueño en la ola B**: `sources.debrid.episode-context` (k, solo escritorio: `mark_cached` recibe `(id, episodio)` desde `LiveBackend::sources`), nota en `ext.custom-providers.info` (ids `ext:…` con extras vacíos → `custom/info`), `sourceType` obligatorio en `server.api.sources`.
- **Stubs "M1" de Android › Dispositivo enlazado** (inventory-next §3; no son features del clásico pero el auditor los cuenta como stub): `link.remote-control`, `link.handoff` ("Continuar en la PC"), `link.discovery` (mDNS + TOFU), todos k / r.
- **tk-server** (k / r): `server.api.invoke` (catálogo, manga y novelas de las extensiones de la PC), `server.api.extensions`; `server.interpreted-extensions` está `out` (D4: tk-server solo sirve `.kai`; Sora / Mangayomi / Seanime / Java se usan por el servidor Node emparejado).
- **Descargas** (h / r): `downloads.integrity-check`; `downloads.page` ahora exige pestañas Manga y Novelas además de Anime, Activas e Historial.
- **Extensiones** (j / r): `ext.kill-switch`, `ext.custom-providers.continue`; `out`: `ext.sideload-audit` (cubierto por `ext.log`), `ext.usage-events-slot` (telemetría prohibida).
- **Inicio y descubrir** (b / r): `settings.metadata.provider-status` (estado y enfriamiento por proveedor; es lo que la re-revisión mira para descartar un proveedor con nota), `settings.privacy.mature-content` (única parte de Privacidad del clásico sin perfil propio; `details.adult-gate` y `manga.adult-gate` lo leen).
- **Novelas**: `novel.tts.kitten.android` queda `out` (§0.1.13: KittenTTS solo escritorio); llevarlo a Android con ONNX es una pregunta abierta para el usuario.
- Corregido `social.avatar-picker`: su `accept` citaba `social.profile.edit`, que ya está `out`; ahora dice que el avatar es el de AniList.

Dueños Android fijos por área en la ola B (test `wave B rows are ready for the workers`): b, h, j, k → r; l, m → s; n → t; `null` solo con `platform: "desktop"`. Ningún id planificado de la ola B nombra Supabase, cloudflared, WARP, Realtime, Giphy ni Turnstile, y ningún `accept` de un `todo` empieza por "Excluido" ni promete un "Próximamente".
- **Defaults aceptados**: `system.secrets-store` (keyring / Keystore para tokens OAuth, debrid y traducción), debrid como sección de Torrents + campo en `SourceVm`, `system.update-check` contra GitHub Releases de `Kushro/Tatakai-own`, estados de lista como control segmentado dentro de "Listas" en el teléfono (`lists.library-tabs`), app id de Discord = el de `desktop/services/discord-rpc.cjs` (`player.discord.watching`); se mantienen `home.recs.atlas`, `details.reviews`, `player.android.upscaling-shaders`; `system.update-install` sigue `out`.

## Campos

| Campo | Valor | Significado |
|---|---|---|
| `id` | `<área>.<feature>[.<detalle>]`, kebab-case | Estable para siempre. Es lo que citan commits, auditoría y re-revisión. |
| `wave` | `"A"` \| `"B"` | Ola del dueño de escritorio. A = `a c d e f g i`; B = `b h j k l m n`. Android: A = `o p q`; B = `r s t`. |
| `area` | letra `a..n` | Dueño de escritorio según el plan §2. En entradas `out` es el dueño más cercano, solo para agrupar. |
| `android` | letra `o..t` \| `null` | Dueño Android. `null` = solo escritorio (bandeja, autostart, Discord, ruta de historial en `tk-link`…). Un dueño Android nunca es de una ola anterior a la de la entrada. Torrents y debrid: escritorio `i` en A y Android `q` en A (plan §0.1.11; antes `r` en B), así que ningún `torrent.*` / `sources.debrid.*` de la ola A lleva `r`. |
| `title` | texto corto en español | Nombre de la feature. |
| `classic` | ruta relativa a la raíz del repo | Archivo principal del clásico; existe. |
| `next` | ruta relativa \| `null` | Archivo de Next donde vive hoy (antes de que el contrato parta `player.rs`, `settings.rs` o `anilist.rs`); `null` si no existe nada. |
| `accept` | una frase en español | Comportamiento observable que la re-revisión (F8) comprueba. |
| `status` | ver abajo | Estado. |
| `audit` | opcional | Clasificación de la auditoría del 2026-11-08 (`Falta`, `Stub`, `Parcial`, `Paridad`, `Next mejor`, `Fuera de plan`) o `Bug` para los dos fallos vistos en la prueba. |
| `platform` | opcional: `"desktop"` \| `"android"` | Solo cuando la entrada aplica a una sola plataforma (lo normal es ambas). |
| `note` | opcional | Motivo de una exclusión, la decisión del plan que la cambió (`Plan §0.1.N`) o una pregunta abierta. |
| `found` | opcional: ruta relativa | **Solo lo escribe el auditor (F4)**: el archivo de Next donde encontró (o buscó) la feature al marcar `verified`, `stub` o `missing`. Es una cadena no vacía; puede nombrar varios archivos separados por `, `. No existe en las entradas que el auditor aún no ha revisado. |

## Estados y quién los cambia

| `status` | Quién lo pone | Cuándo |
|---|---|---|
| `todo` | contrato | Hay que implementarlo. |
| `baseline` | contrato | Next ya lo tiene igual o mejor (`Paridad` / `Next mejor`). Nada que hacer salvo no romperlo. |
| `out` | contrato | Excluido: por el plan §7 (Yomi, Wrapped, comunidad, admin, TV/IPTV, playlists, tier lists, marketplace, DynamicIsland, promos, extensiones de renderer) **o** porque no aplica a la arquitectura de Next **o** porque no tiene dueño en el plan §2. `note` dice cuál de los tres. Se incluyen para que la exclusión sea explícita. |
| `done` | programador | Solo en los ids de **su** `area` (o `android`) y solo cuando cumple `accept`. |
| `verified` / `stub` / `missing` | auditor (F4) | El auditor no se fía de `done`: lee el código y marca lo que encontró, y deja en `found` la ruta donde lo encontró (o donde debería estar, en `missing`). `stub` = control sin conexión, `NotWired`, `TODO`, "Próximamente". |

Reglas:
- Un programador no toca entradas de otra área ni cambia `accept`, `area`, `wave` o `android`. Si cree que un id está mal asignado, lo dice en el mensaje de su commit y lo decide el integrador.
- Las entradas `baseline` y `out` no se marcan `done`.
- Nuevos ids (ola B) siguen el mismo formato y pasan el test.
- Solo el auditor escribe `found`; programadores e integrador no lo añaden ni lo borran.

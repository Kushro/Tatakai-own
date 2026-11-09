# Checklist de paridad (plan `docs/redesign/14-parity-plan.md` §4)

`checklist.json` es la lista de **todas** las features visibles del Tatakai clásico (`src/`, `desktop/`, `server/`), una por entrada, con el dueño que la porta a Tatakai Next en la ola A o B. La escribió el contrato de la ola A (2026-11-09) a partir de `docs/redesign/parity/inventory-classic.md`, `inventory-next.md` y la tabla "Diferencial completo" de `docs/redesign/parity/audit-2026-11-08.html`. El contrato de la ola B puede **añadir** entradas; nadie borra ni renombra un `id`.

`node --test contracts/test` valida el archivo (`parity-checklist.test.mjs`).

## Campos

| Campo | Valor | Significado |
|---|---|---|
| `id` | `<área>.<feature>[.<detalle>]`, kebab-case | Estable para siempre. Es lo que citan commits, auditoría y re-revisión. |
| `wave` | `"A"` \| `"B"` | Ola del dueño de escritorio. A = `a c d e f g i`; B = `b h j k l m n`. |
| `area` | letra `a..n` | Dueño de escritorio según el plan §2. En entradas `out` es el dueño más cercano, solo para agrupar. |
| `android` | letra `o..t` \| `null` | Dueño Android. `null` = solo escritorio (bandeja, autostart, Discord…). Un dueño Android nunca es de una ola anterior a la de la entrada (p. ej. torrents: escritorio `i` en A, Android `r` en B). |
| `title` | texto corto en español | Nombre de la feature. |
| `classic` | ruta relativa a la raíz del repo | Archivo principal del clásico; existe. |
| `next` | ruta relativa \| `null` | Archivo de Next donde vive hoy (antes de que el contrato parta `player.rs`, `settings.rs` o `anilist.rs`); `null` si no existe nada. |
| `accept` | una frase en español | Comportamiento observable que la re-revisión (F8) comprueba. |
| `status` | ver abajo | Estado. |
| `audit` | opcional | Clasificación de la auditoría del 2026-11-08 (`Falta`, `Stub`, `Parcial`, `Paridad`, `Next mejor`, `Fuera de plan`) o `Bug` para los dos fallos vistos en la prueba. |
| `platform` | opcional: `"desktop"` \| `"android"` | Solo cuando la entrada aplica a una sola plataforma (lo normal es ambas). |
| `note` | opcional | Motivo de una exclusión o una pregunta abierta. |

## Estados y quién los cambia

| `status` | Quién lo pone | Cuándo |
|---|---|---|
| `todo` | contrato | Hay que implementarlo. |
| `baseline` | contrato | Next ya lo tiene igual o mejor (`Paridad` / `Next mejor`). Nada que hacer salvo no romperlo. |
| `out` | contrato | Excluido: por el plan §7 (Yomi, Wrapped, comunidad, admin, TV/IPTV, playlists, tier lists, marketplace, DynamicIsland, promos, extensiones de renderer) **o** porque no aplica a la arquitectura de Next **o** porque no tiene dueño en el plan §2. `note` dice cuál de los tres. Se incluyen para que la exclusión sea explícita. |
| `done` | programador | Solo en los ids de **su** `area` (o `android`) y solo cuando cumple `accept`. |
| `verified` / `stub` / `missing` | auditor (F4) | El auditor no se fía de `done`: lee el código y marca lo que encontró. `stub` = control sin conexión, `NotWired`, `TODO`, "Próximamente". |

Reglas:
- Un programador no toca entradas de otra área ni cambia `accept`, `area`, `wave` o `android`. Si cree que un id está mal asignado, lo dice en el mensaje de su commit y lo decide el integrador.
- Las entradas `baseline` y `out` no se marcan `done`.
- Nuevos ids (ola B) siguen el mismo formato y pasan el test.

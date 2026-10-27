# Rediseño nativo M3E de Tatakai-own - índice

Estos documentos son la fuente de verdad que leen los agentes de la Ola 1 y quien revise el PR `redesign/m3e-native → main`. Se escribieron en español; el código y los identificadores van en inglés.
Orden de precedencia cuando dos textos discrepan: **`06-user-decisions.md` > `05-review.md` > `04` > `03` > `02` > `01` > `00`**. Los documentos 00-04 ya incorporan 05 y 06 (así que deberían ser consistentes entre sí);
**`05-review.md` se conserva tal cual como registro histórico** de la revisión (E1-E12) y `06-user-decisions.md` como registro de las decisiones del usuario.

| Doc | Contenido |
|---|---|
| [00-decision-stack.md](00-decision-stack.md) | ADR-000: stack (Compose M3E + Media3 / Dioxus + libmpv), host de extensiones `.kai`, Supabase, router de metadata, convivencia con el legacy, licencias |
| [01-design-system.md](01-design-system.md) | "Tatami Expressive": tokens (fuente única `packages/tokens`), color/AMOLED/contraste, tipografía, forma, movimiento, componentes, accesibilidad |
| [02-ux-flows.md](02-ux-flows.md) | Navegación por forma de ventana, pantallas y estados, flujos (core loop, datos, enlace) |
| [03-pairing-upscale.md](03-pairing-upscale.md) | Tatakai Link v1 (PC ↔ móvil): transporte, emparejamiento, rutas, fuentes con SSRF, pipeline de mejora de imagen |
| [04-implementation-plan.md](04-implementation-plan.md) | Alcance de M0, ramas B1-B6 y propiedad de paths, contratos Kotlin/Rust, criterios de aceptación, integración, riesgos, roadmap |
| [05-review.md](05-review.md) | Revisión independiente: enmiendas obligatorias E1-E12 (histórico, sin cambios) |
| [06-user-decisions.md](06-user-decisions.md) | Decisiones del usuario: AGPL-3.0 para `apps/*`, copiar de repos propios, CameraX+zxing, solo en+es, worktrees |

## Qué existe después de B1 (`m3e/foundation`)

| Artefacto | Ruta | Verificación |
|---|---|---|
| Tokens (10 paletas + AMOLED + 3 contrastes), schema, generador | `packages/tokens/` | `node --test packages/tokens/test` |
| `Tokens.kt` / `tokens.css` generados | `apps/android-native/core/designsystem/.../generated/Tokens.kt`, `apps/desktop-native/crates/tk-ui/assets/tokens.css` | test de "regenerar = sin diff"; `Tokens.kt` compila con `:core:designsystem:compileDebugKotlin` |
| Contratos | `contracts/{extension-host,link,supabase,fixtures}` | `node --test contracts/test` (incluye el guard `tatakai.me`/telemetría) |
| Esqueleto Android | `apps/android-native/` (Gradle 9.3.1, 21 módulos + `build-logic`) | `./gradlew.bat help` y resolución de dependencias de todos los módulos |
| Esqueleto desktop | `apps/desktop-native/` (workspace Cargo, 6 crates) | `cargo metadata`, `cargo check --workspace --no-default-features` |
| Guía operativa | [`apps/README.md`](../../apps/README.md) | toolchain, inyección de config (E8), `.deps-sources` (E2), ownership, reglas |

## Desviaciones de B1 respecto del borrador 04 (leer antes de programar)

1. `Tokens.kt`: `typography` es una **función** `typography(fonts: TatakaiFontFamilies, scale)`; existe `TatakaiTokens.palette(id, ContrastLevel)`. Ver `01 §1`.
2. `@material/material-color-utilities` está **vendorizado** (no hay `npm install` ni dependencia nueva en ningún `package.json`).
3. `:core:link` es librería **Android** (no JVM puro). JVM puro: `:core:model`, `:core:metadata`, `:core:extensions`.
4. KSP `2.3.12` (ya no existe la línea `2.4.0-*`); supabase-kt `3.8.0`; Ktor `3.6.0`; material3 fijado en `1.5.0-alpha15`.
5. Los fixtures `.kai` empaquetados viven en `contracts/fixtures/extensions/packed/` (el `.gitignore` raíz ignora `dist*`).
6. El plugin de convenciones expone el catálogo como `tatakaiCatalog` (un `Project.libs` propio taparía los accessors generados `libs.*` en todos los `build.gradle.kts`).
7. `mdns-sd` NO está en `[workspace.dependencies]` (mDNS es M1); se añadieron varias crates de apoyo para que el árbol congelado no necesite cambios (ver `04 §3.3`).
8. Config del player desktop: sin ventana overlay (E5). Config de claves del cliente Link: modelo de Enigmata (E4).

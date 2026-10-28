# 01 — Sistema de diseño "Tatami Expressive" (M3E, Android + Desktop)

Base: Tatami (`E:/Gitbutler Repos/design_system_android`, M3 estándar: 8 paletas, spacing 4 dp, MediaCover 13:18, AdaptiveSheet…)
+ capa Expressive inspirada en Miko (`MaterialExpressiveTheme` + materialKolor + AMOLED). Se **copia/adapta** dentro de
`apps/android-native/core/designsystem` (06 #1 permite copiar de design_system_android y Miko; no se depende del módulo Tatami: AGP/Kotlin distintos). Los copiados se anotan en `THIRD_PARTY_NOTICES.md`.

## 1. Fuente única de tokens
`packages/tokens/tokens.json` (validado por `packages/tokens/tokens.schema.json`, JSON Schema 2020-12) → `node packages/tokens/generate.mjs` →
- `apps/android-native/core/designsystem/src/main/kotlin/app/tatakai/next/designsystem/generated/Tokens.kt`
- `apps/desktop-native/crates/tk-ui/assets/tokens.css`

Los archivos generados se commitean (nadie necesita Node para compilar). El generador es Node plano **sin dependencias npm**: `@material/material-color-utilities` 0.4.0 está
*vendorizado* en `packages/tokens/vendor/material-color-utilities/` (Apache-2.0, ver su README). `node --test packages/tokens/test` verifica: esquema válido, regenerar no produce diff,
generación determinista, y contraste AA (E12). `generate.mjs --check` hace solo la comparación.

Esquema real de `tokens.json` (resumen):
```json
{
  "version": 1,
  "defaultPalette": "tatakai-violet",
  "palettes": { "<id>": { "label": "Sakura", "seed": "#E86A92", "style": "Expressive|TonalSpot|Vibrant|Fidelity|Monochrome",
                          "contrast": 0.0, "amoledCapable": true, "overrides": { "light|dark|amoled": { "<colorRole>": "#RRGGBB" } } } },
  "allowContrastFailures": [ { "palette": "id|*", "mode": "light|dark|amoled|*", "contrast": "standard|medium|high|*", "pair": ["onX", "x"], "reason": "texto obligatorio" } ],
  "fonts": { "display": "Space Grotesk", "text": "Outfit", "mono": "JetBrains Mono" },
  "typography": { "tvScale": 1.15,
                  "roles": { "displayLarge": { "font": "display", "size": 57, "lineHeight": 64, "weight": 400, "tracking": -0.25, "emphasizedWeight": 600 }, "...15 roles": {} } },
  "shape": { "none": 0, "extraSmall": 4, "small": 8, "medium": 12, "large": 16, "largeIncreased": 20,
             "extraLarge": 28, "extraLargeIncreased": 32, "extraExtraLarge": 48, "full": 9999 },
  "spacing": { "unit": 4, "scale": [0, 2, 4, 8, 12, 16, 20, 24, 32, 40, 48, 64] },
  "motion": { "spatial": { "fast": {"damping":0.9,"stiffness":1400}, "default": {"damping":0.9,"stiffness":700}, "slow": {"damping":0.9,"stiffness":300} },
              "effects": { "fast": {"damping":1.0,"stiffness":3800}, "default": {"damping":1.0,"stiffness":1600}, "slow": {"damping":1.0,"stiffness":800} } },
  "density": { "phone": 0, "tablet": 0, "desktop": -1, "tv": 1 },
  "elevation": [0, 1, 3, 6, 8, 12]
}
```
Los esquemas de color **no** se escriben a mano: el generador expande cada `seed` (`style` → `SchemeExpressive|TonalSpot|Vibrant|Fidelity|Monochrome`) a **los 48 roles** del constructor completo de `ColorScheme` de
material3 1.5.0-alpha15 (incluye `surfaceBright/Dim`, `surfaceContainer{Lowest..Highest}`, `primaryFixed/-Dim`, `onPrimaryFixed/-Variant`, y los equivalentes secondary/tertiary) por {light, dark, amoled} y por
3 niveles de contraste, y los emite **idénticos** en Kotlin (funciones `private fun scheme<Paleta><Nivel><Modo>(): ColorScheme`, una por esquema, para no superar el límite de 64 KB de un `<clinit>`) y CSS. materialKolor queda solo para color dinámico/por póster en runtime.

**API Kotlin generada** (paquete `app.tatakai.next.designsystem.generated`; ya compila contra material3 1.5.0-alpha15):
```kotlin
enum class ContrastLevel { Standard, Medium, High }
data class PaletteSchemes(val light: ColorScheme, val dark: ColorScheme, val amoled: ColorScheme)
data class PaletteInfo(val id: String, val label: String, val seed: Color, val style: String, val amoledCapable: Boolean)
class TatakaiFontFamilies(val display: FontFamily, val text: FontFamily, val mono: FontFamily)   // las crea la app desde res/font
object TatakaiTokens {
  const val DEFAULT_PALETTE: String;  const val TV_TYPE_SCALE: Float
  val paletteInfo: List<PaletteInfo>
  val palettes: Map<String, PaletteSchemes>                       // contraste "standard" de cada paleta
  fun palette(id: String, level: ContrastLevel = ContrastLevel.Standard): PaletteSchemes?
  val shapes: Shapes;  val motion: MotionScheme;  val spacing: Spacing;  val elevation: List<Dp>;  val density: DensitySteps
  fun typography(fonts: TatakaiFontFamilies, scale: Float = 1f): Typography     // 15 roles + 15 *Emphasized
}
object ShapeRadius { val none, extraSmall, ..., full: Dp }
```
(Desviación respecto del borrador 04 §3.4: `typography` es una **función** porque las `FontFamily` salen de los `res/font` de la app, no del generador; se añaden `palette(id, level)` y `ContrastLevel` para la preferencia de contraste.)

**CSS generado**: `:root` con tipografía (`--md-sys-typescale-<rol>-{font,size,line-height,weight,weight-emphasized,tracking}`), formas (`--md-sys-shape-corner-<tamaño>`), movimiento
(`--md-sys-motion-<spatial|effects>-<fast|default|slow>-{duration,easing}`, ver §5), espaciado (`--tk-space-<n>`), elevación y densidad; los colores como `--md-sys-color-<rol-en-kebab>` bajo
`:root[data-palette="<id>"][data-mode="light|dark|amoled"]` y, para contraste medio/alto, el mismo selector más `[data-contrast="medium|high"]`. Sin atributos aplica la paleta por defecto en oscuro.

## 2. Color
- **Dinámico** (Android 12+): `dynamicColorScheme` como opción "Sistema"; por defecto la paleta de marca.
- **Paletas curadas** (de las 27 actuales se quedan 10, mapeadas a seed + estilo):

| id nuevo | viene de | seed | estilo |
|---|---|---|---|
| tatakai-violet (default) | dark / dark-theme | #7C4DFF | Vibrant (Expressive rotates the primary hue) |
| midnight | midnight, obsidian, dark-matter | #3D5AFE | TonalSpot |
| sakura | cherry-blossom, light-sakura | #E86A92 | Vibrant |
| matcha | matcha-light, zen-garden, emerald-forest | #5E9E6E | TonalSpot |
| ocean | deep-ocean, ocean-breeze, frostbite | #0091C2 | Vibrant |
| neon-tokyo | neon-tokyo, cyberpunk, synthwave | #FF2E88 | Vibrant |
| blood-moon | blood-moon, vampire | #C62828 | Fidelity |
| royal-gold | royal-gold, solar, caffeine | #C9A227 | Fidelity |
| aurora | aurora-borealis | #00BFA5 | Vibrant |
| mono | brutalism-*, light-minimal, high-contrast | #808080 | Monochrome (+contraste 1.0) |
- Se eliminan: `anime-theme`, `ultra-lite*`, `lite` (eran modos de rendimiento; el nativo no los necesita).
- Hoy ningún par falla AA, así que `allowContrastFailures` está vacío; si una paleta nueva (p. ej. `Fidelity`) necesita una excepción debe declararse ahí con su `reason`.
- **AMOLED**: toggle independiente de la paleta; se deriva del esquema **dark** por post-proceso (en `lib/colors.mjs`): `background`, `surface`, `surfaceDim` y `surfaceContainerLowest` = `#000000`; `surfaceContainerLow/Container/High/Highest` y `surfaceBright` se re-eligen de la paleta neutral en tonos T5/T8/T12/T16/T20 (dark usa T10/T12/T17/T22/T24). Los demás roles (incluidos los `on*`) no cambian, así que el contraste `on*` sobre `surface` solo puede subir.
- **Contraste**: estándar / medio / alto desde Ajustes → accesibilidad. Estándar = el `contrast` propio de la paleta (0 salvo `mono` = 1.0); medio = `max(0.5, contrast)`; alto = 1.0. Se generan los tres niveles para cada paleta y modo (`TatakaiTokens.palette(id, level)` / `data-contrast`).
- Color de acento por contenido (opcional): en Detalles se puede generar un esquema desde el póster (Palette API / `image` crate) — M1.

## 3. Tipografía
Escala M3E con *emphasized* (peso +100/+200). Display/Headline/Title → Space Grotesk; Body/Label → Outfit. Las fuentes se
**empaquetan** (Android `res/font`, desktop `assets/fonts`), nunca CDN (offline y privacidad), cada familia con su `OFL.txt` (OFL-1.1) listado en "Acerca de". Idiomas de la UI: **en + es** (06 #4).
Roles: displayLarge 57/64, displayMedium 45/52, displaySmall 36/44, headlineLarge 32/40 … labelSmall 11/16, más `*Emphasized`.
TV: escala ×1.15 vía `density.tv`. Soporte de escalado del sistema hasta 200 % (layouts sin alturas fijas en texto).

## 4. Forma y morphing
- Escala de forma de la tabla del JSON. Mapeo: chips/campos = small, tarjetas = medium/large, sheets = extraLarge, FAB/toolbar = full o `largeIncreased`.
- **Morphing**: `MaterialShapes` (Cookie9Sided, Sunny, Pill, Clover…) + `Morph` para (a) botón Play ↔ Pausa (círculo → cuadrado redondeado),
  (b) avatar/estado de enlace (Cookie → Sunny cuando hay PC conectada), (c) indicador de selección en la rail/bar, (d) `LoadingIndicator` M3E.
- Desktop: equivalentes con `clip-path`/`border-radius` animado en CSS + 4 SVG paths de MaterialShapes precalculados; sin morph geométrico real (aceptable).
- `MaterialShapes`/`Morph` y `LoadingIndicator` son API experimental de material3 1.5.0-alpha15: verificar firmas con grep en `.deps-sources/` (ver `apps/README.md`).

## 5. Movimiento
- `MotionScheme.expressive()` como base; los resortes del JSON se emiten como `MotionScheme` custom (Kotlin) y como `--motion-spatial-*`
  (desktop: `--md-sys-motion-*-easing: linear(...)` de 32 puntos + `--md-sys-motion-*-duration`, ambos calculados por `lib/spring.mjs` simulando el resorte de masa 1 hasta ±0,002 de 1; se generan desde el JSON).
- Espacial (posición/tamaño/forma) → spring con rebote leve; efectos (color/opacidad) → sin rebote.
- Transiciones de navegación: shared element póster Home → Detalles → Player (Compose `SharedTransitionLayout`); en desktop View Transitions de WebView2.
- `reduce-motion`: respeta `ANIMATOR_DURATION_SCALE`/`prefers-reduced-motion` → resortes `effects.fast` y sin rebote.
- Presupuesto: ninguna animación infinita en listas; sin blur en tiempo real en listas (haze solo en barras sobre el player y la top bar de Detalles, y desactivable).

## 6. Componentes
| Componente | Compose | Dioxus (`tk-ui/src/components/`) |
|---|---|---|
| Botones, ButtonGroup, SplitButton, IconButton toggle | M3E built-in | `button.rs` (variantes filled/tonal/outlined/text, tamaños XS–XL) |
| FloatingToolbar (acciones de Detalles y Reader) | M3E `HorizontalFloatingToolbar` | `floating_toolbar.rs` |
| NavigationBar / ShortNavigationBar / NavigationRail (wide) | M3E `NavigationSuiteScaffold` adaptable | `nav_rail.rs` (expandible a drawer) |
| LoadingIndicator / ContainedLoadingIndicator | M3E built-in | `loading.rs` (SVG morph de 7 formas) |
| Wavy progress (descargas, buffer) | `LinearWavyProgressIndicator` | `progress.rs` |
| Carousel (hero, continuar viendo) | `HorizontalMultiBrowseCarousel` / `HorizontalUncontainedCarousel` (paquete aparte `androidx.compose.material3.carousel`, E11) | `carousel.rs` (scroll-snap) |
| SearchBar + resultados | `SearchBar`/`ExpandedFullScreenSearchBar` | `search_bar.rs` |
| MediaCover 13:18, MediaCard, EpisodeRow, ChapterRow | custom (desde Tatami) | `media.rs` |
| SectionCard, ListGroupHeader, InfoChip, Pill, Badges | custom (desde Tatami) | `surface.rs` |
| AdaptiveSheet (bottom sheet en phone, side sheet en wide) | `ModalBottomSheet` en compact; en medium/expanded un **side sheet custom** (Surface anclado + scrim). `ModalSideSheet` **no existe** en material3 1.5.0-alpha15 (E11) | `sheet.rs` |
| Player controls (seek bar con miniaturas, chips de calidad/audio/subs) | custom | `player_controls.rs` |
| Empty / Error / Loading screens | custom (Tatami) | `state.rs` |
| Dialog destructivo | custom | `dialog.rs` |
| Title bar sin marco (desktop) | — | `title_bar.rs` |

## 7. Densidad y plataformas
- Phone: touch target ≥ 48 dp, grids 3 columnas (compact), 4–5 (medium).
- Tablet/foldable: window size classes (Compact/Medium/Expanded/Large); list-detail con `ListDetailPaneScaffold`; postura tabletop en el player (controles en la mitad inferior).
- Desktop: densidad −1 (alturas −4 px), hover states, atajos de teclado (espacio, ←/→ 5 s, F, Ctrl+K búsqueda), ancho máx. de contenido 1440 px.
- TV: Compose for TV (`androidx.tv.material3`) solo para el shell de foco en M2; densidad +1, foco visible con escala 1.05 + borde `primary`, overscan 48 dp.

## 8. Accesibilidad
- Contraste mínimo AA (4.5:1) verificado por `node --test packages/tokens/test` sobre cada paleta × modo (light/dark/amoled) × nivel de contraste, con los 9 pares exactos de E12: `(onPrimary, primary)`, `(onSecondary, secondary)`, `(onTertiary, tertiary)`, `(onError, error)`, `(onSurface, surface)`, `(onSurfaceVariant, surface)`, `(onPrimaryContainer, primaryContainer)`, `(onSecondaryContainer, secondaryContainer)`, `(onTertiaryContainer, tertiaryContainer)`.
- `contentDescription`/`aria-label` obligatorio en icon-buttons (lint de Compose + regla en componentes Dioxus: prop requerida).
- Navegación completa por teclado/D-pad; orden de foco explícito en el player.
- Subtítulos: tamaño, fondo y fuente configurables; respetar `CaptioningManager` en Android.
- Sin información solo por color (estados de enlace llevan icono + texto).

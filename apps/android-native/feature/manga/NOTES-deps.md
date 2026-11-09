# NOTES-deps (:feature:manga, parity wave B, worker s)

Sin dependencias externas nuevas: todo está ya en el catálogo.
- `projects.core.manga` (módulo core del mismo worker).
- `libs.coil.network.okhttp` (ya usado por `:core:ui`): `NetworkHeaders` / `httpHeaders` para las páginas que exigen el `Referer` de la extensión.
- `libs.androidx.core.ktx` (`WindowCompat`, modo inmersivo del lector) y `libs.compose.material.icons.extended` (también llega por `:core:designsystem`).

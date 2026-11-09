# NOTES-deps (:core:manga, parity wave B, worker s)

Nada nuevo pedido por el contrato (OkHttp, DocumentFile, Koin, serialización ya están en el catálogo). Si s necesita Room para el
progreso / readlist / capítulos descargados, puede usar `libs.room.runtime` + `libs.room.ktx` + ksp `libs.room.compiler` (ya en el
catálogo; mirar `core/database/build.gradle.kts` para el plugin ksp) con una base de datos PROPIA (`manga.db`), nunca `TatakaiDatabase`.
Añade filas aquí (coordenadas, versión, motivo) para cualquier otra dependencia.

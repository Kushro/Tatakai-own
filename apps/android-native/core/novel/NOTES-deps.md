# NOTES-deps (:core:novel, parity wave B, worker s)

Nada nuevo pedido por el contrato: OkHttp, jsoup (limpiar HTML de los capítulos), Media3 (`media3-exoplayer` para reproducir el audio
de los motores TTS en la nube), Koin y serialización ya están en el catálogo. `android.speech.tts.TextToSpeech` es de la plataforma.
KittenTTS NO tiene build Android (plan §0.1.13: solo escritorio). Añade filas aquí (coordenadas, versión, motivo) para lo demás.

Ola B, worker s (implementación): `:core:novel` depende ahora de `:core:manga` (`api(projects.core.manga)`, módulo del mismo worker, sin
dependencia externa nueva) para reutilizar el cliente AniList, `ReadlistStore`, `JsonFileStore` y la lógica de capítulos. El texto
descargado vive en un SQLite propio (`filesDir/novels/novel.db`, `android.database.sqlite.SQLiteDatabase` de la plataforma; sin Room).

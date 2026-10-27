# 03 — Tatakai Link v1 (PC ↔ móvil) y mejora de imagen

Basado en el contrato Temple v1 (`E:/Gitbutler Repos/Enigmata-Temple/docs/contract/v1.md`). Temple y Enigmata son repos del usuario: **se puede copiar código** (06 #1, apps AGPL-3.0-or-later; anotar el origen en `THIRD_PARTY_NOTICES.md`).

> **Alcance M0 (E7, 06 #2)**: emparejamiento solo por QR o código pegado (**sin mDNS ni TOFU**), **sin** `/v1/settings`, sin `events`/`handoff`/`control`/`sync`/`downloads`/fuente `phone`; `library` = una carpeta elegida, no recursiva, con ffprobe (opcional: la fuente que importa es `url`). Lo diferido se conserva abajo marcado M1/M2 como diseño. **Fuente de verdad máquina: `contracts/link/openapi.v1.json`** (con los trims E6/E7 aplicados y la lista de 26 códigos de error).
Se conserva transporte, pairing, modelo de sesión VOD fMP4-HLS, latido y catálogo de errores; se agregan mDNS (M1), fuente `url`
con política SSRF, handoff, control remoto y sync. Implementación: `apps/desktop-native/crates/tk-link` (servidor) y
`apps/android-native/core/link` (cliente). Contrato máquina: `contracts/link/openapi.v1.json`. Cabecera en toda respuesta: `Tatakai-Link: v1`.

## 1. Transporte e identidad
- `https://<ipv4>:<puerto>` (por defecto **47321**), TLS 1.3/1.2 rustls **con proveedor `ring`**, ECDSA P-256, ALPN `http/1.1`, máx. 64 conexiones.
- CA propia generada en el primer arranque (`%APPDATA%\Tatakai-Next\link\ca.{der,key}`; clave protegida con DPAPI). Cert de servidor con SAN = todas las IPv4 LAN activas + `127.0.0.1` + `localhost`; se re-emite si cambia la IP.
- El teléfono **fija el SHA-256 del DER de la CA**. Cert de cliente emitido por la CA, CN = `device_id`, 365 días. **Modelo de clave de Enigmata (E4)**: clave de **software** P-256, sellada en reposo con AES-GCM de AndroidKeyStore en `noBackupFilesDir` (referencia: `Enigmata/app/.../temple/crypto/TempleCrypto.kt` y `vault/TempleKeyVault.kt`); en uso vive en memoria detrás de un `X509KeyManager` propio para OkHttp, y solo se exporta como PKCS#8 temporal cuando llegue el motor mpv (M1, `tls-key-file`). Una clave Keystore no exportable habría bloqueado mpv y su soporte en handshakes TLS varía por OEM.
- mTLS obligatorio en todas las rutas salvo `POST /v1/pairing`. Revocación en caliente (lista en memoria + archivo, efecto ≤ 2 s; cierra conexiones vivas del dispositivo).
- Cuerpos JSON con tope por ruta y 10 s de plazo. Errores `{"error":{"code","message"}}` con mensajes fijos: los 23 códigos de Temple v1 + `url_source_denied (403)`, `extension_unavailable (422)`, `handoff_rejected (409)` (catálogo completo en el OpenAPI; `extension_unavailable` y `handoff_rejected` no los produce el servidor M0).

## 2. Descubrimiento y emparejamiento
- **QR/texto** (único método en M0; el teléfono escanea con CameraX + zxing, sin Google Play services, o el usuario pega el texto): `tatakai-link:v1?h=<ipv4>&p=<port>&ca=<sha256hex>&c=<10 Crockford>&x=<unix exp>&n=<nombre PC urlencoded>`.
  Ventana única de 5 min, un solo uso, ≤ 5 intentos; orden de validación ventana → CSR → código (igual que Temple).
- **mDNS + TOFU - DIFERIDO A M1 (E7; no se implementa en M0)**. Diseño conservado para M1. Servicio mDNS (opcional, activado por defecto, desactivable): servicio `_tatakai-link._tcp.local.`, TXT `v=1, ca=<primeros 16 hex del pin>, n=<nombre>, pair=0|1`.
  El TXT **nunca** lleva el código. Flujo: el teléfono lista PCs (NsdManager) → el usuario elige → la PC abre ventana y muestra el código de 10 caracteres →
  el usuario lo teclea (o escanea el QR). El pin completo de la CA se recibe en el QR; en el flujo mDNS se usa TOFU: el teléfono calcula el SHA-256 de la CA presentada en el handshake, exige que empiece con el `ca` del TXT, y muestra 4 grupos de 4 hex que la PC también muestra para comparación visual antes de enviar el CSR.
- `POST /v1/pairing {code, device_name, csr_pem, platform:"android"}` → `201 {device_id, client_cert_pem, ca_cert_pem, server_name}`. El teléfono verifica que la CA coincide con el pin y que el cert contiene su clave pública.
- Gestión: PC → Ajustes > Enlace > Dispositivos (renombrar, revocar). Teléfono → Dispositivo enlazado > Olvidar (`DELETE /v1/devices/self`).
- Firewall de Windows (E6): la regla (solo perfil *Privado*) requiere elevación; se crea **únicamente** desde Ajustes > Enlace > "Permitir en red privada" con prompt UAC, nunca al primer arranque ni automáticamente en perfil Público.

## 3. Rutas
| Ruta | Uso | Milestone |
|---|---|---|
| `POST /v1/pairing` | emparejar (sin mTLS de cliente) | M0 |
| `GET /v1/capabilities` | GPU (vendor, nombre, Vulkan), encoders disponibles (`h264_amf/h264_nvenc/h264_qsv/libopenh264`), catálogo de perfiles con `measured_sustain`, `client_fg`, `instance_id` | M0 |
| `DELETE /v1/devices/self` | el teléfono olvida/revoca su propio certificado | M0 |
| `POST /v1/sessions` | `{source, profile?, audio_track?, start_secs?}` → `201 {session_id, generation, playback_path, duration_secs?, segment_secs, heartbeat_interval_secs, idle_timeout_secs}` | M0 |
| `GET /v1/sessions/{id}` / `DELETE` | estado / cerrar | M0 |
| `GET /v1/sessions/{id}/{master.m3u8,media.m3u8,init.mp4,seg_<k>.m4s}` | salida HLS fMP4 (2 s, bajo demanda, ventana RAM 512 MiB) | M0 |
| `POST /v1/sessions/{id}/position` | latido 4 s `{position_secs, paused}`; 120 s sin latido → cierre | M0 |
| `GET /v1/library` | **una carpeta elegida por el usuario, no recursiva, ffprobe** (E7); opcional en M0 si B5 no llega | M0 (opcional) |
| `GET/PUT /v1/settings` | ajustes compartidos con `revision` (409 en conflicto) | **M1** |
| `GET /v1/events?wait=25` | long-poll de eventos PC→teléfono (handoff entrante, comandos) | M1 |
| `POST /v1/handoff` | `{media_ref, episode, position_secs, source_hint}` "continuar en el otro dispositivo" | M1 |
| `POST /v1/control` | `{cmd: play\|pause\|seek\|next\|prev\|volume\|select_track, arg}` control remoto del player desktop | M1 |
| `GET /v1/sessions/{id}/subtitles/{n}.vtt` | subtítulos de la fuente `library` | M1 |
| `GET /v1/sync/state?since=` / `POST /v1/sync/push` | historial y biblioteca local (LWW por `updated_at`, mismo modelo que `watch_history`) | M2 |
| `GET /v1/downloads` / `GET /v1/downloads/{id}/file` (Range) | copiar descargas de PC a teléfono | M2 |
| canal de fuente del teléfono (`source/requests`, `source/ranges`) | archivo local del teléfono → PC | M2 |

## 4. Fuentes de sesión
```json
{ "kind": "library", "item_id": "…" }                                                                  // M0, opcional (una carpeta, no recursiva)
{ "kind": "url", "url": "https://cdn…/master.m3u8", "headers": {"Referer": "…", "User-Agent": "…"},
  "extension_id": "com.example.src", "container": "hls|mp4|dash" }
{ "kind": "extension", "extension_id": "…", "media_ref": {"anilistId": 1}, "episode": 5, "provider": "…" }   // M1: la PC resuelve con su propio host
{ "kind": "phone", "size_bytes": 123, "name": "…" }                                                         // M2
```

### 4.1 Fuente `url` y controles SSRF (resuelve los streams HLS de extensiones con cabeceras)
FFmpeg **nunca** recibe la URL externa. `tk-link` levanta un *fetcher* en loopback (`http://127.0.0.1:<eph>/u/<session_token>/…`) y FFmpeg corre con
`-protocol_whitelist http,tcp,crypto` (**`crypto` es obligatorio** para `#EXT-X-KEY` AES-128, E6) apuntando solo a ese loopback. El fetcher:
1. Acepta solo `https` (y `http` si el ajuste "permitir HTTP" está activo); puertos 80/443/8080/8443.
2. Resuelve DNS él mismo y **rechaza** IPs loopback, privadas (RFC1918, CGNAT 100.64/10), link-local (169.254/16, fe80::/10), ULA fc00::/7, multicast, `0.0.0.0/8`, y metadata cloud; conecta a la IP ya validada (anti DNS-rebinding).
3. Redirecciones: máx. 5; re-valida cada salto; **elimina `Cookie`/`Authorization` en cambio de origen**.
4. Cabeceras: lista blanca `Referer, Origin, User-Agent, Accept, Accept-Language, Cookie, Range` (Cookie solo hacia el origen inicial); tamaño total ≤ 8 KiB; sin CR/LF.
5. Reescribe playlists HLS (master/media/`#EXT-X-KEY`/`#EXT-X-MAP`) para que cada URI pase por el fetcher con la misma política; se aplica a segmentos y claves. El reescritor **rechaza** URIs `data:` y `file:` y cualquier esquema distinto de http(s), limita la playlist a ≤ 2 MiB y la profundidad a master → media (sin más niveles); un `#EXT-X-KEY` con URI absoluta a un host no permitido se rechaza (E6).
6. Si el teléfono envía `extension_id` **y** la PC tiene esa extensión instalada, cada host se intersecta además con sus `permissions network:domain:*`. Si no la tiene, aplica solo la política genérica (1–5, 7).
7. Límites: 64 req concurrentes por sesión, 2 GiB por sesión, timeout 20 s por request; logs redactados (sin query strings).

## 5. Pipeline de mejora en PC
Decode (FFmpeg, `-hwaccel d3d11va` → fallback sw) → **upscale** → **FG** → encode → HLS fMP4.
| Etapa | Opción por defecto | Alternativas |
|---|---|---|
| Upscale | **Anime4K 4.0 vía `libplacebo` (Vulkan, vendor-neutral)**: modos A/B/C × tamaños S/M/L compuestos en un `.glsl` por perfil (`custom_shader_path`) | FSR 1 (`.glsl` en libplacebo), `sr_amf` (AMD), NVIDIA VSR/RTX (M3, vía NVENC SDK) |
| FG | **Ninguna en servidor por defecto**; `frc_amf` si GPU AMD y perfil medido sustentable | `minterpolate` (CPU, solo ≤720p), RIFE ncnn (M3, solo bench), **cliente LSFG** (`client_fg: allowed`) |
| Encode | detectado: `h264_amf` / `h264_nvenc` / `h264_qsv` / `libopenh264` | HEVC (M2, si el teléfono lo declara) |
Perfiles: `p-none`, `p-anime4k-1080`, `p-anime4k-1440`, `p-anime4k-2160`, `p-anime4k-1080-fg2x` (solo AMD). `GET /v1/capabilities` marca cada perfil con
`measured_sustain: true|false|unknown` (bench de 10 s al primer uso, guardado). Seek = reinicio de FFmpeg en el segmento k (latencia objetivo < 4 s 1080p).
FFmpeg y shaders se pinnean en `apps/desktop-native/packaging/ffmpeg.lock.json` y `anime4k.lock.json` (URL + SHA-256); un script PowerShell los descarga **solo cuando el usuario lo ejecuta** (no en build).

## 6. Mejora en el dispositivo (sin PC)
- **Desktop player (mpv embebido)**: `glsl-shaders` con los presets Anime4K oficiales (`Mode A (Fast)`, `A+A`, `B`, `C`, HQ/Fast según GPU) y FSR/CAS;
  conmutación en vivo con `set_property("glsl-shaders", …)`; atajos Ctrl+1..6 (mismos que la guía de Anime4K para mpv). Suavizado: `interpolation=yes, tscale=oversample`
  y `video-sync=display-resample` (no es FG real, se etiqueta "Suavizado"). FG real local → M3 (RIFE/vapoursynth opcional, no en M0).
- **Android M0 (Media3)**: sin shaders. **M1**: motor mpv (AAR precompilado `mpv-android`/`libmpv` con `.so` incluidos, sin NDK propio) con Anime4K `Mode A (Fast)` y `A+A` para gama alta; detección por GPU (Adreno 6xx+/Mali-G7x+) y advertencia de batería. Media3 `GlEffect` con Anime4K portado → M3 investigación.
- **LSFG en teléfono**: integración igual a Enigmata (requiere DLL del usuario, Vulkan 1.3, arm64) → M3, detrás de ajuste experimental; la PC lo habilita con `client_fg`.

## 7. Seguridad (resumen)
Tabla de tests obligatoria en B5 (E6): rebinding (DNS devuelve pública y luego privada), redirección a IP privada, `#EXT-X-KEY` con URI absoluta a host no permitido, cabecera con CR/LF, `data:`/`file:` en playlists, playlist > 2 MiB.
Sin servicios expuestos fuera de LAN (bind a IPs privadas, nunca 0.0.0.0 en perfil público), sin telemetría, logs redactados, código de pairing nunca logueado,
claves de la PC cifradas con DPAPI, del teléfono selladas con AES-GCM de AndroidKeyStore (E4), fetcher SSRF obligatorio para toda URL externa, `-protocol_whitelist` en cada invocación de FFmpeg,
`Job Object` de Windows para matar el árbol de FFmpeg al cerrar.

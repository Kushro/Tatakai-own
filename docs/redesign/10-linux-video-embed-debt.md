# Video incrustado en Linux: estado y deuda de arquitectura

Estado a noviembre de 2026 (rama `m3e/tk-ui-w1`). Complementa la sección 3.2 de `09-wave2-contract.md`.

## Cómo funciona hoy

libmpv dibuja con su Render API (OpenGL) en un `gtk::GLArea` que está **debajo** del `WebKitWebView` dentro de un
`gtk::Overlay` (`crates/tk-player/src/linux/mod.rs`). La web view tiene fondo transparente a nivel de widget y la
página deja un hueco (`#tk-video-hole`) por donde se ve el video; nuestros controles se dibujan encima con HTML.
La ventana de nivel superior sigue opaca.

Verificado en CachyOS, KDE Plasma 6 Wayland, RX 9070 XT, mpv 0.40 / libmpv 2.5, webkit2gtk-4.1 2.54.2: video
visible en Integrado y en Integrado mejorado (con uosc), controles encima, vista previa de la barra sin rastros.

## La falla que tuvimos (y por qué no era la Render API)

Síntomas: audio y tiempo avanzan pero el video sale negro, y los elementos que se mueven sobre el hueco (la
tarjeta de vista previa de la barra) dejan copias apiladas.

Causa: Dioxus desktop 0.7 exporta por defecto, en Wayland, `WEBKIT_DISABLE_DMABUF_RENDERER=1` y `GDK_BACKEND=x11`
(`dioxus-desktop/src/app.rs`, `disable_dma_buf`). El `GDK_BACKEND` llega tarde para la propia app (GTK ya
eligió Wayland), pero ambas variables las heredan el proceso web de WebKit y cualquier proceso hijo. Sin el
renderizador DMA-BUF, WebKitGTK pinta la página en un modo que nunca limpia lo que hay bajo los píxeles
transparentes: el `GLArea` no se ve y lo que se mueve deja rastro. Con `WEBKIT_DISABLE_COMPOSITING_MODE=1` pasa
lo mismo. Nuestro `AppRun` también forzaba `WEBKIT_DISABLE_DMABUF_RENDERER=1`.

Cómo se aisló: `crates/tk-player/examples/linux_embed_probe.rs` (ventana GTK sola + misma superficie GL) mostraba
el video; la misma prueba con la ventana de Dioxus/tao/wry salía negra hasta desactivar
`with_disable_dma_buf_on_wayland`. Arreglo: `Config::with_disable_dma_buf_on_wayland(false)` en `tk-app`,
`AppRun` ya no fuerza la variable y, si el usuario la define igual, `tk-player` no intenta incrustar: mpv abre su
propia ventana y la pantalla del reproductor avisa (`player.embed.own_window`).

## Deuda: lo que queda frágil

1. **Dependemos de que WebKitGTK componga con alfa sobre un widget GL hermano.** Es comportamiento de GTK 3 +
   WebKitGTK, no un contrato. Un cambio de renderizador en WebKitGTK (o un driver que exija desactivar DMA-BUF,
   p. ej. algunos NVIDIA) nos deja en la ventana aparte.
2. **Copias de más.** GTK 3 copia el FBO del `GLArea` a la superficie de la ventana y WebKit compone la página
   encima: no hay *direct scanout* ni *zero-copy* hasta el compositor. Funciona, pero gasta GPU en 4K/HDR.
3. **Sin HDR real.** El camino GTK 3 / GL es SDR de 8 bits.
4. **El render ocurre en el hilo de la UI.** Las llamadas síncronas a mpv desde ese hilo (`stop`, `loadfile`)
   pueden esperar al hilo de video; hoy no se observó bloqueo, pero es un riesgo conocido del Render API cuando
   el hilo de render es el principal.

## Alternativas a evaluar más adelante (sin probar)

| Opción | Idea | Bloqueos / costo |
|---|---|---|
| GTK 4 `GraphicsOffload` / subsuperficies Wayland | mpv entrega dmabufs que GTK 4 pone en una `wl_subsurface` para *direct scanout* (lo que hace tsukimi desde 2026, solo Wayland). | wry 0.53 / tao 0.34 / WebKitGTK 4.1 son GTK 3. Requiere WebKitGTK 6 (GTK 4) y que wry/tao lo soporten, o un shell propio sin Dioxus desktop. Reescritura de la capa de ventana. |
| `wl_subsurface` propia bajo la ventana GTK 3 | Crear una subsuperficie Wayland con `vo=gpu-next` (o `dmabuf-wayland`) debajo de la superficie de la ventana y abrir el hueco con transparencia. | GTK 3 no expone subsuperficies; habría que tocar `wl_surface` de GDK a mano y la ventana tendría que ser transparente (parpadeo con WebKitGTK, commit `4813b3d`). Solo Wayland. |
| Texturas compartidas hacia WebKit | Pasar los frames de mpv a la página (p. ej. WebGL / `<video>` con un *media source* propio). | Copias extra CPU/GPU, latencia, sin uosc. Peor que lo actual. |
| Otro motor web (WPE WebKit, Servo, Blitz) | Motor embebible con control explícito de capas. | Inmaduro para esta UI o sin bindings Rust estables; cambiaría todo el frontend. |
| Ventana de mpv aparte (modo externo) | Ya existe (`PlayerModeVm::External`) y es el respaldo automático. | No es "dentro de la ventana". |

Plan sugerido cuando se retome: (1) seguir la adopción de GTK 4 / WebKitGTK 6 en wry y tao; (2) leer cómo tsukimi
monta mpv bajo `GraphicsOffload`; (3) prototipar con `linux_embed_probe` antes de tocar la app; (4) medir uso de
GPU frente al camino actual con un video 4K.

Referencias: tsukimi v26.7.3 (mpv en GTK 4 con `GraphicsOffload`), gst-plugins-rs#441 (subsuperficies en
gtk4paintablesink), blog de Igalia sobre el renderizado DMA-BUF de WebKitGTK, dioxus#3667 / #4528 (por qué Dioxus
desactiva DMA-BUF).

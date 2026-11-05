# Auditoría del puente de compatibilidad Aniyomi (DartotsuExtensionBridge)

- Repositorio auditado: `aayush2622/DartotsuExtensionBridge`
- Commit: `7778da72eb450d5108b9672d194414c74bd6caf9` (v2.0.17 desktop / 2.0.12 Android según el pin de Tatakai)
- Alcance: lo que termina dentro de `aniyomiDesktop-plugin.jar` y `aniyomiAndroid-plugin.apk`: `runtimeManager/` (scripts de build, catálogos de versiones, wrapper), `aniyomi/{aniyomiCommon,aniyomiDesktop,aniyomiAndroid}` y `libraries/{common,commonLib,commonDesktopLib}`. Fuera de alcance: cloudStream, kotatsu, ireader, tsundoku, código Flutter/Dart del repo raíz.
- Método: lectura estática del código y de los scripts (no se ejecutó nada del repositorio) más comparación del checksum del wrapper contra los publicados por Gradle.
- Limitación: no se auditaron las clases de las dependencias de terceros ni se desensambló el contenido de `android-jar-1.0.0.jar` más allá de listar sus paquetes. Tampoco se comparó contra los binarios hospedados actualmente en `Snozxyx/TatakaiServer`.

## Veredicto

**Aceptable para compilar desde fuente, con mitigaciones.** No se encontró código malicioso ni indicios de exfiltración: no hay telemetría, no hay `Runtime.exec`/`ProcessBuilder`, no hay tareas `Exec` ni descargas en tiempo de build dentro de los scripts del proyecto, no hay librerías nativas ni blobs ofuscados en el código del bridge. Los riesgos reales son de cadena de suministro (dependencias sin verificación criptográfica, un repositorio JitPack, un `.jar` binario versionado) y de superficie en ejecución (descarga sin hash de un runtime nativo, servidor local en todas las interfaces). Ninguno es Crítico. Hay 3 hallazgos de severidad Media y el resto son Bajos o Informativos.

## Hallazgos

### M1 (Media) Sin verificación de integridad de dependencias ni lockfiles
No existe `gradle/verification-metadata.xml` ni lockfiles. Los artefactos Maven se confían por TLS al repositorio. Además `gradle-wrapper.properties` no define `distributionSha256Sum`, de modo que la distribución de Gradle 9.6.1 se descarga sin checksum fijado (solo HTTPS a `services.gradle.org`). Mitigación en el workflow: validación del `gradle-wrapper.jar` con `gradle/actions/setup-gradle`, `--no-build-cache`, sin caché de Gradle. Recomendado a futuro: generar `verification-metadata.xml` con `--write-verification-metadata sha256` en un commit propio y verificarlo en el workflow.

### M2 (Media) Repositorio JitPack y dependencia por hash de commit
`settings.gradle.kts` añade `maven("https://jitpack.io")`. Se usa para `com.github.null2264:injekt-koin:ee267b2e27`, un artefacto que JitPack compila bajo demanda desde GitHub (versión = prefijo de commit). Riesgo: el contenido depende de lo que JitPack construyó/cacheó; la cuenta origen puede reescribir o borrar el repositorio, y JitPack no firma. Además el orden de repositorios es google, mavenCentral, jitpack, jogamp, jetbrains sin `content { includeGroup(...) }`, así que cualquier módulo no resuelto en los dos primeros se consulta primero en JitPack (riesgo de confusión de dependencias, bajo en la práctica porque JitPack solo sirve grupos `com.github.*`/dominios propios).

### M3 (Media) Descarga sin verificación de un runtime nativo (JBR/CEF) en tiempo de ejecución
`CefHelper.kt` (`commonDesktopLib`) descarga `jbr_jcef-25.0.4-<os>-<arch>-b508.27.tar.gz` desde `https://cache-redirector.jetbrains.com/intellij-jbr/` a `~/.dartotsu/kcef` y carga sus librerías nativas. No hay hash, firma ni pin. Se activa solo cuando una extensión necesita el WebView/Cloudflare (ruta CEF) en desktop, pero es código nativo ejecutado dentro del proceso del bridge y queda fuera del pin de Tatakai. Recomendación: en Tatakai no habilitar esa ruta, o proveer el runtime ya verificado; valorar parchear el bridge para fijar SHA-256.

### B1 (Baja) JAR binario versionado en el repo: `libraries/commonDesktopLib/libs/android-jar-1.0.0.jar`
Archivo de 19 983 825 bytes, SHA-256 `88da0ae3c71af66c7be9a8d45a86297ba6c35b6219ee67c427211d77aad2ec65`. Se empaqueta dentro del JAR de desktop (`api(files(...))`). El manifest indica `Created-By: soong_zip` (herramienta de AOSP) y el contenido listado son paquetes `android/*`, `dalvik/*`, `androidx/annotation` y recursos; 3101 clases, sin `.so`/`.dex`/`.jar` anidados ni namespaces ajenos. Es coherente con un framework Android recompilado, pero su procedencia no es verificable desde el repo (no se pudo inspeccionar su historial, el checkout es superficial). Los otros 8 JARs de `libs/` están comentados en `build.gradle.kts` y no se empaquetan. Recomendación: rastrear su origen y fijar este hash en la revisión.

### B2 (Baja) Servidor HTTP en todas las interfaces
`HttpServer : NanoHTTPD(0)` (`aniyomiCommon`) escucha en puerto efímero sin hostname, es decir en la dirección comodín. Lo arranca `AnimeSourceMethods.kt` cuando una fuente lo pide (`createHttpServer()`), y sirve contenido de video proxificado; la URL que se entrega usa `localhost`, pero el socket es alcanzable desde la LAN mientras dura la reproducción. Recomendación: parche para `NanoHTTPD("127.0.0.1", 0)` o aislamiento por firewall.

### B3 (Baja) Provisionamiento automático de JDK por foojay
`settings.gradle.kts` aplica `org.gradle.toolchains.foojay-resolver-convention:1.0.0` y `gradle/gradle-daemon-jvm.properties` apunta a `api.foojay.io` para un JetBrains JDK 21 (sin checksum). Mitigación en el workflow: se elimina ese archivo, se corre sobre Temurin 17 y se pasa `-Porg.gradle.java.installations.auto-download=false`.

### B4 (Baja) Repositorios adicionales
- `https://jogamp.org/deployment/maven`: Bajo; probablemente innecesario para Aniyomi, sin uso evidente.
- `https://packages.jetbrains.team/maven/p/ij/intellij-dependencies`: Bajo; necesario para `org.jetbrains.intellij.deps.jcef:jcef` (solo API, sin nativos).
- `google()`, `mavenCentral()`, `gradlePluginPortal()`: aceptados.

### B5 (Baja) Artefacto Android sin firma
Ni `aniyomiAndroid/build.gradle.kts` ni `plugin-build.gradle.kts` definen `signingConfig`; `assembleRelease` produce un APK sin firmar (`isMinifyEnabled = false`). El APK actual del pin debería estar firmado por alguien. Antes de reemplazar, comprobar cómo `AniyomiAndroidEngine.java` carga el APK del bridge (el verificador `apksig` visible en el código es para los APK de extensiones); si se exige firma, hay que firmarlo con una clave propia en un paso aparte y pinear el certificado.

### I1 (Informativa) Ejecución de código de terceros por diseño
El bridge carga extensiones arbitrarias: `URLClassLoader`/`ChildFirstURLClassLoader`/`PathClassLoader`, `Class.forName`, conversión dex a jar con dex2jar y reescritura de bytecode con ASM (`BytecodeEditor.kt`, `PackageTools.kt`, `AnimeExtensionLoader.android.kt`). Es la función del producto, no un defecto; el riesgo es el de las extensiones, no el del bridge.

### I2 (Informativa) Endpoints y red
Solo hay endpoints locales: `http://127.0.0.1:4567` (`Flutter.kt`), `http://127.0.0.1:8090` (utilidad de torrent) y `http://localhost:<puerto>`. La única URL externa codificada es la del JBR (M3). El tráfico de extensiones usa OkHttp con proxy configurable (`FlutterNetwork`/`FlutterProxy`). Búsquedas negativas: sin telemetría/analytics/Sentry/Firebase, sin `Runtime.exec`, `ProcessBuilder`, `System.load` propio, ni Base64 de payloads. `AndroidManifest.xml` del APK no declara permisos ni componentes.

### I3 (Informativa) Versiones
No hay versiones dinámicas (`+`, rangos, `SNAPSHOT`) en los catálogos. Plugins de Gradle fijos (Kotlin 2.4.0, AGP 9.2.1, Shadow 9.4.2, foojay 1.0.0). El `gradle-wrapper.jar` del repo (SHA-256 `76805e32...93f3`) no es el de 9.6.1 (`497c8c2a...a9c7`) pero coincide exactamente con el wrapper oficial de Gradle 9.0.0 y 9.1.0, así que es un JAR legítimo y anticuado; `validate-wrappers` lo aprueba. Licencia: Unabandon Public License (GPLv3 con obligación de publicar fuente); al redistribuir los binarios desde el fork se adjuntan la licencia y el commit fuente, que cumplen como fuente correspondiente.

### I4 (Informativa) Versión 2.0.17 / 2.0.12 no proviene del código
`plugins.json` y los números de versión los calcula la CI upstream (`scripts/prepare_release.py`): compara el SHA-256 del artefacto con el de la release `latest` anterior y, si cambió, suma 1 a `versionCode` y al parche de `versionName`. Los JAR/APK no contienen esa versión (los valores de Gradle `versionName = "1.0"` y el JSON de metadata son marcadores). Una compilación propia no reproduce esos números; son una etiqueta de la release.

## Tareas Gradle que generan los activos

| Activo | Tarea | Salida |
| --- | --- | --- |
| `aniyomiDesktop-plugin.jar` | `:aniyomi:aniyomiDesktop:buildPlugin` (depende de `shadowJar`) | `runtimeManager/aniyomi/aniyomiDesktop/build/libs/aniyomiDesktop-all.jar`, copiado a `runtimeManager/builds/aniyomiDesktop/aniyomiDesktop-plugin.jar` |
| `aniyomiAndroid-plugin.apk` | `:aniyomi:aniyomiAndroid:buildPlugin` (depende de `assembleRelease`) | `.../build/outputs/apk/release/*release*.apk`, copiado a `runtimeManager/builds/aniyomiAndroid/aniyomiAndroid-plugin.apk` |

`buildPlugin` está definido en `plugin-build.gradle.kts`. El upstream usa `buildEverything` (todos los plugins más `embedded-bridge.jar`); no se necesita para estos dos activos.

Nota: `aniyomiCommon/build.gradle.kts` decide si añade `compileOnly(commonDesktopLib)` según si algún nombre de tarea contiene "Android". Con `buildEverything` es falso para ambos; con tarea explícita de Android es verdadero. El workflow compila desktop y Android en invocaciones separadas; el efecto esperado es solo en el classpath de compilación, pero no está verificado.

## Reproducibilidad

No se espera igualdad byte a byte con los activos upstream: la imagen de CI y el JDK de upstream no están fijados (el daemon usaba JetBrains 21 vía foojay), JitPack y los repositorios Maven pueden haber cambiado, el JAR de Shadow y el APK incluyen metadatos y orden de entradas dependientes de la versión de herramientas, y el APK propio saldría sin firmar. Lo correcto es tratar la compilación propia como un artefacto nuevo: revisar, medir tamaño/contenido contra el pin actual, y reemplazar el pin con los hashes del workflow (más atestación de procedencia).

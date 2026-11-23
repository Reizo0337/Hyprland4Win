# hypr-win

Prototipo de gestor de ventanas en mosaico para Windows 11, inspirado en Hyprland.
Rust + la crate oficial `windows`; sin Electron, Tauri ni .NET. Solo APIs Win32 documentadas.

## Requisitos

- Windows 11 (probado en 10.0.26200).
- Rust estable (`rustup`) con el target `x86_64-pc-windows-msvc`.
- Visual Studio Build Tools con el componente "Desarrollo para el escritorio con C++" (linker MSVC).

## Compilar y ejecutar

```powershell
cargo build --release
.\target\release\hypr-win.exe            # usa config.toml junto al exe o en el directorio actual
.\target\release\hypr-win.exe mi.toml    # o una ruta explícita
```

Si no se encuentra ningún `config.toml`, se usa el que va embebido en el binario.
Salir: `Super+Shift+E` (configurable; siempre existe uno) o `Ctrl+C` si lo lanzaste desde un terminal. Al salir, las ventanas vuelven a su posición original.

Comprobaciones:

```powershell
cargo clippy --all-targets -- -D warnings
cargo test
```

Los tests de layouts, parseo de atajos y validación de configuración no tocan el escritorio.

## Estructura

```
src/main.rs       arranque, bucle de mensajes, limpieza al salir
src/lib.rs        módulos de la biblioteca
src/config.rs     carga y validación de config.toml
src/window.rs     enumeración, filtros, hooks (SetWinEventHook) y colocación (DeferWindowPos)
src/layout/       trait Layout + dwindle + master (funciones puras)
src/workspace.rs  modelo de workspaces y búsqueda de vecinos (puro)
src/keybinds.rs   parseo de atajos y RegisterHotKey
src/bar.rs        ventana appbar y dibujo GDI
src/palette.rs    paleta estilo Spotlight genérica (Handler + render)
src/launcher.rs   lanzador de aplicaciones (Super+K): Handler de apps
src/menu.rs       menú del sistema (Super+Espacio): niveles, atajos, instalar, web, sistema
src/webapp.rs     aplicaciones web (acceso directo + favicon -> .ico)
src/theme.rs      temas: predefinidos, archivos de usuario, contraste
src/wallpaper.rs  fondo de escritorio (IDesktopWallpaper), degradados generados
src/autostart.rs   inicio con Windows (clave Run de HKCU)
src/updater.rs    actualizaciones desde las releases de GitHub (comprobar, verificar SHA-256, reemplazar el exe)
src/screensaver.rs  salvapantallas propio: escena animada + widgets; no gestiona contraseñas
src/mem.rs        recorte del conjunto de trabajo (memoria devuelta a Windows)
src/livewall.rs   fondo animado: ventana por monitor con animación procedural, escena samurái o GIF
src/samurai.rs    escena animada «Samurái» (pura); samurai_data.rs = sprite CC0 generado
src/iconset_bushido.rs  tabla del pack de iconos «Bushidō» (generada)
src/widgets.rs    widgets de escritorio: ventanas, arrastre, disposición guardada
src/widget_draw.rs  dibujo puro de cada widget (reloj, calendario, sistema, tiempo, música, notas, atajos)
src/sysinfo.rs    CPU, memoria, disco y red; src/weather.rs (Open-Meteo); src/media.rs (GSMTC); src/net.rs (WinINet)
src/login.rs      directiva «fondo nítido» del inicio de sesión
src/settings.rs   registro de ajustes editables y su validación
src/packages.rs   winget y Chocolatey: comandos de instalar/actualizar en consola
src/pkgsearch.rs  búsqueda en vivo de paquetes (winget + choco) en segundo plano
src/installer.rs  instalar/actualizar sin consola (winget oculto, choco con runas) + resultado
src/toast.rs      notificaciones flotantes (cualquier hilo -> ventana con alfa por píxel)
src/panel.rs      panel de control: Wi-Fi, sonido, batería (Super+I)
src/dock.rs       dock estilo Apple (layout y magnificación puros + ventana)
src/wifi.rs       WLAN nativa: estado, redes, conexión, radio
src/audio.rs      volumen con Core Audio
src/power.rs      batería
src/status.rs     estado que muestra la barra
src/icons.rs      iconos reales de apps (shell -> GDI+)
src/appid.rs      identidad de app y correspondencia con AppsFolder (pura)
src/glyphs.rs     glifos vectoriales (Wi-Fi, volumen, batería)
src/apps.rs       índice de aplicaciones (shell:AppsFolder)
src/search.rs     búsqueda difusa (pura)
src/animator.rs   hilo de animaciones sincronizado con el vsync
src/anim.rs       curva y interpolación (pura)
src/app.rs        pegamento: eventos, hotkeys, cambio de workspace, restauración
tools/            build_icon_packs.py, build_bushido_icons.py (pack propio), build_samurai_sprite.py
tests/layouts.rs  tests de layouts, atajos y configuración
```

## Ejemplo de `config.toml`

```toml
binds = [
    "Super+Return, exec, wt.exe",
    "Super+Q, close",
    "Super+Left, focus, left",
    "Super+Shift+Left, move, left",
    "Super+F, fullscreen",
    "Super+Space, togglelayout",
    "Super+Shift+E, quit",
    "Super+1, workspace, 1",
    "Super+Shift+1, movetoworkspace, 1",
]

[general]
layout = "dwindle"   # o "master"
workspaces = 9
gaps_in = 8
gaps_out = 8
master_ratio = 0.55

[bar]
height = 32
background = "#1e1e2e"
foreground = "#cdd6f4"
dim = "#6c7086"
accent = "#89b4fa"
accent_text = "#1e1e2e"
font = "Segoe UI"
font_size = 10
```

`binds` debe ir **antes** de cualquier `[tabla]`. Formato: `"Mods+Tecla, acción, argumento"`.
Acciones: `exec`, `close`, `focus`, `move`, `workspace`, `movetoworkspace`, `fullscreen`, `togglelayout`, `quit`.
El `config.toml` del repositorio incluye los atajos mínimos (Enter, Q, flechas, 1..9, Shift+1..9, F).

## Cómo funciona

- **Ventanas gestionables**: visibles, sin propietario, con título y marco redimensionable, sin `WS_EX_TOOLWINDOW`/`NOACTIVATE`, no *cloaked* y de procesos no elevados. El resto se ignora.
- **Eventos**: `SetWinEventHook` con `WINEVENT_OUTOFCONTEXT` (no se inyecta nada en otros procesos).
- **Workspaces (estilo Hyprland)**: son globales y cada monitor muestra uno. El monitor 1 arranca en el workspace 1, el 2 en el 2, etc. `Super+N` enfoca el monitor que ya muestra el workspace N; si ninguno lo muestra, el monitor enfocado pasa a mostrarlo (las ventanas del que deja se mueven a (-32000, -32000)). Cada workspace conserva su orden, layout y foco.
- **Multi-monitor**: una barra y un área de trabajo por monitor. Arrastrar una ventana a otro monitor la pasa al workspace de ese monitor; `Super+Flechas` cruza al monitor vecino y `Super+Shift+Flechas` en el borde le envía la ventana. Los monitores se ordenan: principal primero, luego de izquierda a derecha. **Conectar o desconectar un monitor se aplica en vivo, sin reiniciar**: los monitores que siguen conservan su workspace, el nuevo recupera el que tenía (o toma uno libre), las barras y docks se crean o se quitan, y los workspaces de un monitor desconectado se ocultan sin perder ventanas. Cada monitor se identifica por el nombre de su dispositivo (`\\.\DISPLAYn`), no por su posición. Los widgets pertenecen a un monitor concreto: si se desconecta se ocultan sin moverse y reaparecen donde estaban al volver.
- **Animaciones**: un hilo propio (`src/animator.rs`) sincronizado con el vsync del monitor (`DwmFlush`, que solo espera). Duerme sin consumir CPU cuando no hay nada animándose. Los fotogramas intermedios usan `SWP_ASYNCWINDOWPOS` para que una app lenta no bloquee a las demás; el último es síncrono para que el destino sea exacto. Curva: la "decelerate" de Fluent (`cubic-bezier(0.1, 0.9, 0.2, 1)`), la misma familia que usa Windows 11. Qué se anima: recolocar al abrir/cerrar/mover/intercambiar/pantalla completa, pop-in (94 % -> 100 %) de ventanas nuevas, deslizamiento al cambiar de workspace y deslizamiento del resaltado de la barra. El deslizamiento va por el eje donde no hay otro monitor al lado (con dos pantallas en horizontal, vertical) para no invadir la otra; si no hay eje libre, pop-in. `general.animation_ms` (0 las desactiva).
- **Animaciones de Windows**: `disable_windows_animations = true` apaga la animación de minimizar/maximizar (`SPI_SETANIMATION`) mientras hypr-win corre; no se guarda en el registro y se restaura al salir. Los fundidos de abrir/cerrar los hace el DWM por ventana (`DWMWA_TRANSITIONS_FORCEDISABLED`) o con el interruptor global de Windows (Configuración > Accesibilidad > Efectos visuales > Efectos de animación); no se tocan desde aquí por la restricción de no intervenir en DWM.
- **Barra de tareas de Windows**: con `hide_taskbar = true` se oculta (`ShowWindow(SW_HIDE)` sobre `Shell_TrayWnd` y `Shell_SecondaryTrayWnd`) y se vuelve a mostrar al salir. Esto toca ventanas del proceso `explorer.exe`; es una excepción deliberada a la restricción inicial. Un temporizador de 1 s la oculta de nuevo si Explorer la reaparece. Windows sigue reservando su hueco en el área de trabajo, así que el mosaico usa el monitor entero menos nuestra barra. Si hypr-win muere sin salir limpio: `hypr-win.exe --restore-taskbar`. Se pierden la bandeja del sistema, las notificaciones y el reloj nativos.
- **Start, búsqueda, widgets...**: `block_start_menu = true` traga la tecla Win sola (menú Inicio). Cualquier combinación se puede anular con la acción `none`, p. ej. `"Super+S, none"` (búsqueda), `Super+W` (widgets), `Super+A` (ajustes rápidos), `Super+N` (notificaciones). `Win+R` sigue funcionando para lanzar programas.
- **Estilo macOS**: la barra imita la barra de menús (translúcida, workspaces como puntos con una pastilla activa que se desliza, título a la izquierda, fecha y hora a la derecha) y el lanzador imita Spotlight (tarjeta con esquinas redondeadas y sombra, lupa, campo grande, cabecera "Aplicaciones", fichas de color con la inicial, selección en pastilla azul y `↵`). Todo se dibuja con GDI+ (`src/gfx.rs`) en ventanas propias con transparencia por píxel (`UpdateLayeredWindow`): no se usa DWM para nada. La paleta es la del sistema de Apple, en modo oscuro por defecto (preset claro en `config.toml`); la fuente por defecto es *Segoe UI Variable Text*, lo más cercano a San Francisco que trae Windows 11 (San Francisco no se puede redistribuir). `bar.opacity` regula la translucidez. Vista previa sin tocar el escritorio: `cargo run --example preview -- <carpeta> [claro]` genera PNG de la barra y el lanzador.
- **Lanzador (`Super+K`, también `Super+D`)**: la paleta estilo Spotlight de macOS (`src/palette.rs`). Empieza como una **barra de búsqueda sola** (con lupa y texto grande); al escribir aparecen los resultados con **iconos reales** de las apps, la sección "Mejor resultado" y luego "Aplicaciones", y la etiqueta "Aplicación" a la derecha de cada fila (la seleccionada muestra `↵`). `↓` o `Tab` con la barra vacía muestran todas las apps. Búsqueda difusa (exacto > prefijo > inicio de palabra > subcadena > letras en orden; `vsc` encuentra Visual Studio Code); `↑/↓/Tab` mueven la selección, `Enter` lanza, `Esc` cierra y devuelve el foco, `Ctrl+Backspace` borra, clic y rueda funcionan. El índice sale de `shell:AppsFolder` (apps de escritorio y de la Tienda). El foco por ratón se pausa mientras está abierto.
- **Menú (`Super+Espacio`)**, al estilo del menú de Omarchy: la misma paleta, por niveles (`Esc` o `Retroceso` con el campo vacío suben un nivel; escribir filtra). El layout pasa a `Super+Shift+Espacio`.
  - **Aplicaciones**: abre el lanzador.
  - **Instalar**: con la barra vacía, una selección de programas habituales por categorías, cada uno con su **icono** (el real si ya está instalado y, si no, el favicon del fabricante, que se descarga en segundo plano y se guarda en `%APPDATA%\hypr-win\pkgicons`; si un sitio no tiene favicon, como VLC, queda la ficha de color). **Al escribir (2 letras o más) busca solo**, en segundo plano y tras una pausa de 350 ms para no buscar en cada tecla: primero la sección **winget** (`winget search`) y debajo la de **Chocolatey** (`choco search`, reordenado por relevancia y con el paquete de nombre exacto primero), hasta 8 resultados de cada una. Mientras llegan se ve "Buscando en winget…". Los resultados quedan en caché, así que volver a una consulta es instantáneo. Elegir un resultado lo instala **en segundo plano, sin ventana de consola**, con su id exacto (`src/installer.rs`): winget se ejecuta oculto (`--silent`) con la salida capturada; Chocolatey, que necesita administrador, se lanza oculto con el verbo `runas` (el aviso de UAC lo muestra Windows y no se puede ocultar; si lo rechazas se avisa de que se canceló). El progreso y el resultado se ven en una **notificación flotante** estilo macOS arriba a la derecha ("Instalando X", "X instalado", o el motivo del fallo; un clic la descarta), y al terminar se refresca el índice de apps para que aparezca en el lanzador y el dock. También "Actualizar aplicaciones" (winget o Chocolatey) corre en segundo plano. Cada gestor solo aparece si está instalado; si no hay ninguno, el menú lo avisa. No se instala Chocolatey automáticamente: su instalación ejecuta un script descargado de internet, y eso lo decides tú (https://chocolatey.org/install). Los textos escritos se sanean antes de llegar a la línea de comandos; la tabla de winget se interpreta por posición (sus títulos de columna cambian con el idioma).
  - **Aplicaciones web**: crea una app a partir de una página (YouTube, Spotify, Classroom, WhatsApp, Gmail, ChatGPT... sugeridas, o "Nueva aplicación web…" con nombre y dirección). Es un acceso directo en el menú Inicio (`Hypr Web Apps`) que abre `msedge --app=URL` (o Chrome/Brave) con el **favicon como icono** (Google, con DuckDuckGo de respaldo), así que aparece en el lanzador, se puede fijar en el dock y se tilea como cualquier ventana. También se pueden abrir y eliminar desde ahí.
  - **Atajos**: lista todos los atajos con lo que hacen; al elegir uno, **pulsa la nueva combinación** (el hook de teclado captura también las combinaciones con Super sin que Windows las ejecute). Avisa si la combinación ya está en uso o no incluye Super/Ctrl/Alt, y el cambio se **guarda en `config.toml` y se aplica al instante**. También abre `config.toml` para editar a mano.
  - **Panel de control**: abre el panel (Wi-Fi, Bluetooth, sonido).
  - **Sistema**: bloquear, suspender, cerrar sesión, reiniciar, apagar (con confirmación), actualizar aplicaciones (`winget upgrade --all`) y salir de hypr-win.
  - **Temas** (`src/theme.rs`): 12 predefinidos (Apple Oscuro/Claro, Catppuccin Mocha/Latte, Nord, Dracula, Gruvbox, Tokyo Night, Rosé Pine, Solarized, Everforest, Grafito) con una muestra de color cada uno. Elegir uno **cambia en vivo toda la interfaz** (barra, dock, lanzador, menú, panel y avisos) y lo guarda en `[bar] theme`. **Crear un tema** (parte del actual) y **editar cada propiedad**: fondo, texto, texto secundario, acento, texto sobre el acento, fuente, **iconos** y opacidad. **Packs de iconos** (`src/iconset.rs`, `assets/icons`): Phosphor (regular, grueso y relleno), Fluent UI System Icons de Microsoft (regular y relleno, el estilo de Windows 11) y Tabler (todos MIT) y Lucide (ISC), incrustados como fuentes recortadas de 10-20 KB cada una (`tools/build_icon_packs.py` las regenera), más los de Windows (*Segoe Fluent Icons*, que no se redistribuyen). Cada tema lleva el suyo (`icons` en su archivo, o `[bar] icons` sin tema); se cambia en vivo en Temas > Editar el tema actual > Iconos y afecta a los menús, avisos y widgets, y también a los **iconos de estado de la barra y del panel** (Wi-Fi, volumen, batería, Bluetooth, candado y tic; sin pack se dibujan los vectoriales de siempre). **Iconos de las aplicaciones del dock**: cada tema tiene un estilo (`app_icons`): *originales*, *en ficha* (cada icono sobre una ficha redondeada del color del tema), *monocromo* (tonos del color del texto) o *acento* (tonos del acento); se elige en Temas > Editar el tema actual > Iconos de las aplicaciones. Además puedes poner **iconos propios**: un `.png`, `.ico` o `.jpg` con el nombre de la app o de su ejecutable (`Spotify.png`, `code.ico`) en `%APPDATA%\hypr-win\icons` (el menú abre la carpeta y tiene "Recargar los iconos"); mandan sobre el estilo. Los colores se escriben como `#rrggbb` o `#rgb`; para el acento hay una paleta de Apple, y el texto sobre el acento se ajusta solo por contraste (WCAG). Editar un tema predefinido crea una copia tuya ("Mi Nord") sin tocar el original. Los temas tuyos son archivos TOML en `%APPDATA%\hypr-win\themes` que puedes compartir o editar a mano; uno con el nombre de un predefinido lo sustituye.
  - **Fondo de pantalla** (`src/wallpaper.rs`): usa la interfaz documentada `IDesktopWallpaper`, así que admite un **fondo distinto por monitor** ("Aplicar a"). Imágenes de tu carpeta (por defecto `Imágenes\Wallpapers`, configurable), las generadas y las que trae Windows, con **miniaturas** (se generan en segundo plano y se guardan en caché). **Fondos generados con los colores del tema**: Degradado, Resplandor, Aurora y Color liso (1920x1080, con ruido para evitar bandas). Fondo al azar, ajuste (rellenar, ajustar, expandir, mosaico, centrar, abarcar), **cambio automático cada N minutos** y "cambiar el fondo con el tema" (opcional, apagado por defecto). La primera vez que se cambia se guarda el fondo que tenías y se puede **restaurar** desde el menú. Windows aplica el fondo de forma asíncrona (~0,5 s). **La misma imagen se aplica también a la pantalla de bloqueo (`Win+L`)** con la API documentada de WinRT `LockScreen.SetImageFileAsync` (opción `[wallpaper] lock_screen`, activada por defecto, con interruptor en el menú): se usa al aplicar a todos los monitores o al primero, y también en el cambio automático y al seguir el tema. Ojo: si tu pantalla de bloqueo usa *Windows Spotlight*, al aplicar un fondo Windows pasa a "Imagen"; esa API no puede volver a Spotlight (se hace en Configuración > Personalización > Pantalla de bloqueo). hypr-win guarda la imagen que había y "Restaurar la pantalla de bloqueo anterior" la recupera (como Imagen). Los fondos generados se guardan en `Imágenes\Wallpapers\Generados` porque `IDesktopWallpaper` rechaza imágenes de carpetas propias bajo `AppData`.
  - **Fondo animado** (`src/livewall.rs`): en Fondo > *Fondo animado* hay Degradado, Resplandor y Aurora **en movimiento** (se calculan en pequeño con los colores del tema, se amplían y siguen al tema en vivo) y cualquier **GIF animado** de tu carpeta de imágenes (con sus propios tiempos). Se guarda en `[wallpaper] animated` (`gradient`, `glow`, `aurora` o la ruta de un `.gif`) y `animated_fps` (5-60). Es una ventana por monitor, detrás de todas las demás y transparente al ratón; los widgets quedan por encima. **Límite:** con APIs documentadas no se puede colocar entre el fondo y los iconos, así que mientras está activo **tapa los iconos del escritorio**. Se probó el truco de `Progman`/`WorkerW` (ventanas hijas del escritorio, en capas o no) y en Windows 11 26200 no llega a verse, por eso no se usa. No se reproducen vídeos (haría falta Media Foundation). Se detiene desde el mismo menú; el fondo normal de Windows no se toca.
  - **Inicio con Windows** (`src/autostart.rs`): Menú > Sistema > *Iniciar con Windows* (o `hypr-win.exe --autostart=on` / `off`) guarda un valor en `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`: es la vía documentada, no necesita administrador y también se ve y se apaga en Configuración > Aplicaciones > Inicio. El comando lleva la ruta del `config.toml` en uso (al iniciar sesión el directorio actual es otro) y `--autostart`, que espera a que exista la barra de tareas de Windows antes de empezar a gestionar ventanas. La actualización automática reemplaza el exe en la misma ruta, así que el inicio sigue valiendo; si mueves el exe, vuelve a activarlo. **Sin consola:** `hypr-win.exe` es una aplicación de subsistema Windows, así que al arrancar solo (o con doble clic) no abre ningún terminal; si lo lanzas desde uno, la salida de texto y `Ctrl+C` siguen funcionando (el prompt vuelve enseguida, porque el programa ya no es una app de consola). Un error fatal al iniciar se guarda en `%APPDATA%\hypr-win\hypr-win.log` y, si no hay terminal, se muestra en un aviso. **Siempre hay un atajo para salir:** `Super+Shift+E` (`quit`); si tu `config.toml` no define ninguno, se añade ese automáticamente.
  - **Salvapantallas** (`src/screensaver.rs`; Menú > Sistema > *Salvapantallas*, o `Super+Shift+L`): una escena animada (la que tengas como fondo animado, o una Aurora ambiental con los colores del tema si no hay ninguna) con reloj, tiempo, música y sistema encima, en una ventana real por monitor, por encima de todo. Aparece con el atajo o, si pones `[screensaver] idle_minutes` por encima de 0, tras ese tiempo sin tocar teclado ni ratón (`GetLastInputInfo`, API documentada). **No gestiona contraseñas ni nada de la autenticación**: corre en tu sesión normal, sin privilegios, como cualquier otra ventana de hypr-win. Al tocar cualquier tecla o mover el ratón se oculta y, si `lock_on_wake` está activado (por defecto), pide a Windows que bloquee la sesión de verdad (`LockWorkStation`, la misma API de `Super+L`); la pantalla de contraseña que sale después la dibuja Windows por completo, con tu imagen de bloqueo si la configuraste (ver *Inicio de sesión* arriba). `Super+L` (sin Shift) sigue siendo el bloqueo directo de Windows, sin pasar por el salvapantallas. **Por qué no se hizo de otra forma:** la alternativa con control total (sustituir `LogonUI.exe`, el proceso que pinta la pantalla de contraseña, mediante `Image File Execution Options`) se ejecutaría como `SYSTEM` en el escritorio seguro, sin red ni tu perfil; un fallo ahí no se puede arreglar con `Ctrl+Alt+Supr` ni modo seguro, solo con un medio de recuperación externo, y ese mismo mecanismo es el que usan los ataques de bypass de inicio de sesión (el "truco de Sticky Keys"), así que cualquier antivirus lo marca como amenaza. No se implementa ni como opción.
  - **Actualizaciones** (`src/updater.rs`): al arrancar (como mucho una vez cada 24 h; se desactiva con `[updates] check = false`) y en Menú > Sistema > *Buscar actualizaciones* se consulta la última release de GitHub. Si es más nueva, sale un aviso y aparece *Actualizar a vX.Y.Z*, que descarga `hypr-win.exe`, **verifica su SHA-256** (el `digest` del asset o el `hypr-win.exe.sha256` de la release; sin suma no se instala), renombra el exe en uso a `.old`, copia el nuevo y lo lanza con `--wait-pid=<pid>` para que espere a que este proceso cierre y recupere los atajos; si algo falla a medias, se deshace. Solo descarga de `github.com/Reizo0337/Hyprland4Win/releases/download/`, nunca instala sin que lo pidas, y si el exe está en una carpeta sin permiso de escritura (p. ej. `Program Files`) avisa para descargarlo a mano. **Publicar una versión:** subir la versión en `Cargo.toml`, compilar en release, y crear una release con el tag `vX.Y.Z` que lleve `hypr-win.exe` y `hypr-win.exe.sha256` (`certutil -hashfile hypr-win.exe SHA256`).
  - **Cursor del ratón** (`src/cursor.rs`): se cambia desde la aplicación, en Menú > Temas > Editar el tema actual > **Cursor del ratón**, y se aplica al momento a todo Windows (también al elegir un tema). Hay cursores generados con los colores del tema (Clásico, Contraste, Acento, Suave, Katana) y packs completos con flecha, mano, texto y "ocupado" (Rosé Pine, macOS, macOS blanco, Capitaine Gruvbox y Capitaine Nord); "Windows" deja los tuyos. Cada tema lleva el suyo (`cursor` en su archivo, o `[bar] cursor` sin tema). Se guarda en `HKCU\Control Panel\Cursors` y **se restauran los tuyos al salir** de hypr-win. Licencias de los packs: `assets/cursors`.
  - **Tema «Bushidō»** (oscuro, `src/theme.rs`): tinta, marfil y carmesí, con los colores sacados de la escena del fondo. Trae todo en uno y se aplica de una vez desde Temas: **fondo animado «Samurái»** (`src/samurai.rs`: luna, estrellas que titilan, tres cordilleras con niebla que se desplaza, un torii, un acantilado con el samurái pixel art en reposo y pétalos de cerezo; se calcula a 1/4 de resolución y se amplía sin suavizar, ~2,6 ms por fotograma, máx. 20 fps), **fuente Yu Mincho** (serif japonesa que ya trae Windows), **pack de iconos propio «Bushidō»** (56 iconos de trazo angular dibujados para el proyecto, `tools/build_bushido_icons.py`), **cursor «katana»** (flecha en forma de espada con los colores del tema), **barra «Tinta»** (numerales kanji 一 二 三… y un sello rojo que se desliza al workspace activo, con un filo carmesí que se desvanece) y **dock «Tinta»** (fichas casi negras con filo carmesí, iconos en marfil y marcador rojo alargado). Las piezas también se pueden usar por separado: la barra «Tinta» está en Temas > Diseño de la barra, el fondo en Fondo > Fondo animado, y los demás campos del tema son `icons`, `app_icons = "ink"`, `cursor = "katana"`, `style = "ink"` y `animated = "samurai"` (un tema tuyo en TOML puede usarlos). Aplicar otro tema devuelve la barra a «Minimal 1» si venía de «Tinta», pero no detiene el fondo animado. **Créditos:** el sprite del samurái es de DezrasDragons ([OpenGameArt](https://opengameart.org/content/samurai-animated), CC0); ver `assets/samurai/CREDITS.md`. Se descartó el pack «Starry Night» de SethByrd porque su `License.txt` prohíbe redistribuir los archivos tal cual, y las ilustraciones de Oyasumi (CC0) por ser claras y pastel, impropias de un tema oscuro.
  - **Widgets de escritorio** (menú > Widgets; `Super+Shift+W` los trae delante un momento): tarjetas translúcidas con el tema activo, que se **arrastran** con el ratón (se pegan a 8 px a los bordes y se guardan por monitor) y viven detrás de las ventanas. Reloj (S/M), Calendario (S/L), Sistema (CPU, memoria, disco y red), Tiempo (S/M), Música (con anterior/pausa/siguiente), Notas (M/L) y Atajos (L). Se pueden poner varios, quitar, ocultar y restablecer sus posiciones. La disposición está en `%APPDATA%\hypr-win\widgets.toml` y la nota en `notes.txt`. **Privacidad del tiempo**: solo el nombre de ciudad que escribes se envía a Open-Meteo (sin cuenta ni clave); sin ciudad no se hace ninguna petición. La música lee la sesión multimedia de Windows (GSMTC).
  - **Inicio de sesión** (menú > Inicio de sesión): el diseño de la ventana de contraseña lo dibuja Windows y **no se puede sustituir** con APIs seguras y documentadas. Sí se puede: poner el fondo del escritorio o una imagen generada con el tema como imagen de bloqueo/inicio de sesión (WinRT `LockScreen`), restaurar la anterior, activar la directiva de Windows «fondo de inicio de sesión nítido» (`DisableAcrylicBackgroundOnLogon`, necesita administrador: Windows muestra su aviso de UAC) y abrir las páginas de Configuración de pantalla de bloqueo y foto de cuenta. No se puede volver a *Windows Spotlight* por API. **Escena samurái:** *Inicio de sesión > Escena samurái (Bushidō)* renderiza un fotograma de la escena del tema (`src/samurai.rs`) a la resolución de tu monitor principal (1280x720 a 3840x2160, ampliada sin suavizar) y la pone como imagen de bloqueo e inicio de sesión; al elegir el tema Bushidō se aplica sola si `[wallpaper] lock_screen` está activado. Es una imagen fija (Windows no permite animar ahí) y la ventana de contraseña, el reloj y los botones los sigue dibujando Windows; con *Fondo nítido* (administrador) se ve sin el desenfoque acrílico.
  - **Ajustes** (`src/settings.rs`): 16 opciones editables sin abrir el archivo (huecos, ancho de la ventana principal, animaciones, foco por ratón, barra de tareas, tecla Win, alto y letra de la barra, tamaño/magnificación/auto-ocultado/apps fijadas del dock...). Los interruptores se cambian al momento; el resto pide el valor y lo valida (rangos y unidades). Se guarda en `config.toml` **conservando comentarios** y se **aplica en vivo** (releyendo y aplicando el archivo), salvo el número de escritorios y activar el dock, que piden reiniciar y lo avisan. Todo lo demás sigue en `config.toml` ("Abrir config.toml").
- **Estado del sistema en la barra y panel de control** (`Super+I` o clic en los iconos): Bluetooth, Wi-Fi, volumen y batería a la derecha del reloj, y un panel al estilo del Centro de control de macOS. Wi-Fi con la API WLAN nativa (`wlanapi`): encender/apagar la radio, lista de redes con señal y candado, conectar (las guardadas sin contraseña; las nuevas WPA2/WPA3-Personal y abiertas piden la contraseña en el propio panel y crean el perfil), desconectar. **Desde Windows 11 24H2 Windows devuelve "acceso denegado" a las apps que piden nombres de red o señal sin permiso de ubicación**: en ese caso el panel sigue mostrando que el adaptador está conectado y ofrece un botón que abre *Configuración > Privacidad > Ubicación* (activa los servicios de ubicación y "Permitir que las aplicaciones de escritorio accedan a tu ubicación"). Bluetooth: interruptor de la radio (`Windows.Devices.Radios`) y dispositivos conectados (API Win32 de Bluetooth). Sonido con Core Audio: slider de volumen, silencio y nombre de la salida (cambiar de dispositivo de salida solo se puede con interfaces COM no documentadas, así que no está). Batería con `GetSystemPowerStatus`. Red, Sonido y Bluetooth abren su página de Configuración. Se sondea cada 3 s en el hilo de la interfaz (Bluetooth cada ~9 s y al abrir el panel).
- **Dock estilo Apple** (`[dock]` en `config.toml`): pastilla translúcida centrada abajo, una en cada monitor, con los **iconos reales** de las apps (`IShellItemImageFactory`), magnificación con curva de campana al pasar el ratón, etiqueta con el nombre, punto bajo las apps abiertas, rebote al lanzar y separador antes de las abiertas que no están fijadas. Tamaño por defecto compacto (`icon_size = 36`, `magnification = 1.5`). Con `autohide = true` (por defecto) se desliza hacia abajo en cuanto hay ventanas en ese monitor y reaparece al dejar el ratón unos 400 ms en el borde inferior; se vuelve a ocultar 600 ms después de apartar el ratón. Con el auto-ocultado activo no reserva espacio (aparece por encima de las ventanas); con `autohide = false` reserva su franja inferior. Clic: enfoca la app (la siguiente ventana si ya tiene el foco; cambia de workspace si hace falta) o la lanza. Las apps fijadas se indican por nombre (`pinned`) y se resuelven con la misma búsqueda difusa del lanzador; las ventanas se asocian a su icono por AppUserModelID o por el nombre del ejecutable.
- **Atajos con Super**: los atiende un hook de teclado de bajo nivel (`WH_KEYBOARD_LL`, se ejecuta en nuestro hilo; no se inyecta en otros procesos), porque el shell se queda antes con `Win+Número` y abriría las apps ancladas. Al soltar Win se envía una tecla ficticia para que no salte el menú Inicio. Los atajos sin Super usan `RegisterHotKey`.
- **Alt+Tab propio** (`src/switcher.rs`, `alt_tab = true` en `[general]`, también en Menú > Ajustes): el hook de teclado traga el Alt+Tab de Windows y lo sustituye por una lista de tarjetas (icono y título de cada ventana de todos los escritorios) en el centro del monitor donde está el ratón, ordenada por **uso reciente** (la ventana activa primero, así que un toque rápido de Alt+Tab alterna entre las dos últimas). Con Alt pulsado: `Tab` / `→` / `↓` avanzan, `Shift+Tab` / `←` / `↑` retroceden, `Alt+Shift+Tab` empieza por la última, `Esc` cancela y **soltar Alt** activa la elegida (si está en otro escritorio, se cambia a él). Las **ventanas minimizadas** no se pierden: salen del mosaico pero siguen en el Alt+Tab (atenuadas y marcadas "minimizada") y en el dock como apps abiertas, y al elegirlas se restauran en su escritorio (también las que ya estaban minimizadas al arrancar hypr-win). La lista aparece tras ~110 ms: un toque rápido cambia de ventana sin enseñar nada. Usa el tema activo y no coge el foco. Con `alt_tab = false` vuelve el de Windows. Límites: no muestra miniaturas en vivo de las ventanas (solo iconos), y `Win+Tab` sigue siendo la vista de tareas de Windows.
- **Música en la barra**: cuando suena algo en cualquier reproductor (sesión multimedia de Windows, `GlobalSystemMediaTransportControls`, la misma que usa el widget de música), en el centro de la barra aparece una pastilla con una **onda animada** (en el color de acento) y el **título** de la canción. Desaparece al pausar o parar. Se comprueba cada segundo y la animación solo corre mientras suena algo. Funciona en los dos diseños de la barra; si no cabe en el centro (poco espacio entre lo de la izquierda y los iconos de estado), no se muestra.
- **Barra**: appbar superior por monitor (`ABM_NEW/QUERYPOS/SETPOS`); el área de trabajo del sistema la excluye, así que las ventanas maximizadas no la tapan.
- **Bordes invisibles**: se compensan con `DWMWA_EXTENDED_FRAME_BOUNDS` (solo lectura) para que los gaps sean exactos.

## Limitaciones conocidas

- **Menú**: instalar usa `winget` o Chocolatey (si no hay ninguno, el menú lo avisa). Si un instalador falla, la notificación solo da el código de salida o la última línea de winget; no hay registro completo. Algunos instaladores de winget también piden UAC por su cuenta. Las apps web necesitan Edge, Chrome o Brave. Al capturar un atajo, las teclas sin nombre en `config.toml` (p. ej. signos de puntuación) no se pueden usar. Cambiar de dispositivo de salida de audio sigue sin estar.

- **Dock**: hay uno en cada monitor gestionado, con el mismo contenido. Si dejas visible la barra de tareas de Windows (`hide_taskbar = false`) se superpondrá con ella. Los iconos de apps que no se pueden resolver (procesos sin ruta accesible) se muestran como ficha de color con la inicial.
- **Pantalla de bloqueo**: solo se cambia la imagen de `Win+L`; la pantalla de inicio de sesión tiene su propio ajuste en Windows. Spotlight no se puede reactivar por API.
- **Wi-Fi**: sin permiso de ubicación Windows no entrega nombres de red ni señal (ver arriba); las redes de empresa (802.1X) y WEP se configuran en Windows; encender/apagar la radio puede requerir permisos según el equipo.
- **Bluetooth**: el interruptor depende de que Windows permita el acceso a la radio; la lista solo muestra los dispositivos conectados (conectar/desconectar un dispositivo concreto no tiene API Win32 documentada).

- **Atajos con Win**: el hook de bajo nivel puede dejar de recibir teclas si el proceso se bloquea (Windows lo desinstala tras ~300 ms sin respuesta) y no ve las teclas de ventanas elevadas. Atajos que el sistema protege (`Ctrl+Alt+Supr`, `Win+L`) no se pueden capturar.
- **Cambio de monitores en caliente**: si cambia el número de monitores hay que reiniciar hypr-win (solo se recolocan barras y áreas si cambia la resolución). La barra usa el DPI del sistema en todos los monitores.
- Las animaciones redimensionan ventanas fotograma a fotograma; aplicaciones pesadas (navegadores, Electron) pueden ir a saltos aunque el movimiento sea fluido. Si hay lag, baja `animation_ms` o ponlo a 0.
- Con `hide_taskbar = false` las ventanas de workspaces ocultos siguen apareciendo en la barra de tareas nativa.
- "Pantalla completa" ocupa el área de trabajo sin gaps y deja visible la barra.
- Solo monitor principal. Ventanas elevadas no se gestionan (UIPI). Un cierre brusco (kill) deja las ventanas de otros workspaces fuera de pantalla; relanzar y salir normalmente no las recupera si ya estaban fuera.
- No hay redimensionado manual del reparto de los layouts ni animaciones.

## Fuera del prototipo (y por qué)

| Elemento | Motivo |
|---|---|
| Ocultar la barra de tareas nativa | Requisito explícito: mejora futura. |
| Escritorios virtuales de Windows | Solo hay interfaces COM no documentadas. |
| Animaciones | Exigen DWM/composición propia; fuera de las restricciones. |
| Multi-monitor avanzado | Cada monitor necesitaría su appbar, workspaces y reglas de foco. |
| Reglas por aplicación (flotantes, workspace fijo) | Fuera del alcance de las 5 fases. |
| Redimensionado interactivo de splits | Requiere un modelo de árbol con ratios; los layouts actuales son puros y sin estado. |
| Hook de teclado de bajo nivel | Alternativa a `RegisterHotKey` para Win+..., ver limitaciones. |

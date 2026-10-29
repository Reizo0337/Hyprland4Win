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
Salir: `Super+Shift+E` (configurable) o `Ctrl+C` en la consola. Al salir, las ventanas vuelven a su posición original.

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
src/packages.rs   winget y Chocolatey: comandos de instalar/actualizar en consola
src/pkgsearch.rs  búsqueda en vivo de paquetes (winget + choco) en segundo plano
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
- **Multi-monitor**: una barra y un área de trabajo por monitor. Arrastrar una ventana a otro monitor la pasa al workspace de ese monitor; `Super+Flechas` cruza al monitor vecino y `Super+Shift+Flechas` en el borde le envía la ventana. Los monitores se ordenan: principal primero, luego de izquierda a derecha.
- **Animaciones**: un hilo propio (`src/animator.rs`) sincronizado con el vsync del monitor (`DwmFlush`, que solo espera). Duerme sin consumir CPU cuando no hay nada animándose. Los fotogramas intermedios usan `SWP_ASYNCWINDOWPOS` para que una app lenta no bloquee a las demás; el último es síncrono para que el destino sea exacto. Curva: la "decelerate" de Fluent (`cubic-bezier(0.1, 0.9, 0.2, 1)`), la misma familia que usa Windows 11. Qué se anima: recolocar al abrir/cerrar/mover/intercambiar/pantalla completa, pop-in (94 % -> 100 %) de ventanas nuevas, deslizamiento al cambiar de workspace y deslizamiento del resaltado de la barra. El deslizamiento va por el eje donde no hay otro monitor al lado (con dos pantallas en horizontal, vertical) para no invadir la otra; si no hay eje libre, pop-in. `general.animation_ms` (0 las desactiva).
- **Animaciones de Windows**: `disable_windows_animations = true` apaga la animación de minimizar/maximizar (`SPI_SETANIMATION`) mientras hypr-win corre; no se guarda en el registro y se restaura al salir. Los fundidos de abrir/cerrar los hace el DWM por ventana (`DWMWA_TRANSITIONS_FORCEDISABLED`) o con el interruptor global de Windows (Configuración > Accesibilidad > Efectos visuales > Efectos de animación); no se tocan desde aquí por la restricción de no intervenir en DWM.
- **Barra de tareas de Windows**: con `hide_taskbar = true` se oculta (`ShowWindow(SW_HIDE)` sobre `Shell_TrayWnd` y `Shell_SecondaryTrayWnd`) y se vuelve a mostrar al salir. Esto toca ventanas del proceso `explorer.exe`; es una excepción deliberada a la restricción inicial. Un temporizador de 1 s la oculta de nuevo si Explorer la reaparece. Windows sigue reservando su hueco en el área de trabajo, así que el mosaico usa el monitor entero menos nuestra barra. Si hypr-win muere sin salir limpio: `hypr-win.exe --restore-taskbar`. Se pierden la bandeja del sistema, las notificaciones y el reloj nativos.
- **Start, búsqueda, widgets...**: `block_start_menu = true` traga la tecla Win sola (menú Inicio). Cualquier combinación se puede anular con la acción `none`, p. ej. `"Super+S, none"` (búsqueda), `Super+W` (widgets), `Super+A` (ajustes rápidos), `Super+N` (notificaciones). `Win+R` sigue funcionando para lanzar programas.
- **Estilo macOS**: la barra imita la barra de menús (translúcida, workspaces como puntos con una pastilla activa que se desliza, título a la izquierda, fecha y hora a la derecha) y el lanzador imita Spotlight (tarjeta con esquinas redondeadas y sombra, lupa, campo grande, cabecera "Aplicaciones", fichas de color con la inicial, selección en pastilla azul y `↵`). Todo se dibuja con GDI+ (`src/gfx.rs`) en ventanas propias con transparencia por píxel (`UpdateLayeredWindow`): no se usa DWM para nada. La paleta es la del sistema de Apple, en modo oscuro por defecto (preset claro en `config.toml`); la fuente por defecto es *Segoe UI Variable Text*, lo más cercano a San Francisco que trae Windows 11 (San Francisco no se puede redistribuir). `bar.opacity` regula la translucidez. Vista previa sin tocar el escritorio: `cargo run --example preview -- <carpeta> [claro]` genera PNG de la barra y el lanzador.
- **Lanzador (`Super+K`, también `Super+D`)**: la paleta estilo Spotlight de macOS (`src/palette.rs`). Empieza como una **barra de búsqueda sola** (con lupa y texto grande); al escribir aparecen los resultados con **iconos reales** de las apps, la sección "Mejor resultado" y luego "Aplicaciones", y la etiqueta "Aplicación" a la derecha de cada fila (la seleccionada muestra `↵`). `↓` o `Tab` con la barra vacía muestran todas las apps. Búsqueda difusa (exacto > prefijo > inicio de palabra > subcadena > letras en orden; `vsc` encuentra Visual Studio Code); `↑/↓/Tab` mueven la selección, `Enter` lanza, `Esc` cierra y devuelve el foco, `Ctrl+Backspace` borra, clic y rueda funcionan. El índice sale de `shell:AppsFolder` (apps de escritorio y de la Tienda). El foco por ratón se pausa mientras está abierto.
- **Menú (`Super+Espacio`)**, al estilo del menú de Omarchy: la misma paleta, por niveles (`Esc` o `Retroceso` con el campo vacío suben un nivel; escribir filtra). El layout pasa a `Super+Shift+Espacio`.
  - **Aplicaciones**: abre el lanzador.
  - **Instalar**: con la barra vacía, una selección de programas habituales por categorías, cada uno con su **icono** (el real si ya está instalado y, si no, el favicon del fabricante, que se descarga en segundo plano y se guarda en `%APPDATA%\hypr-win\pkgicons`; si un sitio no tiene favicon, como VLC, queda la ficha de color). **Al escribir (2 letras o más) busca solo**, en segundo plano y tras una pausa de 350 ms para no buscar en cada tecla: primero la sección **winget** (`winget search`) y debajo la de **Chocolatey** (`choco search`, reordenado por relevancia y con el paquete de nombre exacto primero), hasta 8 resultados de cada una. Mientras llegan se ve "Buscando en winget…". Los resultados quedan en caché, así que volver a una consulta es instantáneo. Elegir un resultado lo instala con su id exacto: winget en una consola que se queda abierta para ver el progreso; Chocolatey en una consola de administrador (aviso de UAC). Cada gestor solo aparece si está instalado; si no hay ninguno, el menú lo avisa. No se instala Chocolatey automáticamente: su instalación ejecuta un script descargado de internet, y eso lo decides tú (https://chocolatey.org/install). Los textos escritos se sanean antes de llegar a la línea de comandos; la tabla de winget se interpreta por posición (sus títulos de columna cambian con el idioma).
  - **Aplicaciones web**: crea una app a partir de una página (YouTube, Spotify, Classroom, WhatsApp, Gmail, ChatGPT... sugeridas, o "Nueva aplicación web…" con nombre y dirección). Es un acceso directo en el menú Inicio (`Hypr Web Apps`) que abre `msedge --app=URL` (o Chrome/Brave) con el **favicon como icono** (Google, con DuckDuckGo de respaldo), así que aparece en el lanzador, se puede fijar en el dock y se tilea como cualquier ventana. También se pueden abrir y eliminar desde ahí.
  - **Atajos**: lista todos los atajos con lo que hacen; al elegir uno, **pulsa la nueva combinación** (el hook de teclado captura también las combinaciones con Super sin que Windows las ejecute). Avisa si la combinación ya está en uso o no incluye Super/Ctrl/Alt, y el cambio se **guarda en `config.toml` y se aplica al instante**. También abre `config.toml` para editar a mano.
  - **Panel de control**: abre el panel (Wi-Fi, Bluetooth, sonido).
  - **Sistema**: bloquear, suspender, cerrar sesión, reiniciar, apagar (con confirmación), actualizar aplicaciones (`winget upgrade --all`) y salir de hypr-win.
- **Estado del sistema en la barra y panel de control** (`Super+I` o clic en los iconos): Bluetooth, Wi-Fi, volumen y batería a la derecha del reloj, y un panel al estilo del Centro de control de macOS. Wi-Fi con la API WLAN nativa (`wlanapi`): encender/apagar la radio, lista de redes con señal y candado, conectar (las guardadas sin contraseña; las nuevas WPA2/WPA3-Personal y abiertas piden la contraseña en el propio panel y crean el perfil), desconectar. **Desde Windows 11 24H2 Windows devuelve "acceso denegado" a las apps que piden nombres de red o señal sin permiso de ubicación**: en ese caso el panel sigue mostrando que el adaptador está conectado y ofrece un botón que abre *Configuración > Privacidad > Ubicación* (activa los servicios de ubicación y "Permitir que las aplicaciones de escritorio accedan a tu ubicación"). Bluetooth: interruptor de la radio (`Windows.Devices.Radios`) y dispositivos conectados (API Win32 de Bluetooth). Sonido con Core Audio: slider de volumen, silencio y nombre de la salida (cambiar de dispositivo de salida solo se puede con interfaces COM no documentadas, así que no está). Batería con `GetSystemPowerStatus`. Red, Sonido y Bluetooth abren su página de Configuración. Se sondea cada 3 s en el hilo de la interfaz (Bluetooth cada ~9 s y al abrir el panel).
- **Dock estilo Apple** (`[dock]` en `config.toml`): pastilla translúcida centrada abajo, una en cada monitor, con los **iconos reales** de las apps (`IShellItemImageFactory`), magnificación con curva de campana al pasar el ratón, etiqueta con el nombre, punto bajo las apps abiertas, rebote al lanzar y separador antes de las abiertas que no están fijadas. Tamaño por defecto compacto (`icon_size = 36`, `magnification = 1.5`). Con `autohide = true` (por defecto) se desliza hacia abajo en cuanto hay ventanas en ese monitor y reaparece al dejar el ratón unos 400 ms en el borde inferior; se vuelve a ocultar 600 ms después de apartar el ratón. Con el auto-ocultado activo no reserva espacio (aparece por encima de las ventanas); con `autohide = false` reserva su franja inferior. Clic: enfoca la app (la siguiente ventana si ya tiene el foco; cambia de workspace si hace falta) o la lanza. Las apps fijadas se indican por nombre (`pinned`) y se resuelven con la misma búsqueda difusa del lanzador; las ventanas se asocian a su icono por AppUserModelID o por el nombre del ejecutable.
- **Atajos con Super**: los atiende un hook de teclado de bajo nivel (`WH_KEYBOARD_LL`, se ejecuta en nuestro hilo; no se inyecta en otros procesos), porque el shell se queda antes con `Win+Número` y abriría las apps ancladas. Al soltar Win se envía una tecla ficticia para que no salte el menú Inicio. Los atajos sin Super usan `RegisterHotKey`.
- **Barra**: appbar superior por monitor (`ABM_NEW/QUERYPOS/SETPOS`); el área de trabajo del sistema la excluye, así que las ventanas maximizadas no la tapan.
- **Bordes invisibles**: se compensan con `DWMWA_EXTENDED_FRAME_BOUNDS` (solo lectura) para que los gaps sean exactos.

## Limitaciones conocidas

- **Menú**: instalar usa `winget` o Chocolatey (si no hay ninguno, el menú lo avisa). Las apps web necesitan Edge, Chrome o Brave. Al capturar un atajo, las teclas sin nombre en `config.toml` (p. ej. signos de puntuación) no se pueden usar. Cambiar de dispositivo de salida de audio sigue sin estar.

- **Dock**: hay uno en cada monitor gestionado, con el mismo contenido. Si dejas visible la barra de tareas de Windows (`hide_taskbar = false`) se superpondrá con ella. Los iconos de apps que no se pueden resolver (procesos sin ruta accesible) se muestran como ficha de color con la inicial.
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

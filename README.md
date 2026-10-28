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
- **Workspaces**: al cambiar, las ventanas del actual se mueven a (-32000, -32000) y se recoloca el nuevo. Cada workspace conserva su orden, layout y foco.
- **Barra**: appbar superior (`ABM_NEW/QUERYPOS/SETPOS`); el área de trabajo del sistema la excluye, así que las ventanas maximizadas no la tapan.
- **Bordes invisibles**: se compensan con `DWMWA_EXTENDED_FRAME_BOUNDS` (solo lectura) para que los gaps sean exactos.

## Limitaciones conocidas

- **Atajos con Win**: `RegisterHotKey` falla para combinaciones que Windows se reserva (p. ej. algunas `Win+Número` o `Win+Q` según versión/configuración). Al arrancar se imprime un aviso por cada atajo no registrado. Alternativa documentada si ocurre: un hook de teclado de bajo nivel (`SetWindowsHookEx(WH_KEYBOARD_LL)`), no implementado; mientras tanto, cambia el modificador en `config.toml` (p. ej. `Alt`).
- Las ventanas ocultas por workspace siguen apareciendo en la barra de tareas nativa (que no se oculta).
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

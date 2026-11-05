"""Genera los packs de iconos que incluye hypr-win a partir de los paquetes de npm originales.

    npm pack lucide-static @phosphor-icons/web @tabler/icons-webfont @fluentui/react-icons
    (descomprimir cada .tgz en una carpeta con su nombre: lucide/, phosphor/, tabler/, fluent/)
    python tools/build_icon_packs.py <carpeta con lucide/package, phosphor/package, tabler/package, fluent/package>

Para cada pack se recorta la fuente a los iconos que usa la interfaz (unos pocos KB en vez de cientos o miles) y se
escribe `src/iconset_data.rs` con la equivalencia entre el glifo de *Segoe Fluent Icons* que usa el código y el glifo del
pack, más los iconos de estado de la barra y el panel. Requiere `pip install fonttools`.
"""
import json
import re
import shutil
import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "icons"

# glifo de Segoe -> (significado, Phosphor, Lucide, Fluent, Tabler)
ICONS = {
    0xE121: ("reloj", "clock", "clock", "Clock", "clock"),
    0xE701: ("wifi", "wifi-high", "wifi", "Wifi1", "wifi"),
    0xE706: ("brillo", "sun", "sun", "BrightnessHigh", "sun"),
    0xE708: ("suspender", "moon", "moon", "WeatherMoon", "moon"),
    0xE70B: ("nota", "note", "sticky-note", "Note", "note"),
    0xE70F: ("editar", "pencil-simple", "pencil", "Edit", "pencil"),
    0xE710: ("añadir", "plus", "plus", "Add", "plus"),
    0xE711: ("cerrar", "x", "x", "Dismiss", "x"),
    0xE713: ("ajustes", "gear", "settings", "Settings", "settings"),
    0xE71D: ("aplicaciones", "squares-four", "layout-grid", "Apps", "layout-grid"),
    0xE721: ("buscar", "magnifying-glass", "search", "Search", "search"),
    0xE72A: ("siguiente", "arrow-right", "arrow-right", "ArrowRight", "arrow-right"),
    0xE72E: ("bloquear", "lock", "lock", "LockClosed", "lock"),
    0xE73E: ("correcto", "check", "check", "Checkmark", "check"),
    0xE740: ("ajuste de imagen", "arrows-out", "maximize", "FullScreenMaximize", "arrows-maximize"),
    0xE74D: ("eliminar", "trash", "trash-2", "Delete", "trash"),
    0xE74E: ("guardar", "floppy-disk", "save", "Save", "device-floppy"),
    0xE753: ("nube", "cloud", "cloud", "Cloud", "cloud"),
    0xE765: ("teclado", "keyboard", "keyboard", "Keyboard", "keyboard"),
    0xE774: ("web", "globe", "globe", "Globe", "world"),
    0xE777: ("reiniciar", "arrow-clockwise", "rotate-cw", "ArrowClockwise", "rotate-clockwise"),
    0xE77B: ("usuario", "user", "user", "Person", "user"),
    0xE783: ("error", "warning-circle", "circle-alert", "ErrorCircle", "alert-circle"),
    0xE787: ("calendario", "calendar-blank", "calendar", "Calendar", "calendar"),
    0xE790: ("paleta", "palette", "palette", "Color", "palette"),
    0xE7A7: ("deshacer", "arrow-counter-clockwise", "undo-2", "ArrowUndo", "arrow-back-up"),
    0xE7B3: ("ver", "eye", "eye", "Eye", "eye"),
    0xE7E8: ("apagar", "power", "power", "Power", "power"),
    0xE7F4: ("monitor", "monitor", "monitor", "Desktop", "device-desktop"),
    0xE80F: ("widgets", "app-window", "app-window", "AppFolder", "app-window"),
    0xE81D: ("ubicación", "map-pin", "map-pin", "Location", "map-pin"),
    0xE895: ("actualizar", "arrows-clockwise", "refresh-cw", "ArrowSync", "refresh"),
    0xE896: ("descargar", "download-simple", "download", "ArrowDownload", "download"),
    0xE8A5: ("documento", "file-text", "file-text", "Document", "file-text"),
    0xE8B1: ("al azar", "shuffle", "shuffle", "ArrowShuffle", "arrows-shuffle"),
    0xE8B7: ("carpeta", "folder-open", "folder-open", "FolderOpen", "folder-open"),
    0xE8D2: ("fuente", "text-aa", "type", "TextFont", "typography"),
    0xE8D6: ("música", "music-notes", "music", "MusicNote2", "music"),
    0xE916: ("historial", "clock-counter-clockwise", "history", "History", "history"),
    0xE91B: ("imagen", "image", "image", "Image", "photo"),
    0xE946: ("información", "info", "info", "Info", "info-circle"),
    0xE9D9: ("sistema", "cpu", "cpu", "DeveloperBoard", "cpu"),
}

# (en Fluent `Wifi1` es la señal completa y `Wifi4` la más débil)
# iconos de estado de la barra y del panel: nombre interno -> (Phosphor, Lucide, Fluent, Tabler)
STATUS = {
    "wifi-3": ("wifi-high", "wifi", "Wifi1", "wifi"),
    "wifi-2": ("wifi-medium", "wifi-high", "Wifi2", "wifi-2"),
    "wifi-1": ("wifi-low", "wifi-low", "Wifi3", "wifi-1"),
    "wifi-0": ("wifi-none", "wifi-zero", "Wifi4", "wifi-0"),
    "wifi-off": ("wifi-slash", "wifi-off", "WifiOff", "wifi-off"),
    "speaker-3": ("speaker-high", "volume-2", "Speaker2", "volume"),
    "speaker-1": ("speaker-low", "volume-1", "Speaker1", "volume-2"),
    "speaker-0": ("speaker-none", "volume", "Speaker0", "volume-3"),
    "speaker-off": ("speaker-x", "volume-x", "SpeakerOff", "volume-off"),
    "battery-100": ("battery-full", "battery-full", "Battery10", "battery-4"),
    "battery-75": ("battery-high", "battery-full", "Battery7", "battery-3"),
    "battery-50": ("battery-medium", "battery-medium", "Battery5", "battery-2"),
    "battery-25": ("battery-low", "battery-low", "Battery2", "battery-1"),
    "battery-0": ("battery-empty", "battery-warning", "Battery0", "battery-exclamation"),
    "battery-charging": ("battery-charging", "battery-charging", "BatteryCharge", "battery-charging"),
    "bluetooth": ("bluetooth", "bluetooth", "Bluetooth", "bluetooth"),
    "lock": ("lock-simple", "lock", "LockClosed", "lock"),
    "check": ("check", "check", "Checkmark", "check"),
}


def css_codes(css: Path, pattern: str) -> dict:
    text = css.read_text(encoding="utf-8")
    return {m.group(1): int(m.group(2), 16) for m in re.finditer(pattern, text)}


def phosphor_codes(css: Path, prefix: str) -> dict:
    return css_codes(css, rf"\.{re.escape(prefix)}\.ph-([a-z0-9-]+):before\s*{{\s*content:\s*\"\\([0-9a-f]+)\"")


def fluent_codes(json_path: Path, variant: str) -> dict:
    """`Wifi4` -> código de `Wifi424Regular` (el tamaño de 24 px)."""
    raw = json.loads(json_path.read_text(encoding="utf-8"))
    return {k[: -len("24" + variant)]: v for k, v in raw.items() if k.endswith("24" + variant)}


def build(src_dir: Path):
    ph, lu, tb, fl = (src_dir / n / "package" for n in ("phosphor", "lucide", "tabler", "fluent"))
    fonts = fl / "lib/utils/fonts"
    # clave, fuente original, tabla nombre->código, columna de nombres en ICONS (1 Phosphor, 2 Lucide, 3 Fluent, 4 Tabler)
    packs = [
        ("phosphor", ph / "src/regular/Phosphor.ttf", phosphor_codes(ph / "src/regular/style.css", "ph"), 1),
        ("phosphor-bold", ph / "src/bold/Phosphor-Bold.ttf", phosphor_codes(ph / "src/bold/style.css", "ph-bold"), 1),
        ("phosphor-fill", ph / "src/fill/Phosphor-Fill.ttf", phosphor_codes(ph / "src/fill/style.css", "ph-fill"), 1),
        ("lucide", lu / "font/lucide.ttf", json.loads((lu / "font/codepoints.json").read_text(encoding="utf-8")), 2),
        ("fluent", fonts / "FluentSystemIcons-Regular.ttf", fluent_codes(fonts / "FluentSystemIcons-Regular.json", "Regular"), 3),
        ("fluent-fill", fonts / "FluentSystemIcons-Filled.ttf", fluent_codes(fonts / "FluentSystemIcons-Filled.json", "Filled"), 3),
        ("tabler", tb / "dist/fonts/tabler-icons.ttf", css_codes(tb / "dist/tabler-icons.css", r"\.ti-([a-z0-9-]+):before\s*{\s*content:\s*\"\\([0-9a-f]+)\""), 4),
    ]
    OUT.mkdir(parents=True, exist_ok=True)
    shutil.copy(ph / "LICENSE", OUT / "LICENSE-phosphor.txt")
    shutil.copy(lu / "LICENSE", OUT / "LICENSE-lucide.txt")
    shutil.copy(tb / "LICENSE", OUT / "LICENSE-tabler.txt")
    # el paquete de Fluent no trae el archivo de licencia: es MIT (Microsoft Corporation), como declara su package.json
    (OUT / "LICENSE-fluent.txt").write_text(
        "Fluent UI System Icons\nCopyright (c) Microsoft Corporation\nLicensed under the MIT License\n"
        "https://github.com/microsoft/fluentui-system-icons/blob/main/LICENSE\n\n" + (ph / "LICENSE").read_text(encoding="utf-8").split("\n", 3)[3].lstrip(),
        encoding="utf-8",
    )
    rust = ["// Generado por tools/build_icon_packs.py: no editar a mano.", "// (glifo de Segoe Fluent Icons, glifo del pack)", ""]
    for key, ttf, codes, col in packs:
        pairs, wanted = [], []
        for seg, meta in sorted(ICONS.items()):
            name = meta[col]
            if name not in codes:
                sys.exit(f"{key}: no existe el icono «{name}»")
            pairs.append((seg, codes[name]))
            wanted.append(codes[name])
        status = []
        for key_name, meta in STATUS.items():
            name = meta[col - 1]
            if name not in codes:
                sys.exit(f"{key}: no existe el icono de estado «{name}»")
            status.append((key_name, codes[name]))
            wanted.append(codes[name])
        opts = subset.Options()
        opts.layout_features = []
        opts.name_IDs = [1, 2, 4, 6]
        opts.notdef_outline = True
        font = TTFont(str(ttf))
        sub = subset.Subsetter(opts)
        sub.populate(unicodes=wanted)
        sub.subset(font)
        target = OUT / f"{key}.ttf"
        font.save(str(target))
        family = TTFont(str(target))["name"].getDebugName(1)
        print(f"{key}: {target.stat().st_size} bytes, familia «{family}»")
        const = key.upper().replace("-", "_")
        rust.append(f"pub const {const}_FAMILY: &str = {json.dumps(family)};")
        rust.append(f"pub static {const}_MAP: &[(char, char)] = &[")
        for seg, cp in pairs:
            rust.append(f"    ('\\u{{{seg:X}}}', '\\u{{{cp:X}}}'),")
        rust.append("];")
        rust.append(f"pub static {const}_STATUS: &[(&str, char)] = &[")
        for name, cp in status:
            rust.append(f'    ("{name}", \'\\u{{{cp:X}}}\'),')
        rust.append("];")
        rust.append("")
    (ROOT / "src" / "iconset_data.rs").write_text("\n".join(rust), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    build(Path(sys.argv[1]))

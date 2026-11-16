"""Genera el pack de iconos «Bushidō» (assets/icons/bushido.ttf + src/iconset_bushido.rs).

Los iconos están dibujados desde cero en una rejilla de 24x24 con trazos rectos y círculos octogonales (estilo
angular, de pincel). Cada icono es una unión de segmentos con grosor, así que se escribe como contornos que se
solapan (todos con el mismo sentido; TrueType rellena con la regla del giro no nulo). Licencia: los dibujos son
de este proyecto (como el resto del código).

    python tools/build_bushido_icons.py [--preview hoja.png]      (requiere fonttools y, para la vista previa, pillow)
"""
import math
import sys
from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

ROOT = Path(__file__).resolve().parent.parent
FONT_OUT = ROOT / "assets" / "icons" / "bushido.ttf"
RUST_OUT = ROOT / "src" / "iconset_bushido.rs"
FAMILY = "Hypr Bushido"

UPM, ASCENT, DESCENT = 1024, 960, -64
BOX = 832          # lado del icono (como Phosphor: x 0..832, y 64..896)
GRID = 24.0
STROKE = 2.0       # grosor en la rejilla
ZOOM = 1.3         # los dibujos ocupan ~75 % de la rejilla; se amplían para igualar el tamaño visual de Phosphor

# ---------- primitivas (coordenadas de la rejilla, y hacia abajo) ----------


def circle(cx, cy, r, n=8):
    """Círculo octogonal (lados planos arriba, abajo y a los lados)."""
    off = math.pi / n
    return [(cx + r * math.cos(off + 2 * math.pi * i / n), cy + r * math.sin(off + 2 * math.pi * i / n)) for i in range(n)]


def arc(cx, cy, r, a0, a1, n=8):
    """Puntos de un arco de `a0` a `a1` grados (0° = derecha, crece hacia abajo)."""
    return [(cx + r * math.cos(math.radians(a0 + (a1 - a0) * i / n)), cy + r * math.sin(math.radians(a0 + (a1 - a0) * i / n))) for i in range(n + 1)]


def S(*pts, closed=False):
    """Trazo: lista de puntos (x, y, x, y...) o tuplas."""
    flat = list(pts)
    if flat and not isinstance(flat[0], tuple):
        flat = [(flat[i], flat[i + 1]) for i in range(0, len(flat), 2)]
    return ("stroke", flat, closed)


def Fill(*pts):
    flat = list(pts)
    if flat and not isinstance(flat[0], tuple):
        flat = [(flat[i], flat[i + 1]) for i in range(0, len(flat), 2)]
    return ("fill", flat)


def Rect(x, y, w, h):
    return S((x, y), (x + w, y), (x + w, y + h), (x, y + h), closed=True)


def Ring(cx, cy, r):
    return ("stroke", circle(cx, cy, r), True)


def Disc(cx, cy, r):
    return ("fill", circle(cx, cy, r))


def chevrons(levels):
    """Wi-Fi: de 0 a 3 arcos angulares (de dentro hacia fuera) y el punto."""
    arcs = [S(9, 16.8, 12, 15, 15, 16.8), S(6, 12.8, 12, 9.4, 18, 12.8), S(3, 8.8, 12, 3.6, 21, 8.8)]
    return arcs[:levels] + [Disc(12, 20, 1.3)]


def speaker(waves):
    out = [S((4, 10), (8, 10), (13, 6), (13, 18), (8, 14), (4, 14), closed=True)]
    if waves >= 1:
        out.append(S(16, 9.5, 17.5, 12, 16, 14.5))
    if waves >= 2:
        out.append(S(18.8, 6, 21.5, 12, 18.8, 18))
    return out


def battery(level, bolt=False, bang=False):
    out = [Rect(2.5, 6, 17, 12), S(21.8, 10, 21.8, 14)]
    if level:
        out.append(Fill((5, 8.6), (5 + 12.6 * level, 8.6), (5 + 12.6 * level, 15.4), (5, 15.4)))
    if bolt:
        out.append(Fill((13.4, 6.0), (7.2, 13.0), (11.2, 13.0), (9.8, 18.0), (16.4, 10.2), (12.4, 10.2)))
    if bang:
        out.append(S(11, 9.2, 11, 12.8))
        out.append(Disc(11, 15.2, 0.7))
    return out


ICONS = {
    "clock": [Ring(12, 12, 9), S(12, 7, 12, 12, 16, 14)],
    "wifi": chevrons(3),
    "wifi-2": chevrons(2),
    "wifi-1": chevrons(1),
    "wifi-0": chevrons(0),
    "wifi-off": chevrons(3) + [S(4, 4, 20, 20)],
    "sun": [Ring(12, 12, 4.2)] + [S(12 + 7 * math.cos(math.radians(a)), 12 + 7 * math.sin(math.radians(a)), 12 + 10 * math.cos(math.radians(a)), 12 + 10 * math.sin(math.radians(a))) for a in range(0, 360, 45)],
    "moon": [S((14, 3), (9, 5), (5, 10), (5, 15), (9, 20), (15, 21), (20, 18), (21, 17), (16, 17), (12, 13), (11, 8), closed=True)],
    "note": [Rect(5, 3, 14, 18), S(8, 9, 16, 9), S(8, 13, 16, 13), S(8, 17, 12, 17)],
    "edit": [S((4, 20), (5.5, 14.5), (16, 4), (20, 8), (9.5, 18.5), closed=True), S(13.5, 6.5, 17.5, 10.5)],
    "plus": [S(12, 5, 12, 19), S(5, 12, 19, 12)],
    "x": [S(6, 6, 18, 18), S(18, 6, 6, 18)],
    "gear": [Ring(12, 12, 7), Ring(12, 12, 2.6)] + [S(12 + 7 * math.cos(math.radians(a)), 12 + 7 * math.sin(math.radians(a)), 12 + 10 * math.cos(math.radians(a)), 12 + 10 * math.sin(math.radians(a))) for a in range(22, 382, 45)],
    "apps": [Rect(4, 4, 6, 6), Rect(14, 4, 6, 6), Rect(4, 14, 6, 6), Rect(14, 14, 6, 6)],
    "search": [Ring(10, 10, 6.5), S(15, 15, 20, 20)],
    "next": [S(4, 12, 19, 12), S(13, 6, 19, 12, 13, 18)],
    "lock": [Rect(5, 11, 14, 10), S(8, 11, 8, 7.5, 10, 4.5, 14, 4.5, 16, 7.5, 16, 11), Disc(12, 16, 1.3)],
    "check": [S(4, 12.5, 9.5, 18, 20, 6.5)],
    "fit": [S(4, 9, 4, 4, 9, 4), S(15, 4, 20, 4, 20, 9), S(20, 15, 20, 20, 15, 20), S(9, 20, 4, 20, 4, 15)],
    "trash": [S(4, 7, 20, 7), S(9, 7, 9, 4, 15, 4, 15, 7), S(6, 7, 7, 20, 17, 20, 18, 7), S(10, 11, 10, 16), S(14, 11, 14, 16)],
    "save": [Rect(4, 4, 16, 16), S(7, 4, 7, 9, 16, 9, 16, 4), S(8, 20, 8, 14, 16, 14, 16, 20)],
    "cloud": [S((7, 18), (4, 15), (4, 12), (7, 10), (8, 7), (12, 5), (16, 7), (17, 10), (20, 11), (21, 14), (19, 18), closed=True)],
    "keyboard": [Rect(3, 6, 18, 12), S(6, 10, 7, 10), S(9.5, 10, 10.5, 10), S(13, 10, 14, 10), S(16.5, 10, 17.5, 10), S(7, 14, 17, 14)],
    "globe": [Ring(12, 12, 9), S(3, 12, 21, 12), S((12, 3), (16, 7), (16, 17), (12, 21), (8, 17), (8, 7), closed=True)],
    "refresh": [S(*arc(12, 12, 8, 30, 330, 9)[0:], closed=False), S(18.5, 4, 19, 9.5, 13.5, 9.5)],
    "user": [Ring(12, 8, 3.8), S(5, 21, 5, 18, 8, 14.5, 16, 14.5, 19, 18, 19, 21)],
    "alert": [Ring(12, 12, 9), S(12, 7, 12, 13), Disc(12, 16.4, 1.2)],
    "calendar": [Rect(4, 5, 16, 15), S(4, 10, 20, 10), S(8, 3, 8, 7), S(16, 3, 16, 7)],
    "palette": [S((12, 3), (18, 5), (21, 10), (20, 15), (16, 16), (14, 17), (14, 20), (11, 21), (6, 19), (3, 14), (4, 8), (8, 4), closed=True), Disc(7.8, 11.5, 1.2), Disc(10, 7.6, 1.2), Disc(14.4, 7.6, 1.2), Disc(17, 11, 1.2)],
    "undo": [S(*arc(12, 12, 8, 150, -150, 9)), S(5.5, 4, 5, 9.5, 10.5, 9.5)],
    "eye": [S((2, 12), (7, 6), (12, 5), (17, 6), (22, 12), (17, 18), (12, 19), (7, 18), closed=True), Ring(12, 12, 3)],
    "power": [S(*arc(12, 13, 8, -55, 235, 8)), S(12, 3, 12, 12)],
    "monitor": [Rect(3, 4, 18, 12), S(8, 20, 16, 20), S(12, 16, 12, 20)],
    "widgets": [Rect(3, 4, 18, 16), S(3, 9, 21, 9), S(6, 6.5, 7, 6.5)],
    "pin": [S((12, 21), (6, 13), (6, 8), (9, 4), (15, 4), (18, 8), (18, 13), closed=True), Ring(12, 9, 2.6)],
    "download": [S(12, 4, 12, 15), S(6, 10, 12, 16, 18, 10), S(4, 20, 20, 20)],
    "doc": [S((6, 3), (14, 3), (19, 8), (19, 21), (6, 21), closed=True), S(14, 3, 14, 8, 19, 8), S(9, 13, 16, 13), S(9, 17, 16, 17)],
    "shuffle": [S(3, 7, 8, 7, 15, 17, 20, 17), S(17, 14, 20, 17, 17, 20), S(3, 17, 8, 17, 10, 14), S(14, 10, 15, 7, 20, 7), S(17, 4, 20, 7, 17, 10)],
    "folder": [S((3, 6), (9, 6), (11, 8), (21, 8), (21, 19), (3, 19), closed=True)],
    "font": [S(4, 20, 9, 6, 14, 20), S(6, 15, 12, 15), S(17, 20, 17, 12, 21, 12)],
    "music": [S(9, 17, 9, 5, 19, 3, 19, 15), Disc(7, 18, 2.6), Disc(17, 16, 2.6)],
    "history": [Ring(13, 12, 8), S(13, 7.5, 13, 12, 16.5, 14), S(2, 8, 4, 12, 7.5, 9.5)],
    "image": [Rect(3, 4, 18, 16), Disc(8, 9, 1.8), S(3, 18, 9, 12, 13, 16, 16, 13, 21, 18)],
    "info": [Ring(12, 12, 9), Disc(12, 7.6, 1.2), S(12, 11, 12, 17)],
    "cpu": [Rect(6, 6, 12, 12), Rect(9.5, 9.5, 5, 5), S(9, 3, 9, 6), S(15, 3, 15, 6), S(9, 18, 9, 21), S(15, 18, 15, 21), S(3, 9, 6, 9), S(3, 15, 6, 15), S(18, 9, 21, 9), S(18, 15, 21, 15)],
    "speaker-3": speaker(2),
    "speaker-1": speaker(1),
    "speaker-0": speaker(0),
    "speaker-off": speaker(0) + [S(16, 9.5, 21, 14.5), S(21, 9.5, 16, 14.5)],
    "battery-100": battery(1.0),
    "battery-75": battery(0.75),
    "battery-50": battery(0.5),
    "battery-25": battery(0.25),
    "battery-0": battery(0, bang=True),
    "battery-charging": battery(0, bolt=True),
    "bluetooth": [S(7, 8, 17, 16, 12, 20, 12, 4, 17, 8, 7, 16)],
}

# glifo de Segoe Fluent Icons (el que usa el código) -> icono (mismo orden y significado que tools/build_icon_packs.py)
SEGOE = {
    0xE121: "clock", 0xE701: "wifi", 0xE706: "sun", 0xE708: "moon", 0xE70B: "note", 0xE70F: "edit", 0xE710: "plus",
    0xE711: "x", 0xE713: "gear", 0xE71D: "apps", 0xE721: "search", 0xE72A: "next", 0xE72E: "lock", 0xE73E: "check",
    0xE740: "fit", 0xE74D: "trash", 0xE74E: "save", 0xE753: "cloud", 0xE765: "keyboard", 0xE774: "globe",
    0xE777: "refresh", 0xE77B: "user", 0xE783: "alert", 0xE787: "calendar", 0xE790: "palette", 0xE7A7: "undo",
    0xE7B3: "eye", 0xE7E8: "power", 0xE7F4: "monitor", 0xE80F: "widgets", 0xE81D: "pin", 0xE895: "refresh",
    0xE896: "download", 0xE8A5: "doc", 0xE8B1: "shuffle", 0xE8B7: "folder", 0xE8D2: "font", 0xE8D6: "music",
    0xE916: "history", 0xE91B: "image", 0xE946: "info", 0xE9D9: "cpu",
}

# iconos de estado de la barra y del panel: nombre interno -> icono
STATUS = {
    "wifi-3": "wifi", "wifi-2": "wifi-2", "wifi-1": "wifi-1", "wifi-0": "wifi-0", "wifi-off": "wifi-off",
    "speaker-3": "speaker-3", "speaker-1": "speaker-1", "speaker-0": "speaker-0", "speaker-off": "speaker-off",
    "battery-100": "battery-100", "battery-75": "battery-75", "battery-50": "battery-50", "battery-25": "battery-25",
    "battery-0": "battery-0", "battery-charging": "battery-charging", "bluetooth": "bluetooth", "lock": "lock",
    "check": "check",
}

# ---------- geometría -> contornos ----------


def to_font(pt):
    x, y = pt[0] * BOX / GRID, 64 + BOX - pt[1] * BOX / GRID
    cx, cy = BOX / 2, 64 + BOX / 2
    return (cx + (x - cx) * ZOOM, cy + (y - cy) * ZOOM)


def clockwise(poly):
    """Fuerza el sentido horario (el exterior en TrueType, con y hacia arriba)."""
    area = sum(poly[i][0] * poly[(i + 1) % len(poly)][1] - poly[(i + 1) % len(poly)][0] * poly[i][1] for i in range(len(poly)))
    return poly if area < 0 else poly[::-1]


def contours(prims):
    half = STROKE / 2
    out = []
    for p in prims:
        if p[0] == "fill":
            out.append(p[1])
            continue
        pts, closed = p[1], p[2]
        n = len(pts)
        segs = [(pts[i], pts[(i + 1) % n]) for i in range(n if closed else n - 1)]
        for k, (a, b) in enumerate(segs):
            dx, dy = b[0] - a[0], b[1] - a[1]
            length = math.hypot(dx, dy)
            if length < 1e-9:
                continue
            ux, uy = dx / length, dy / length
            # extremos abiertos: se alargan un poco para que el trazo no quede "mocho"
            ext0 = 0.0 if closed or k > 0 else half * 0.35
            ext1 = 0.0 if closed or k < len(segs) - 1 else half * 0.35
            ax, ay, bx, by = a[0] - ux * ext0, a[1] - uy * ext0, b[0] + ux * ext1, b[1] + uy * ext1
            nx, ny = -uy * half, ux * half
            out.append([(ax + nx, ay + ny), (bx + nx, by + ny), (bx - nx, by - ny), (ax - nx, ay - ny)])
        # uniones octogonales en los vértices interiores
        joints = range(n) if closed else range(1, n - 1)
        for i in joints:
            out.append(circle(pts[i][0], pts[i][1], half * 1.02, 8))
    return out


def build_glyphs():
    order = [".notdef"] + list(ICONS)
    glyphs = {}
    pen = TTGlyphPen(None)
    glyphs[".notdef"] = pen.glyph()
    for name, prims in ICONS.items():
        pen = TTGlyphPen(None)
        for poly in contours(prims):
            pts = clockwise([to_font(p) for p in poly])
            pen.moveTo((round(pts[0][0]), round(pts[0][1])))
            for q in pts[1:]:
                pen.lineTo((round(q[0]), round(q[1])))
            pen.closePath()
        glyphs[name] = pen.glyph()
    return order, glyphs


def main():
    order, glyphs = build_glyphs()
    cp_of = {name: 0xF100 + i for i, name in enumerate(ICONS)}
    fb = FontBuilder(UPM, isTTF=True)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({cp: name for name, cp in cp_of.items()})
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics({n: (UPM, 0) for n in order})
    fb.setupHorizontalHeader(ascent=ASCENT, descent=DESCENT)
    fb.setupNameTable({"familyName": FAMILY, "styleName": "Regular"})
    fb.setupOS2(sTypoAscender=ASCENT, sTypoDescender=DESCENT, sTypoLineGap=64, usWinAscent=ASCENT, usWinDescent=-DESCENT, fsType=0)
    fb.setupPost()
    FONT_OUT.parent.mkdir(parents=True, exist_ok=True)
    fb.save(str(FONT_OUT))

    rust = ["// Generado por tools/build_bushido_icons.py: no editar a mano.",
            "// (glifo de Segoe Fluent Icons, glifo del pack Bushidō)", "",
            f'pub const BUSHIDO_FAMILY: &str = "{FAMILY}";', "pub static BUSHIDO_MAP: &[(char, char)] = &["]
    for seg, name in sorted(SEGOE.items()):
        rust.append(f"    ('\\u{{{seg:X}}}', '\\u{{{cp_of[name]:X}}}'),")
    rust += ["];", "pub static BUSHIDO_STATUS: &[(&str, char)] = &["]
    for key, name in STATUS.items():
        rust.append(f'    ("{key}", \'\\u{{{cp_of[name]:X}}}\'),')
    rust.append("];")
    RUST_OUT.write_text("\n".join(rust) + "\n", encoding="utf-8", newline="\n")
    print(f"{FONT_OUT.name}: {FONT_OUT.stat().st_size} bytes, {len(ICONS)} iconos")

    if "--preview" in sys.argv:
        from PIL import Image, ImageDraw, ImageFont
        path = sys.argv[sys.argv.index("--preview") + 1]
        cell, cols = 96, 10
        rows = (len(ICONS) + cols - 1) // cols
        img = Image.new("RGB", (cols * cell, rows * (cell + 14)), (14, 11, 13))
        d = ImageDraw.Draw(img)
        font = ImageFont.truetype(str(FONT_OUT), 64)
        for i, (name, cp) in enumerate(cp_of.items()):
            x, y = (i % cols) * cell, (i // cols) * (cell + 14)
            d.text((x + 16, y + 6), chr(cp), font=font, fill=(236, 228, 214))
            d.text((x + 4, y + cell), name[:14], fill=(138, 127, 118))
        img.save(path)


if __name__ == "__main__":
    main()

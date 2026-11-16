"""Convierte assets/samurai/idle_*.png en src/samurai_data.rs (píxeles ARGB, 4 fotogramas).

    python tools/build_samurai_sprite.py        (requiere pillow)
"""
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
frames = [Image.open(ROOT / "assets" / "samurai" / f"idle_{i}.png").convert("RGBA") for i in range(4)]
w, h = frames[0].size
assert all(f.size == (w, h) for f in frames)
out = ["//! Generado por tools/build_samurai_sprite.py (sprite CC0 de DezrasDragons; ver assets/samurai/CREDITS.md).", "",
       f"pub const W: usize = {w};", f"pub const H: usize = {h};", "",
       "/// Fotogramas de reposo, ARGB (0 = transparente).", f"pub static IDLE: [[u32; W * H]; {len(frames)}] = ["]
for f in frames:
    px = [(a << 24) | (r << 16) | (g << 8) | b if a else 0 for r, g, b, a in f.getdata()]
    out.append("    [")
    for i in range(0, len(px), 12):
        out.append("        " + ", ".join(f"0x{v:08X}" for v in px[i:i + 12]) + ",")
    out.append("    ],")
out.append("];")
(ROOT / "src" / "samurai_data.rs").write_text("\n".join(out) + "\n", encoding="utf-8")
print("ok", w, h)

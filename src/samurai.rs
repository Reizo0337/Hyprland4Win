//! Escena animada «Samurái»: noche con luna, estrellas que titilan, tres cordilleras en niebla, un torii,
//! un acantilado en primer plano con el samurái pixel art (CC0, ver `assets/samurai/CREDITS.md`) en reposo y
//! pétalos de cerezo cayendo. Función pura: dado el instante `t` (segundos) devuelve los píxeles ARGB.
//! Se calcula a resolución reducida y se amplía sin suavizar, lo que da el aspecto pixel art.

use crate::samurai_data as sprite;

type Rgb = [f32; 3];

const TAU: f32 = std::f32::consts::TAU;

fn unpack(c: u32) -> Rgb {
    [(c & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, ((c >> 16) & 0xff) as f32 / 255.0]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn pack(c: Rgb) -> u32 {
    let ch = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u32;
    0xFF00_0000 | ch(c[0]) << 16 | ch(c[1]) << 8 | ch(c[2])
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Hash determinista -> [0, 1).
fn hash(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA6B) ^ 0x27D4_EB2F;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65536.0
}

/// Cordilleras: (altura base, amplitud, frecuencias, fase, cuánto se funde con la niebla del horizonte).
const RIDGES: [(f32, f32, f32, f32, f32, f32); 3] =
    [(0.60, 0.07, 3.1, 7.3, 0.4, 0.62), (0.69, 0.08, 2.3, 5.1, 1.9, 0.38), (0.77, 0.06, 1.7, 4.3, 3.3, 0.18)];

fn ridge_y(layer: usize, x: usize, w: usize, h: usize) -> f32 {
    let (base, amp, f1, f2, ph, _) = RIDGES[layer];
    let u = x as f32 / w as f32 * TAU;
    (base + amp * (0.62 * (f1 * u + ph).sin() + 0.38 * (f2 * u + ph * 2.0).sin())) * h as f32
}

/// Borde superior del acantilado del primer plano.
fn cliff_y(x: f32, w: usize, h: usize) -> f32 {
    let u = x / w as f32;
    (0.865 + 0.03 * (u * 5.0 + 1.0).sin() + 0.02 * (u * 13.0).sin() - 0.05 * (1.0 - u * 1.6).clamp(0.0, 1.0)) * h as f32
}

/// Tamaño del lienzo reducido para un monitor de `w` x `h` píxeles.
pub fn scene_size(w: i32, h: i32) -> (i32, i32) {
    ((w / 4).max(64), (h / 4).max(36))
}

/// Píxeles ARGB opacos (`w` x `h`) en el instante `t`. `bg` y `accent` son COLORREF del tema.
pub fn render(w: usize, h: usize, t: f32, bg: u32, accent: u32) -> Vec<u32> {
    let (bg, ac) = (unpack(bg), unpack(accent));
    let (fw, fh) = (w as f32, h as f32);
    let ink = mix(bg, [0.0; 3], 0.55);
    let horizon = mix(bg, ac, 0.42);
    let sky_top = mix(bg, [0.0; 3], 0.25);
    let moon_col = [0.94, 0.88, 0.72];
    let (mx, my, mr) = (fw * 0.76, fh * 0.27, fh * 0.115);
    let ridges: Vec<Vec<f32>> = (0..3).map(|l| (0..w).map(|x| ridge_y(l, x, w, h)).collect()).collect();
    let cliff: Vec<f32> = (0..w).map(|x| cliff_y(x as f32, w, h)).collect();
    // torii: sobre la segunda cordillera
    let tcx = (fw * 0.58) as usize;
    let t_bottom = ridges[1][tcx.min(w - 1)] + fh * 0.02;
    let t_top = t_bottom - fh * 0.25;
    let t_col = mix(mix(ac, ink, 0.45), horizon, 0.18);
    let (pil, half) = (fw * 0.0075, fw * 0.045);

    let pulse = 1.0 + 0.06 * (t * 0.6).sin();
    let (halo_r, halo_col) = (mr * 5.0 * pulse, mix(horizon, moon_col, 0.6));
    let halo2 = halo_r * halo_r;
    let fog_col = mix(horizon, moon_col, 0.25);

    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        let yf = y as f32;
        let v = yf / fh;
        let band = (-((v - 0.69) / 0.07).powi(2)).exp();
        let sky = mix(sky_top, horizon, smooth(0.15, 0.78, v));
        for x in 0..w {
            let fx = x as f32;
            // cielo + resplandor de la luna (la raíz solo dentro del halo)
            let mut c = sky;
            let d2 = (fx - mx).powi(2) + (yf - my).powi(2);
            if d2 < halo2 {
                let d = d2.sqrt();
                c = mix(c, halo_col, (1.0 - (d / halo_r).min(1.0)).powi(2) * 0.38);
                if d < mr {
                    // disco con "mares" suaves y borde nítido
                    let crater = 0.06 * hash((x / 3) as u32, (y / 3) as u32);
                    let k = smooth(mr, mr - 1.5, d);
                    c = mix(c, mix(moon_col, [0.8, 0.72, 0.55], crater * 4.0), k);
                }
            }
            // cordilleras (de lejos a cerca) con el torii entre la 2ª y la 3ª
            for l in 0..3 {
                if yf >= ridges[l][x] {
                    let fog = RIDGES[l].5;
                    c = mix(ink, mix(horizon, bg, 0.2), fog);
                    // un poco de luz en la cresta
                    if yf - ridges[l][x] < 1.2 {
                        c = mix(c, mix(horizon, moon_col, 0.4), 0.35 * fog);
                    }
                }
                if l == 1 && in_torii(fx, yf, tcx as f32, t_top, t_bottom, pil, half, fh) {
                    c = t_col;
                }
            }
            // niebla que se desplaza entre las cordilleras (solo en su banda de filas)
            if band > 0.02 {
                let drift = 0.5 + 0.5 * (fx * 0.035 + yf * 0.06 - t * 0.45).sin() * (fx * 0.011 - t * 0.17).cos();
                c = mix(c, fog_col, band * drift * 0.22);
            }
            // acantilado del primer plano
            if yf >= cliff[x] {
                c = ink;
                if yf - cliff[x] < 1.4 {
                    c = mix(ink, ac, 0.28);
                }
            }
            out.push(pack(c));
        }
    }

    // estrellas: posiciones fijas (≈0,25 % del cielo) que titilan; solo donde aún se ve el cielo
    let n_stars = (w * h) as f32 * 0.62 * 0.0025;
    for i in 0..n_stars as u32 {
        let (x, y) = ((hash(i, 11) * fw) as usize, (hash(i, 12) * fh * 0.62) as usize);
        if x >= w || y >= h {
            continue;
        }
        let yf = y as f32;
        let in_moon = (x as f32 - mx).powi(2) + (yf - my).powi(2) < (mr * 1.1).powi(2);
        if in_moon || (0..3).any(|l| yf >= ridges[l][x]) {
            continue;
        }
        let tw = 0.55 + 0.45 * (t * 2.2 + hash(i, 13) * 400.0).sin();
        let o = y * w + x;
        out[o] = pack(mix(unpack_argb(out[o]), [0.95, 0.92, 0.85], tw * (1.0 - yf / fh * 1.2).clamp(0.0, 1.0)));
    }

    // samurái en reposo sobre el acantilado
    let scale = (h / 135).max(1);
    let frame = ((t * 4.0) as usize) % sprite::IDLE.len();
    let sx0 = (fw * 0.26) as i32;
    let feet = cliff[(sx0 as usize + sprite::W * scale / 2).min(w - 1)] as i32 + 2 * scale as i32;
    let top = feet - (sprite::H * scale) as i32;
    for py in 0..(sprite::H * scale) {
        for px in 0..(sprite::W * scale) {
            let p = sprite::IDLE[frame][(py / scale) * sprite::W + px / scale];
            let (dx, dy) = (sx0 + px as i32, top + py as i32);
            if p >> 24 != 0 && dx >= 0 && dy >= 0 && (dx as usize) < w && (dy as usize) < h {
                out[dy as usize * w + dx as usize] = p | 0xFF00_0000;
            }
        }
    }

    // pétalos de cerezo: llevados por el viento hacia la izquierda
    let petal = mix(mix(ac, [1.0, 0.85, 0.88], 0.62), horizon, 0.1);
    for i in 0..44u32 {
        let (a, b, c2) = (hash(i, 1), hash(i, 2), hash(i, 3));
        let px = (a - t * (0.010 + 0.014 * b) + 0.04 * (t * 0.8 + i as f32).sin()).rem_euclid(1.0);
        let py = (c2 + t * (0.035 + 0.035 * b)).rem_euclid(1.0);
        let (x, y) = ((px * fw) as usize, (py * fh) as usize);
        let flutter = if (t * 3.0 + i as f32).sin() > 0.0 { 2 } else { 1 };
        for dx in 0..flutter {
            if x + dx < w && y < h {
                let o = y * w + x + dx;
                let under = unpack_argb(out[o]);
                out[o] = pack(mix(under, petal, 0.9));
            }
        }
    }
    out
}

fn unpack_argb(p: u32) -> Rgb {
    [((p >> 16) & 0xff) as f32 / 255.0, ((p >> 8) & 0xff) as f32 / 255.0, (p & 0xff) as f32 / 255.0]
}

/// ¿Cae el píxel dentro de la silueta del torii (dos pilares, travesaño y dintel curvo)?
#[allow(clippy::too_many_arguments)]
fn in_torii(x: f32, y: f32, cx: f32, top: f32, bottom: f32, pil: f32, half: f32, fh: f32) -> bool {
    if y < top - fh * 0.03 || y > bottom {
        return false;
    }
    let dx = (x - cx).abs();
    // pilares
    if (dx - half).abs() < pil && y >= top {
        return true;
    }
    // travesaño inferior (nuki)
    if dx < half + pil && (y - (top + fh * 0.055)).abs() < fh * 0.008 {
        return true;
    }
    // dintel superior (kasagi): curvado hacia arriba en los extremos
    let lift = (dx / (half * 1.55)).powi(2) * fh * 0.02;
    dx < half * 1.55 && (y - (top - lift)).abs() < fh * 0.012
}

#[cfg(test)]
mod tests {
    use super::*;

    const BG: u32 = 0x0d0b0e; // #0e0b0d
    const AC: u32 = 0x2e10c8; // #c8102e

    #[test]
    fn frames_are_opaque_and_deterministic() {
        let (w, h) = scene_size(1920, 1080);
        assert_eq!((w, h), (480, 270));
        let a = render(w as usize, h as usize, 3.2, BG, AC);
        assert_eq!(a.len(), (w * h) as usize);
        assert!(a.iter().all(|p| p >> 24 == 0xff));
        assert_eq!(a, render(w as usize, h as usize, 3.2, BG, AC));
    }

    #[test]
    fn the_scene_moves_over_time() {
        let (w, h) = (320usize, 180usize);
        let a = render(w, h, 0.0, BG, AC);
        let b = render(w, h, 7.0, BG, AC);
        let changed = a.iter().zip(&b).filter(|(x, y)| x != y).count();
        assert!(changed > 100, "solo cambiaron {changed} píxeles");
        // pero no es un parpadeo: la mayor parte del cuadro (cordilleras, acantilado) queda igual
        assert!(changed < w * h / 2, "cambió demasiado: {changed}");
    }

    #[test]
    fn it_is_dark_and_has_red_and_a_bright_moon() {
        let (w, h) = (320usize, 180usize);
        let v = render(w, h, 1.0, BG, AC);
        let lum = |p: u32| ((p >> 16) & 0xff) + ((p >> 8) & 0xff) + (p & 0xff);
        let avg = v.iter().map(|&p| lum(p) as u64).sum::<u64>() / v.len() as u64;
        assert!(avg < 3 * 110, "demasiado clara para un tema oscuro: {avg}");
        let (mx, my) = ((w as f32 * 0.76) as usize, (h as f32 * 0.27) as usize);
        assert!(lum(v[my * w + mx]) > 3 * 190, "no se ve la luna");
        assert!(v.iter().any(|&p| ((p >> 16) & 0xff) > 100 && ((p >> 8) & 0xff) < 40), "no hay carmesí (torii, pañuelo)");
    }

    #[test]
    fn the_samurai_is_drawn_on_the_cliff() {
        let (w, h) = (320usize, 180usize);
        let with = render(w, h, 0.0, BG, AC);
        // la banda de píxeles carmesí del sprite (166, 0, 0) aparece sobre el acantilado
        assert!(with.iter().any(|&p| p & 0x00FF_FFFF == 0x00A60000), "falta el sprite");
    }

    #[test]
    fn tiny_canvases_do_not_panic() {
        for (w, h) in [(64usize, 36usize), (16, 9)] {
            assert_eq!(render(w, h, 0.5, BG, AC).len(), w * h);
        }
    }

    /// A mano: `cargo test dump_scene -- --ignored` escribe `%TEMP%\samurai_*.png` para ver la escena.
    #[test]
    #[ignore]
    fn dump_scene() {
        let (w, h) = scene_size(1920, 1080);
        for (i, t) in [0.0f32, 6.0].into_iter().enumerate() {
            let mut c = crate::gfx::Canvas::new(w, h).unwrap();
            c.fill_from(&render(w as usize, h as usize, t, BG, AC));
            let path = std::env::temp_dir().join(format!("samurai_{i}.png"));
            assert!(c.save_png(&path.to_string_lossy()));
        }
    }

    /// A mano: `cargo test --release time_a_frame -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn time_a_frame() {
        let (w, h) = scene_size(1920, 1080);
        let start = std::time::Instant::now();
        for i in 0..30 {
            std::hint::black_box(render(w as usize, h as usize, i as f32 / 10.0, BG, AC));
        }
        println!("{:?} por fotograma ({w}x{h})", start.elapsed() / 30);
    }
}

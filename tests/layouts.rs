use hypr_win::config::{parse_color, Config};
use hypr_win::keybinds::{parse_binding, parse_keys, Action, Dir, MOD_SHIFT, MOD_WIN};
use hypr_win::layout::{arrange, Gaps, LayoutKind, Rect};

const AREAS: [Rect; 3] = [
    Rect { x: 0, y: 32, w: 1920, h: 1048 },
    Rect { x: 0, y: 0, w: 1080, h: 1920 },
    Rect { x: 100, y: 50, w: 800, h: 600 },
];
const GAPS: [Gaps; 3] = [
    Gaps { inner: 8, outer: 8 },
    Gaps { inner: 0, outer: 0 },
    Gaps { inner: 15, outer: 4 },
];

fn all_layouts() -> [LayoutKind; 2] {
    [LayoutKind::Dwindle, LayoutKind::Master]
}

#[test]
fn returns_one_rect_per_window() {
    for kind in all_layouts() {
        for n in 0..=12 {
            assert_eq!(arrange(kind, AREAS[0], n, GAPS[0], 0.55).len(), n, "{kind:?} n={n}");
        }
    }
}

#[test]
fn single_window_fills_area_minus_outer_gap() {
    for kind in all_layouts() {
        let r = arrange(kind, AREAS[0], 1, GAPS[0], 0.55);
        assert_eq!(r[0], AREAS[0].shrink(8));
    }
}

#[test]
fn no_overlap_and_inner_gap_respected() {
    for kind in all_layouts() {
        for area in AREAS {
            for gaps in GAPS {
                // más de 8 ventanas en 800x600 con gap 15 ya no caben físicamente
                for n in 1..=8 {
                    let rects = arrange(kind, area, n, gaps, 0.55);
                    for (i, a) in rects.iter().enumerate() {
                        assert!(a.w > 0 && a.h > 0, "{kind:?} n={n} rect vacío");
                        for b in &rects[i + 1..] {
                            let inflated = Rect::new(
                                a.x - gaps.inner,
                                a.y - gaps.inner,
                                a.w + 2 * gaps.inner,
                                a.h + 2 * gaps.inner,
                            );
                            assert!(
                                !inflated.intersects(b),
                                "{kind:?} n={n} gap {gaps:?}: {a:?} y {b:?} demasiado juntos"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn rects_stay_inside_area_with_outer_gap() {
    for kind in all_layouts() {
        for area in AREAS {
            for gaps in GAPS {
                let bound = area.shrink(gaps.outer);
                for n in 1..=8 {
                    for r in arrange(kind, area, n, gaps, 0.55) {
                        assert!(r.x >= bound.x && r.y >= bound.y, "{kind:?} n={n}: {r:?}");
                        assert!(r.right() <= bound.right() && r.bottom() <= bound.bottom(), "{kind:?} n={n}: {r:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn master_layout_shape() {
    let gaps = Gaps { inner: 10, outer: 0 };
    let r = arrange(LayoutKind::Master, Rect::new(0, 0, 1010, 600), 4, gaps, 0.5);
    // master ocupa toda la altura a la izquierda
    assert_eq!((r[0].x, r[0].y, r[0].h), (0, 0, 600));
    // el resto se apila a la derecha, con el mismo x y ancho
    assert!(r[1..].iter().all(|s| s.x == r[1].x && s.w == r[1].w && s.x > r[0].right()));
    assert_eq!(r[3].bottom(), 600);
}

#[test]
fn dwindle_alternates_axes() {
    let gaps = Gaps { inner: 0, outer: 0 };
    let r = arrange(LayoutKind::Dwindle, Rect::new(0, 0, 1600, 900), 3, gaps, 0.5);
    assert_eq!(r[0], Rect::new(0, 0, 800, 900)); // vertical
    assert_eq!(r[1], Rect::new(800, 0, 800, 450)); // luego horizontal
    assert_eq!(r[2], Rect::new(800, 450, 800, 450));
}

#[test]
fn layout_parse_and_toggle() {
    assert_eq!(LayoutKind::parse("Master").unwrap(), LayoutKind::Master);
    assert!(LayoutKind::parse("x").is_err());
    assert_eq!(LayoutKind::Dwindle.toggled(), LayoutKind::Master);
}

#[test]
fn keys_parse() {
    let (m, vk) = parse_keys("Super+Shift+Left").unwrap();
    assert!(m & MOD_WIN != 0 && m & MOD_SHIFT != 0);
    assert_eq!(vk, 0x25);
    assert_eq!(parse_keys("Super+q").unwrap().1, b'Q' as u32);
    assert_eq!(parse_keys("Alt+F4").unwrap().1, 0x73);
    assert!(parse_keys("Super+").is_err());
    assert!(parse_keys("Super+A+B").is_err());
}

#[test]
fn bindings_parse() {
    let b = parse_binding("Super+Return, exec, wt.exe -d C:\\").unwrap();
    assert_eq!(b.action, Action::Exec("wt.exe -d C:\\".into()));
    assert_eq!(parse_binding("Super+3, workspace, 3").unwrap().action, Action::Workspace(2));
    assert_eq!(
        parse_binding("Super+Shift+Up, move, up").unwrap().action,
        Action::Move(Dir::Up)
    );
    assert!(parse_binding("Super+1, workspace, 0").is_err());
    assert!(parse_binding("Super+X, nada").is_err());
    assert!(parse_binding("Super+X, exec").is_err());
}

#[test]
fn default_config_is_valid() {
    let c = Config::default_config();
    assert_eq!(c.general.workspaces, 9);
    assert!(c.bindings().unwrap().len() >= 20);
}

#[test]
fn config_validation() {
    assert!(Config::parse("[general]\nworkspaces = 12").is_err());
    assert!(Config::parse("[bar]\nbackground = \"rojo\"").is_err());
    assert!(Config::parse("binds = [\"Super+9, workspace, 9\"]\n[general]\nworkspaces = 5").is_err());
    assert!(Config::parse("[general]\ngaps_in = 0").is_ok());
    assert_eq!(parse_color("#102030").unwrap(), 0x0030_2010);
}

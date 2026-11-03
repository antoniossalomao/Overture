//! Fundos de letra animados (estilo escuro), gerados do zero.
//!
//! Cada estilo é um `Style` com um `Engine` (tipo de movimento) e paleta própria. Todo movimento usa
//! multiplicadores inteiros de `TAU * phase`, onde `phase` vai de 0 a 1 a cada `MOTION_LOOP_SECS`.
//! Assim o frame em `t + MOTION_LOOP_SECS` é idêntico ao de `t`.
//!
//! Os mesmos frames podem ser exportados para vídeo (ver `examples/export_motion.rs`) e tocados via
//! libmpv, o que reduz o custo de CPU em tempo de apresentação.

use std::f32::consts::TAU;
use std::path::PathBuf;

pub const MOTION_LOOP_SECS: f32 = 24.0;
pub const MOTION_FPS: u32 = 30;

/// Grade de cálculo para campos suaves (ondas, névoa, órbitas, raios, anéis).
const GRID_DIV: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionPreset {
    Aurora,
    Ocean,
    WarmGold,
    /// Índice em `STYLES`.
    Style(usize),
}

#[derive(Debug, Clone, Copy)]
pub enum Shape {
    Disc,
    Ring,
    Hex,
    Sparkle,
}

#[derive(Debug, Clone, Copy)]
pub enum Engine {
    /// Partículas em disco, anel, hexágono ou faísca. `fall`: caem e quicam; senão sobem.
    Particles { count: u32, radius: f32, shape: Shape, fall: bool, speed: f32 },
    /// Faixas de cor fluindo com distorção.
    Waves { freq: f32, warp: f32, layers: u32 },
    /// Orbes grandes e suaves em trajetórias de Lissajous.
    Orbs { count: u32, radius: f32 },
    /// Raios diagonais que varrem a tela.
    Rays { count: u32, width: f32 },
    /// Névoa de baixa frequência.
    Fog { scale: f32 },
    /// Grade de pontos que pulsam.
    Grid { cells: f32 },
    /// Traços verticais caindo.
    Rain { count: u32, length: f32 },
}

pub struct Style {
    pub uri: &'static str,
    pub name: &'static str,
    /// Cor de base (quase preta).
    pub base: [f32; 3],
    /// Cor principal do brilho.
    pub tint: [f32; 3],
    /// Cor secundária (mistura por posição ou alternância entre elementos).
    pub tint2: [f32; 3],
    /// Intensidade máxima (0..1). Mantida baixa para o fundo ficar escuro.
    pub strength: f32,
    pub engine: Engine,
}

const fn p(count: u32, radius: f32, shape: Shape, fall: bool, speed: f32) -> Engine {
    Engine::Particles { count, radius, shape, fall, speed }
}

pub const STYLES: &[Style] = &[
    // --- Partículas (bokeh, anéis, hexágonos, faíscas) ---
    Style { uri: "motion://bokeh-teal", name: "Bokeh Azul-Petróleo", base: [12.0, 14.0, 16.0], tint: [150.0, 210.0, 230.0], tint2: [120.0, 180.0, 200.0], strength: 0.36, engine: p(170, 0.054, Shape::Disc, false, 1.0) },
    Style { uri: "motion://bokeh-graphite", name: "Bokeh Grafite", base: [12.0, 12.0, 13.0], tint: [200.0, 205.0, 212.0], tint2: [170.0, 175.0, 185.0], strength: 0.30, engine: p(140, 0.054, Shape::Disc, false, 1.0) },
    Style { uri: "motion://bokeh-violet", name: "Bokeh Violeta", base: [10.0, 8.0, 16.0], tint: [200.0, 170.0, 235.0], tint2: [150.0, 120.0, 210.0], strength: 0.33, engine: p(150, 0.054, Shape::Disc, false, 1.0) },
    Style { uri: "motion://bokeh-ember", name: "Bokeh Brasa Apagada", base: [12.0, 9.0, 8.0], tint: [230.0, 170.0, 120.0], tint2: [200.0, 130.0, 90.0], strength: 0.28, engine: p(120, 0.054, Shape::Disc, false, 1.0) },
    Style { uri: "motion://ring-drift", name: "Anéis à Deriva", base: [9.0, 10.0, 14.0], tint: [140.0, 170.0, 220.0], tint2: [110.0, 140.0, 200.0], strength: 0.38, engine: p(90, 0.03, Shape::Ring, false, 1.0) },
    Style { uri: "motion://hex-fall", name: "Hexágonos em Queda", base: [8.0, 12.0, 11.0], tint: [120.0, 200.0, 170.0], tint2: [90.0, 170.0, 140.0], strength: 0.36, engine: p(80, 0.03, Shape::Hex, true, 1.0) },
    Style { uri: "motion://sparkle-rain", name: "Faíscas Caindo", base: [8.0, 8.0, 10.0], tint: [245.0, 240.0, 220.0], tint2: [220.0, 210.0, 190.0], strength: 0.85, engine: p(60, 0.012, Shape::Sparkle, true, 1.0) },
    Style { uri: "motion://sparkle-steady", name: "Faíscas Suaves", base: [8.0, 9.0, 12.0], tint: [210.0, 225.0, 245.0], tint2: [180.0, 200.0, 230.0], strength: 0.70, engine: p(70, 0.010, Shape::Sparkle, false, 1.0) },
    Style { uri: "motion://bounce-steel", name: "Bolinhas que Quicam", base: [10.0, 11.0, 14.0], tint: [150.0, 165.0, 190.0], tint2: [120.0, 135.0, 165.0], strength: 0.40, engine: p(60, 0.032, Shape::Disc, true, 2.0) },
    Style { uri: "motion://sparkle-gold", name: "Faíscas Douradas", base: [12.0, 10.0, 7.0], tint: [235.0, 190.0, 110.0], tint2: [200.0, 160.0, 90.0], strength: 0.75, engine: p(55, 0.011, Shape::Sparkle, false, 1.0) },
    // --- Ondas de cor ---
    Style { uri: "motion://wave-teal", name: "Ondas Azul-Petróleo", base: [8.0, 12.0, 14.0], tint: [30.0, 110.0, 124.0], tint2: [20.0, 70.0, 90.0], strength: 0.50, engine: Engine::Waves { freq: 1.6, warp: 0.10, layers: 3 } },
    Style { uri: "motion://wave-indigo", name: "Ondas Índigo", base: [9.0, 9.0, 16.0], tint: [54.0, 52.0, 120.0], tint2: [40.0, 60.0, 130.0], strength: 0.48, engine: Engine::Waves { freq: 1.3, warp: 0.14, layers: 3 } },
    Style { uri: "motion://wave-plum", name: "Ondas Ameixa", base: [12.0, 8.0, 13.0], tint: [100.0, 46.0, 96.0], tint2: [80.0, 40.0, 110.0], strength: 0.42, engine: Engine::Waves { freq: 1.9, warp: 0.08, layers: 4 } },
    Style { uri: "motion://wave-moss", name: "Ondas Musgo", base: [9.0, 12.0, 9.0], tint: [60.0, 98.0, 62.0], tint2: [70.0, 90.0, 40.0], strength: 0.39, engine: Engine::Waves { freq: 1.4, warp: 0.12, layers: 3 } },
    Style { uri: "motion://wave-ember", name: "Ondas Brasa", base: [13.0, 9.0, 8.0], tint: [120.0, 62.0, 36.0], tint2: [140.0, 80.0, 40.0], strength: 0.40, engine: Engine::Waves { freq: 1.2, warp: 0.16, layers: 2 } },
    // --- Orbes ---
    Style { uri: "motion://orbs-blue", name: "Orbes Azuis", base: [6.0, 8.0, 14.0], tint: [40.0, 80.0, 150.0], tint2: [30.0, 110.0, 130.0], strength: 0.40, engine: Engine::Orbs { count: 6, radius: 0.26 } },
    Style { uri: "motion://orbs-rose", name: "Orbes Rosé", base: [12.0, 8.0, 11.0], tint: [120.0, 50.0, 80.0], tint2: [90.0, 60.0, 110.0], strength: 0.36, engine: Engine::Orbs { count: 5, radius: 0.30 } },
    Style { uri: "motion://orbs-gold", name: "Orbes Âmbar", base: [12.0, 10.0, 7.0], tint: [120.0, 85.0, 30.0], tint2: [100.0, 60.0, 30.0], strength: 0.34, engine: Engine::Orbs { count: 5, radius: 0.24 } },
    Style { uri: "motion://orbs-mint", name: "Orbes Menta", base: [7.0, 12.0, 11.0], tint: [40.0, 120.0, 100.0], tint2: [50.0, 100.0, 130.0], strength: 0.36, engine: Engine::Orbs { count: 7, radius: 0.20 } },
    // --- Raios ---
    Style { uri: "motion://rays-steel", name: "Raios de Aço", base: [9.0, 10.0, 13.0], tint: [120.0, 135.0, 165.0], tint2: [100.0, 115.0, 150.0], strength: 0.36, engine: Engine::Rays { count: 6, width: 0.07 } },
    Style { uri: "motion://rays-cyan", name: "Raios Ciano", base: [6.0, 11.0, 13.0], tint: [60.0, 170.0, 190.0], tint2: [50.0, 130.0, 170.0], strength: 0.34, engine: Engine::Rays { count: 5, width: 0.06 } },
    Style { uri: "motion://rays-violet", name: "Raios Violetas", base: [10.0, 7.0, 14.0], tint: [110.0, 70.0, 170.0], tint2: [90.0, 60.0, 150.0], strength: 0.36, engine: Engine::Rays { count: 6, width: 0.08 } },
    Style { uri: "motion://rays-warm", name: "Raios Quentes", base: [12.0, 9.0, 7.0], tint: [150.0, 95.0, 45.0], tint2: [130.0, 80.0, 40.0], strength: 0.34, engine: Engine::Rays { count: 5, width: 0.07 } },
        // --- Névoa ---
    Style { uri: "motion://fog-slate", name: "Névoa Ardósia", base: [9.0, 10.0, 12.0], tint: [70.0, 78.0, 92.0], tint2: [60.0, 70.0, 85.0], strength: 0.55, engine: Engine::Fog { scale: 1.6 } },
    Style { uri: "motion://fog-forest", name: "Névoa Floresta", base: [7.0, 11.0, 9.0], tint: [45.0, 80.0, 60.0], tint2: [40.0, 70.0, 75.0], strength: 0.50, engine: Engine::Fog { scale: 2.2 } },
    Style { uri: "motion://fog-dusk", name: "Névoa Crepúsculo", base: [11.0, 8.0, 13.0], tint: [85.0, 55.0, 95.0], tint2: [70.0, 60.0, 100.0], strength: 0.50, engine: Engine::Fog { scale: 1.9 } },
    // --- Grade ---
    Style { uri: "motion://grid-pulse", name: "Grade Pulsante", base: [8.0, 9.0, 11.0], tint: [90.0, 120.0, 150.0], tint2: [90.0, 120.0, 150.0], strength: 0.55, engine: Engine::Grid { cells: 24.0 } },
    Style { uri: "motion://grid-amber", name: "Grade Âmbar", base: [11.0, 9.0, 7.0], tint: [150.0, 105.0, 50.0], tint2: [150.0, 105.0, 50.0], strength: 0.50, engine: Engine::Grid { cells: 16.0 } },
    // --- Chuva ---
    Style { uri: "motion://rain-blue", name: "Chuva Azul", base: [7.0, 9.0, 13.0], tint: [110.0, 150.0, 200.0], tint2: [90.0, 130.0, 185.0], strength: 0.42, engine: Engine::Rain { count: 120, length: 0.10 } },
    Style { uri: "motion://rain-grey", name: "Chuva Cinza", base: [10.0, 10.0, 11.0], tint: [150.0, 150.0, 158.0], tint2: [130.0, 130.0, 140.0], strength: 0.36, engine: Engine::Rain { count: 160, length: 0.07 } },
];

/// Presets legados (Aurora, Oceano e Âmbar): mantidos porque temas já cadastrados os usam.
impl MotionPreset {
    pub fn from_uri(uri: &str) -> Self {
        let lower = uri.to_ascii_lowercase();
        if let Some(i) = STYLES.iter().position(|s| s.uri == lower) {
            return Self::Style(i);
        }
        if lower.contains("ocean") || lower.contains("agua") {
            Self::Ocean
        } else if lower.contains("warm") || lower.contains("gold") || lower.contains("fogo") {
            Self::WarmGold
        } else {
            Self::Aurora
        }
    }
}

/// Gera um frame RGBA `(width, height)` do fundo `preset` no instante `t_secs`.
pub fn generate_motion_frame(width: u32, height: u32, t_secs: f32, preset: MotionPreset) -> Vec<u8> {
    match preset {
        MotionPreset::Style(i) => generate_style_frame(width, height, t_secs, &STYLES[i.min(STYLES.len() - 1)]),
        legacy => generate_legacy_frame(width, height, t_secs, legacy),
    }
}

/// Procura o arquivo de vídeo exportado de um estilo (`<nome>.mp4`) em pastas conhecidas.
pub fn motion_video_path(uri: &str) -> Option<PathBuf> {
    let name = uri.strip_prefix("motion://")?;
    let file = format!("{name}.mp4");
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        dirs.push(dir.join("assets").join("motion"));
        dirs.push(dir.join("motion"));
    }
    dirs.push(PathBuf::from("assets").join("motion"));
    dirs.into_iter().map(|d| d.join(&file)).find(|p| p.is_file())
}

// ---------------------------------------------------------------------------------------------
// Utilidades

/// Hash determinístico (0..1) para espalhar elementos sem aleatoriedade.
fn hash01(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B1) ^ salt.wrapping_mul(0x85EB_CA77);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0x00FF_FFFF) as f32 / 16_777_216.0
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn phase_of(t_secs: f32) -> f32 {
    t_secs.rem_euclid(MOTION_LOOP_SECS) / MOTION_LOOP_SECS
}

/// Renderiza um campo de cor calculado numa grade de 1/`GRID_DIV` e interpolado até a resolução final.
/// `f(nx, ny, aspect)` devolve a cor (0..255) para coordenadas normalizadas em 0..1.
fn render_grid<F: Fn(f32, f32, f32) -> [f32; 3]>(width: u32, height: u32, f: F) -> Vec<u8> {
    let aspect = width.max(1) as f32 / height.max(1) as f32;
    let gw = (width / GRID_DIV).max(2);
    let gh = (height / GRID_DIV).max(2);
    let mut grid = vec![[0.0f32; 3]; (gw * gh) as usize];
    for gy in 0..gh {
        let ny = gy as f32 / (gh - 1) as f32;
        for gx in 0..gw {
            let nx = gx as f32 / (gw - 1) as f32;
            grid[(gy * gw + gx) as usize] = f(nx, ny, aspect);
        }
    }

    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    let sx = (gw - 1) as f32 / width.max(1) as f32;
    let sy = (gh - 1) as f32 / height.max(1) as f32;
    let cols: Vec<(usize, usize, f32)> = (0..width)
        .map(|x| {
            let fx = x as f32 * sx;
            let x0 = fx.floor() as u32;
            let x1 = (x0 + 1).min(gw - 1);
            (x0 as usize, x1 as usize, fx - x0 as f32)
        })
        .collect();
    let gwu = gw as usize;
    for y in 0..height {
        let fy = y as f32 * sy;
        let y0 = (fy.floor() as u32).min(gh - 1) as usize;
        let y1 = (y0 + 1).min(gh as usize - 1);
        let ty = fy - y0 as f32;
        let row0 = &grid[y0 * gwu..(y0 + 1) * gwu];
        let row1 = &grid[y1 * gwu..(y1 + 1) * gwu];
        let out = &mut pixels[(y as usize) * (width as usize) * 4..(y as usize + 1) * (width as usize) * 4];
        for (x, &(x0, x1, tx)) in cols.iter().enumerate() {
            let (c00, c10, c01, c11) = (row0[x0], row0[x1], row1[x0], row1[x1]);
            let o = x * 4;
            for c in 0..3 {
                let top = c00[c] + (c10[c] - c00[c]) * tx;
                let bot = c01[c] + (c11[c] - c01[c]) * tx;
                out[o + c] = (top + (bot - top) * ty).clamp(0.0, 255.0) as u8;
            }
            out[o + 3] = 255;
        }
    }
    pixels
}

/// Gradiente de fundo: base em cima, um pouco mais clara embaixo.
fn background(base: [f32; 3], ny: f32) -> [f32; 3] {
    let g = smoothstep(0.1, 1.0, ny) * 0.6 + 0.4 * ny;
    [base[0] * (0.8 + 0.4 * g), base[1] * (0.8 + 0.4 * g), base[2] * (0.8 + 0.4 * g)]
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn add_scaled(a: [f32; 3], c: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] + c[0] * k, a[1] + c[1] * k, a[2] + c[2] * k]
}

// ---------------------------------------------------------------------------------------------
// Estilos

fn generate_style_frame(width: u32, height: u32, t: f32, s: &Style) -> Vec<u8> {
    let phase = phase_of(t);
    let str_ = s.strength;
    match s.engine {
        Engine::Particles { count, radius, shape, fall, speed } => {
            particles_frame(width, height, phase, s, count, radius, shape, fall, speed)
        }
        Engine::Waves { freq, warp, layers } => render_grid(width, height, |nx, ny, aspect| {
            let ax = nx * aspect;
            let wx = ax + warp * (TAU * phase + ny * TAU).sin();
            let mut v = (TAU * (freq * ny + 0.35 * wx) + TAU * phase).sin();
            v += 0.6 * (TAU * (0.7 * freq * wx - 0.5 * ny) + TAU * 2.0 * phase + 1.3).sin();
            v += 0.4 * (TAU * 0.5 * freq * (wx + ny) - TAU * phase + 2.7).sin();
            if layers >= 4 {
                v += 0.3 * (TAU * (1.7 * freq * wx + 0.3 * ny) - TAU * 2.0 * phase + 0.9).sin();
            }
            let v = v / (2.0 + if layers >= 4 { 0.3 } else { 0.0 });
            let band = smoothstep(0.0, 1.0, v * 0.5 + 0.5).powi(2);
            let hue = smoothstep(0.3, 0.9, 0.5 + 0.5 * (TAU * (0.8 * ny + phase)).sin());
            let col = mix3(s.tint, s.tint2, hue);
            let k = str_ * band * (0.6 + 0.4 * ny);
            mix3(background(s.base, ny), col, k).map(|c| c.min(255.0))
        }),
        Engine::Orbs { count, radius } => render_grid(width, height, |nx, ny, aspect| {
            let mut col = background(s.base, ny);
            for i in 0..count {
                let h0 = hash01(i, 10);
                let h1 = hash01(i, 11);
                let kx = 1.0 + (i % 3) as f32;
                let ky = 1.0 + ((i + 1) % 2) as f32;
                let cx = 0.5 + (0.22 + 0.2 * h0) * (TAU * phase * kx + h1 * TAU).cos();
                let cy = 0.5 + (0.18 + 0.15 * h1) * (TAU * phase * ky + h0 * TAU * 1.3).sin();
                let r = radius * (0.8 + 0.4 * h0);
                let dx = (nx * aspect - cx * aspect) / r;
                let dy = (ny - cy) / r;
                let g = (-(dx * dx + dy * dy)).exp();
                let c = if i % 2 == 0 { s.tint } else { s.tint2 };
                col = add_scaled(col, c, str_ * g);
            }
            col.map(|c| c.min(255.0))
        }),
        Engine::Rays { count, width: rw } => render_grid(width, height, |nx, ny, aspect| {
            let mut col = background(s.base, ny);
            let taper = smoothstep(0.0, 0.35, ny) * smoothstep(1.0, 0.65, ny);
            for i in 0..count {
                let start = hash01(i, 21);
                let slope = 0.45; // todos paralelos: feixes de luz, não zigue-zague
                let x_line = (start + phase) .rem_euclid(1.0) * aspect + slope * (ny - 0.5) * aspect;
                let mut dx = nx * aspect - x_line;
                dx -= (dx / aspect).round() * aspect; // distância em anel (a tela dá a volta)
                let g = (-(dx / rw).powi(2)).exp() * taper;
                let c = if i % 2 == 0 { s.tint } else { s.tint2 };
                col = add_scaled(col, c, str_ * g / count as f32 * 2.0);
            }
            col.map(|c| c.min(255.0))
        }),
        Engine::Fog { scale } => render_grid(width, height, |nx, ny, aspect| {
            let mut v = 0.0;
            for j in 0..3 {
                let a = (1.0 + hash01(j, 40) * 2.0) * scale;
                let b = (0.5 + hash01(j, 41) * 1.5) * scale;
                let c = 1 + (j % 2);
                v += (TAU * (a * nx * aspect + b * ny) + TAU * c as f32 * phase + hash01(j, 42) * TAU).sin();
            }
            let fog = smoothstep(-1.2, 2.0, v);
            mix3(background(s.base, ny), mix3(s.tint, s.tint2, ny), fog * str_).map(|c| c.min(255.0))
        }),
        Engine::Grid { cells } => grid_frame(width, height, phase, s, cells),
        Engine::Rain { count, length } => rain_frame(width, height, phase, s, count, length),
    }
}

#[allow(clippy::too_many_arguments)]
fn particles_frame(
    width: u32,
    height: u32,
    phase: f32,
    s: &Style,
    count: u32,
    radius_frac: f32,
    shape: Shape,
    fall: bool,
    speed: f32,
) -> Vec<u8> {
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    let mut acc = vec![0.0f32; (width as usize) * (height as usize)];
    let mut acc_tint2 = vec![0.0f32; (width as usize) * (height as usize)];

    for i in 0..count {
        let bx = hash01(i, 1);
        let by = hash01(i, 2);
        let depth = hash01(i, 3);
        let r = radius_frac * h * (0.55 + 0.9 * depth);
        let bright = s.strength * (0.3 + 0.7 * depth);
        let sway = 0.012 + 0.02 * hash01(i, 5);
        let sway_ph = TAU * hash01(i, 6);
        let cx = (bx + sway * (TAU * phase + sway_ph).sin()).rem_euclid(1.0) * w;
        // Queda com quique: |sin| periódico em phase (speed inteiro). Subida: fração linear.
        let u = by + phase * speed;
        let cy0 = if fall { (1.0 - (std::f32::consts::PI * u).sin().abs()) * h } else { (1.0 - u.rem_euclid(1.0)) * h };
        let use_t2 = i % 2 == 1;
        let target = if use_t2 { &mut acc_tint2 } else { &mut acc };

        for k in -1..=1 {
            let cy = cy0 + k as f32 * h;
            let x_min = (cx - r).floor().max(0.0) as u32;
            let x_max = (cx + r).ceil().min(w - 1.0) as u32;
            let y_min = (cy - r).floor().max(0.0) as u32;
            let y_max = (cy + r).ceil().min(h - 1.0) as u32;
            if x_min > x_max || y_min > y_max {
                continue;
            }
            for py in y_min..=y_max {
                let dy = py as f32 + 0.5 - cy;
                for px in x_min..=x_max {
                    let dx = px as f32 + 0.5 - cx;
                    let d = (dx * dx + dy * dy).sqrt() / r;
                    let v = match shape {
                        Shape::Disc => {
                            if d >= 1.0 { continue; }
                            (1.0 - d * d) * (0.75 + 0.5 * smoothstep(0.85, 1.0, d))
                        }
                        Shape::Ring => {
                            (-((d - 0.8) / 0.12).powi(2)).exp()
                        }
                        Shape::Hex => {
                            let hx = dx.abs() / r;
                            let hy = (dx * 0.5 + dy * 0.866).abs() / r;
                            let hz = (-dx * 0.5 + dy * 0.866).abs() / r;
                            let hd = hx.max(hy).max(hz);
                            if hd >= 1.0 { continue; }
                            (1.0 - hd * hd) * (0.6 + 0.4 * smoothstep(0.8, 1.0, hd))
                        }
                        Shape::Sparkle => {
                            if d >= 2.0 { continue; }
                            let core = (-(d * 2.5).powi(2)).exp();
                            let arm_x = (-((dx / (0.12 * r)).powi(2))).exp() * (-(d * 1.2).powi(2)).exp();
                            let arm_y = (-((dy / (0.12 * r)).powi(2))).exp() * (-(d * 1.2).powi(2)).exp();
                            core + 0.6 * (arm_x + arm_y)
                        }
                    };
                    target[(py * width + px) as usize] += bright * v;
                }
            }
        }
    }

    // Fundo + partículas (tint e tint2) em RGBA.
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    for y in 0..height {
        let ny = y as f32 / h;
        let bg = background(s.base, ny);
        for x in 0..width {
            let p = (y * width + x) as usize;
            let a1 = acc[p].min(1.0);
            let a2 = acc_tint2[p].min(1.0);
            let c = add_scaled(add_scaled(bg, s.tint, a1), s.tint2, a2);
            let idx = p * 4;
            for k in 0..3 {
                pixels[idx + k] = c[k].clamp(0.0, 255.0) as u8;
            }
            pixels[idx + 3] = 255;
        }
    }
    pixels
}

fn grid_frame(width: u32, height: u32, phase: f32, s: &Style, cells: f32) -> Vec<u8> {
    let aspect = width.max(1) as f32 / height.max(1) as f32;
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    for y in 0..height {
        let ny = y as f32 / height.max(1) as f32;
        let bg = background(s.base, ny);
        for x in 0..width {
            let nx = x as f32 / width.max(1) as f32;
            let px = nx * aspect * cells;
            let py = ny * cells;
            let cell = (px.floor(), py.floor());
            let local = (px - cell.0 - 0.5, py - cell.1 - 0.5);
            let d = (local.0 * local.0 + local.1 * local.1).sqrt() * 2.0;
            let id = (cell.0 as i64 as u32).wrapping_mul(73) ^ (cell.1 as i64 as u32).wrapping_mul(151);
            let pulse = 0.5 + 0.5 * (TAU * phase + TAU * hash01(id, 50)).sin();
            let g = (-(d / 0.14).powi(2)).exp() * pulse * s.strength;
            let c = add_scaled(bg, s.tint, g);
            let idx = ((y * width + x) * 4) as usize;
            for k in 0..3 {
                pixels[idx + k] = c[k].clamp(0.0, 255.0) as u8;
            }
            pixels[idx + 3] = 255;
        }
    }
    pixels
}

fn rain_frame(width: u32, height: u32, phase: f32, s: &Style, count: u32, length: f32) -> Vec<u8> {
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    let mut acc = vec![0.0f32; (width as usize) * (height as usize)];
    for i in 0..count {
        let x = hash01(i, 60) * w;
        let speed = if hash01(i, 61) < 0.5 { 1.0 } else { 2.0 };
        let head = (hash01(i, 62) + phase * speed).rem_euclid(1.0) * h;
        let len = length * h * (0.6 + 0.8 * hash01(i, 63));
        let bright = s.strength * (0.5 + 0.5 * hash01(i, 64));
        for k in -1..=1 {
            let hy = head + k as f32 * h;
            let y_min = (hy - len).floor().max(0.0) as u32;
            let y_max = hy.ceil().min(h - 1.0) as u32;
            if y_min > y_max { continue; }
            let x_min = (x - 1.0).floor().max(0.0) as u32;
            let x_max = (x + 1.0).ceil().min(w - 1.0) as u32;
            for py in y_min..=y_max {
                let t = ((hy - py as f32) / len).clamp(0.0, 1.0);
                let v = bright * (1.0 - t) * (1.0 - t);
                for px in x_min..=x_max {
                    let dx = (px as f32 + 0.5 - x).abs();
                    let column = (1.0 - dx).max(0.0);
                    acc[(py * width + px) as usize] += v * column;
                }
            }
        }
    }
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    for y in 0..height {
        let ny = y as f32 / h;
        let bg = background(s.base, ny);
        for x in 0..width {
            let a = acc[(y * width + x) as usize].min(1.0);
            let c = add_scaled(bg, s.tint, a);
            let idx = ((y * width + x) * 4) as usize;
            for k in 0..3 {
                pixels[idx + k] = c[k].clamp(0.0, 255.0) as u8;
            }
            pixels[idx + 3] = 255;
        }
    }
    pixels
}

/// Presets originais (Aurora, Oceano, Âmbar). Movimento sem período fixo, como antes.
fn generate_legacy_frame(width: u32, height: u32, t_secs: f32, preset: MotionPreset) -> Vec<u8> {
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    let wf = width.max(1) as f32;
    let hf = height.max(1) as f32;
    for y in 0..height {
        let ny = y as f32 / hf;
        let wave_y = (ny * 4.2 + t_secs * 0.65).sin();
        let row_offset = (y * width) as usize * 4;
        for x in 0..width {
            let nx = x as f32 / wf;
            let wave_x = (nx * 5.0 - t_secs * 0.8).cos();
            let diag = ((nx + ny) * 3.5 + t_secs * 0.5).sin();
            let v = ((wave_x + wave_y + diag) / 3.0) * 0.5 + 0.5;
            let (r, g, b) = match preset {
                MotionPreset::Ocean => ((6.0 + v * 22.0) as u8, (34.0 + v * 85.0) as u8, (68.0 + v * 145.0) as u8),
                MotionPreset::WarmGold => ((42.0 + v * 125.0) as u8, (22.0 + v * 72.0) as u8, (10.0 + v * 28.0) as u8),
                _ => ((12.0 + v * 36.0) as u8, (22.0 + v * 92.0 * (1.0 - ny * 0.4)) as u8, (48.0 + v * 135.0) as u8),
            };
            let idx = row_offset + (x as usize * 4);
            pixels[idx] = r;
            pixels[idx + 1] = g;
            pixels[idx + 2] = b;
            pixels[idx + 3] = 255;
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_styles_loop_seamlessly() {
        for (i, s) in STYLES.iter().enumerate() {
            let p = MotionPreset::Style(i);
            let a = generate_motion_frame(64, 36, 7.0, p);
            let b = generate_motion_frame(64, 36, 7.0 + MOTION_LOOP_SECS, p);
            assert_eq!(a, b, "{} não fecha o ciclo", s.uri);
        }
    }

    #[test]
    fn uris_resolve_to_their_style() {
        for (i, s) in STYLES.iter().enumerate() {
            assert_eq!(MotionPreset::from_uri(s.uri), MotionPreset::Style(i));
        }
        assert_eq!(MotionPreset::from_uri("motion://ocean"), MotionPreset::Ocean);
    }

    #[test]
    fn styles_stay_dark() {
        assert!(STYLES.len() >= 30, "esperados 30 estilos, há {}", STYLES.len());
        for (i, s) in STYLES.iter().enumerate() {
            assert!(s.base.iter().cloned().fold(0.0f32, f32::max) <= 20.0, "{} base clara", s.uri);
            assert!(s.strength <= 0.9, "{} brilho alto", s.uri);
            let f = generate_motion_frame(96, 54, 3.0, MotionPreset::Style(i));
            let mean: f32 = f.chunks(4).map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0).sum::<f32>()
                / (f.len() / 4) as f32;
            assert!(mean < 70.0, "{} média {:.1} clara demais", s.uri, mean);
        }
    }
}

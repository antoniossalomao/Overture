//! Fundos de letra animados gerados do zero (sem vídeo nem imagem).
//!
//! Todo movimento usa multiplicadores inteiros de `TAU * phase`, onde `phase` vai de 0 a 1 a cada
//! `MOTION_LOOP_SECS`. Assim o frame em `t + MOTION_LOOP_SECS` é idêntico ao de `t` e o ciclo não aparece.
//!
//! Estilos:
//! - **Bokeh**: partículas suaves e faíscas subindo devagar sobre gradiente escuro.
//! - **Ondas**: faixas de cor fluindo com distorção suave, em brilho baixo.
//! - **Legado**: Aurora, Oceano e Âmbar (presets originais, mantidos por compatibilidade).

use std::f32::consts::TAU;

pub const MOTION_LOOP_SECS: f32 = 24.0;

/// Escala da grade de cálculo das ondas. Os valores são calculados nessa resolução e
/// interpolados até a resolução final; as ondas são suaves, então não há perda visível.
const WAVE_GRID_DIV: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionPreset {
    Aurora,
    Ocean,
    WarmGold,
    /// Índice em `BOKEH_STYLES`.
    Bokeh(usize),
    /// Índice em `WAVE_STYLES`.
    Wave(usize),
}

impl MotionPreset {
    pub fn from_uri(uri: &str) -> Self {
        let lower = uri.to_ascii_lowercase();
        if let Some(i) = BOKEH_STYLES.iter().position(|m| m.uri == lower) {
            return Self::Bokeh(i);
        }
        if let Some(i) = WAVE_STYLES.iter().position(|m| m.uri == lower) {
            return Self::Wave(i);
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

/// Fundo de partículas bokeh.
pub struct BokehStyle {
    pub uri: &'static str,
    pub name: &'static str,
    /// Topo e base do gradiente (cores de fundo).
    pub top: [f32; 3],
    pub bottom: [f32; 3],
    /// Cor do brilho das partículas.
    pub tint: [f32; 3],
    /// Intensidade máxima do brilho (0..1). Valores baixos deixam o fundo mais sóbrio.
    pub glow: f32,
    /// Número de partículas.
    pub count: u32,
}

/// Fundo de ondas de cor.
pub struct WaveStyle {
    pub uri: &'static str,
    pub name: &'static str,
    /// Cor de base escura.
    pub base: [f32; 3],
    /// Cor das faixas de onda.
    pub band: [f32; 3],
    /// Intensidade das faixas (0..1). Acima de 0,5 o fundo deixa de ser escuro.
    pub strength: f32,
    /// Quantidade de faixas na tela (frequência espacial).
    pub freq: f32,
    /// Força da distorção que dobra as faixas.
    pub warp: f32,
}

pub const BOKEH_STYLES: &[BokehStyle] = &[
    BokehStyle { uri: "motion://bokeh-teal", name: "Bokeh Azul-Petróleo", top: [12.0, 14.0, 16.0], bottom: [14.0, 62.0, 78.0], tint: [150.0, 210.0, 230.0], glow: 0.55, count: 170 },
    BokehStyle { uri: "motion://bokeh-graphite", name: "Bokeh Grafite", top: [12.0, 12.0, 13.0], bottom: [40.0, 42.0, 46.0], tint: [200.0, 205.0, 212.0], glow: 0.45, count: 140 },
    BokehStyle { uri: "motion://bokeh-violet", name: "Bokeh Violeta", top: [10.0, 8.0, 16.0], bottom: [48.0, 30.0, 78.0], tint: [200.0, 170.0, 235.0], glow: 0.50, count: 150 },
    BokehStyle { uri: "motion://bokeh-ember", name: "Bokeh Brasa Apagada", top: [12.0, 9.0, 8.0], bottom: [70.0, 38.0, 24.0], tint: [230.0, 170.0, 120.0], glow: 0.40, count: 120 },
];

pub const WAVE_STYLES: &[WaveStyle] = &[
    WaveStyle { uri: "motion://wave-teal", name: "Ondas Azul-Petróleo", base: [8.0, 12.0, 14.0], band: [30.0, 110.0, 124.0], strength: 0.50, freq: 1.6, warp: 0.10 },
    WaveStyle { uri: "motion://wave-indigo", name: "Ondas Índigo", base: [9.0, 9.0, 16.0], band: [54.0, 52.0, 120.0], strength: 0.48, freq: 1.3, warp: 0.14 },
    WaveStyle { uri: "motion://wave-plum", name: "Ondas Ameixa", base: [12.0, 8.0, 13.0], band: [100.0, 46.0, 96.0], strength: 0.42, freq: 1.9, warp: 0.08 },
    WaveStyle { uri: "motion://wave-moss", name: "Ondas Musgo", base: [9.0, 12.0, 9.0], band: [60.0, 98.0, 62.0], strength: 0.39, freq: 1.4, warp: 0.12 },
    WaveStyle { uri: "motion://wave-slate", name: "Ondas Ardósia", base: [10.0, 11.0, 13.0], band: [78.0, 90.0, 104.0], strength: 0.33, freq: 1.7, warp: 0.10 },
    WaveStyle { uri: "motion://wave-ember", name: "Ondas Brasa", base: [13.0, 9.0, 8.0], band: [120.0, 62.0, 36.0], strength: 0.39, freq: 1.2, warp: 0.16 },
];

/// Gera um frame RGBA `(width, height)` do fundo `preset` no instante `t_secs`.
pub fn generate_motion_frame(width: u32, height: u32, t_secs: f32, preset: MotionPreset) -> Vec<u8> {
    match preset {
        MotionPreset::Bokeh(i) => {
            generate_bokeh_frame(width, height, t_secs, &BOKEH_STYLES[i.min(BOKEH_STYLES.len() - 1)])
        }
        MotionPreset::Wave(i) => {
            generate_wave_frame(width, height, t_secs, &WAVE_STYLES[i.min(WAVE_STYLES.len() - 1)])
        }
        legacy => generate_legacy_frame(width, height, t_secs, legacy),
    }
}

/// Hash determinístico (0..1) para espalhar as partículas sem depender de aleatoriedade.
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

/// Bokeh: gradiente de fundo + discos de luz suave e faíscas, cada um subindo com velocidade
/// de um número inteiro de alturas por ciclo.
fn generate_bokeh_frame(width: u32, height: u32, t_secs: f32, style: &BokehStyle) -> Vec<u8> {
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    let phase = t_secs.rem_euclid(MOTION_LOOP_SECS) / MOTION_LOOP_SECS;

    let mut acc = vec![0.0f32; (width as usize) * (height as usize)];

    for i in 0..style.count {
        let bx = hash01(i, 1);
        let by = hash01(i, 2);
        let depth = hash01(i, 3); // 0 = longe (pequena e lenta), 1 = perto (grande e um pouco mais rápida)
        let sparkle = hash01(i, 4) < 0.18;
        let radius = if sparkle { 0.006 } else { 0.014 + depth * 0.040 } * h;
        let bright = style.glow * if sparkle { 1.0 } else { 0.25 + depth * 0.5 };
        let speed = if !sparkle && depth > 0.6 { 2.0 } else { 1.0 };
        let sway_amp = 0.012 + 0.02 * hash01(i, 5);
        let sway_ph = TAU * hash01(i, 6);

        let cx = (bx + sway_amp * (TAU * phase + sway_ph).sin()).rem_euclid(1.0) * w;
        let cy0 = (by - phase * speed).rem_euclid(1.0) * h;

        for k in -1..=1 {
            let cy = cy0 + k as f32 * h;
            let x_min = (cx - radius).floor().max(0.0) as u32;
            let x_max = (cx + radius).ceil().min(w - 1.0) as u32;
            let y_min = (cy - radius).floor().max(0.0) as u32;
            let y_max = (cy + radius).ceil().min(h - 1.0) as u32;
            if x_min > x_max || y_min > y_max {
                continue;
            }
            for py in y_min..=y_max {
                let dy = py as f32 + 0.5 - cy;
                for px in x_min..=x_max {
                    let dx = px as f32 + 0.5 - cx;
                    let d = (dx * dx + dy * dy).sqrt() / radius;
                    if d >= 1.0 {
                        continue;
                    }
                    let edge = smoothstep(0.85, 1.0, d);
                    let v = bright * (1.0 - d * d) * (0.75 + 0.5 * edge);
                    acc[(py * width + px) as usize] += v;
                }
            }
        }
    }

    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    for y in 0..height {
        let ny = y as f32 / h;
        let grad = smoothstep(0.15, 1.0, ny);
        for x in 0..width {
            let p = (y * width + x) as usize;
            let a = acc[p].min(1.0);
            let idx = p * 4;
            for c in 0..3 {
                let base = style.top[c] + (style.bottom[c] - style.top[c]) * grad;
                pixels[idx + c] = (base + style.tint[c] * a).clamp(0.0, 255.0) as u8;
            }
            pixels[idx + 3] = 255;
        }
    }
    pixels
}

/// Ondas: três senoides de baixa frequência com distorção, mapeadas para faixas de cor em brilho baixo.
/// Calculado numa grade reduzida e interpolado até a resolução final.
fn generate_wave_frame(width: u32, height: u32, t_secs: f32, style: &WaveStyle) -> Vec<u8> {
    let phase = t_secs.rem_euclid(MOTION_LOOP_SECS) / MOTION_LOOP_SECS;
    let aspect = width.max(1) as f32 / height.max(1) as f32;

    let gw = (width / WAVE_GRID_DIV).max(2);
    let gh = (height / WAVE_GRID_DIV).max(2);
    let mut grid = vec![[0.0f32; 3]; (gw * gh) as usize];

    for gy in 0..gh {
        let ny = gy as f32 / (gh - 1) as f32;
        for gx in 0..gw {
            let nx = gx as f32 / (gw - 1) as f32;
            let ax = nx * aspect;
            // Distorção: todos os termos usam multiplicadores inteiros da fase.
            let wx = ax + style.warp * (TAU * phase + ny * TAU).sin();
            let v1 = (TAU * (style.freq * ny + 0.35 * wx) + TAU * phase).sin();
            let v2 = (TAU * (0.7 * style.freq * wx - 0.5 * ny) + TAU * 2.0 * phase + 1.3).sin();
            let v3 = (TAU * 0.5 * style.freq * (wx + ny) - TAU * phase + 2.7).sin();
            let v = (v1 + 0.6 * v2 + 0.4 * v3) / 2.0; // -1..1
            let band = smoothstep(0.0, 1.0, v * 0.5 + 0.5).powi(2);
            let vignette = 0.6 + 0.4 * ny; // mais forte embaixo, escuro no topo
            let k = style.strength * band * vignette;
            grid[(gy * gw + gx) as usize] = [
                style.base[0] + (style.band[0] - style.base[0]) * k,
                style.base[1] + (style.band[1] - style.base[1]) * k,
                style.base[2] + (style.band[2] - style.base[2]) * k,
            ];
        }
    }

    // Interpolação bilinear da grade até (width, height). Índices e pesos das colunas são
    // calculados uma vez; o laço interno só faz somas.
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
    let gw = gw as usize;
    for y in 0..height {
        let fy = y as f32 * sy;
        let y0 = (fy.floor() as u32).min(gh - 1) as usize;
        let y1 = (y0 + 1).min(gh as usize - 1);
        let ty = fy - y0 as f32;
        let row0 = &grid[y0 * gw..(y0 + 1) * gw];
        let row1 = &grid[y1 * gw..(y1 + 1) * gw];
        let out = &mut pixels[(y as usize) * (width as usize) * 4..(y as usize + 1) * (width as usize) * 4];
        for (x, &(x0, x1, tx)) in cols.iter().enumerate() {
            let c00 = row0[x0];
            let c10 = row0[x1];
            let c01 = row1[x0];
            let c11 = row1[x1];
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
                MotionPreset::Ocean => (
                    (6.0 + v * 22.0) as u8,
                    (34.0 + v * 85.0) as u8,
                    (68.0 + v * 145.0) as u8,
                ),
                MotionPreset::WarmGold => (
                    (42.0 + v * 125.0) as u8,
                    (22.0 + v * 72.0) as u8,
                    (10.0 + v * 28.0) as u8,
                ),
                _ => (
                    (12.0 + v * 36.0) as u8,
                    (22.0 + v * 92.0 * (1.0 - ny * 0.4)) as u8,
                    (48.0 + v * 135.0) as u8,
                ),
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
        for i in 0..BOKEH_STYLES.len() {
            let p = MotionPreset::Bokeh(i);
            let a = generate_motion_frame(64, 36, 7.0, p);
            let b = generate_motion_frame(64, 36, 7.0 + MOTION_LOOP_SECS, p);
            assert_eq!(a, b, "bokeh {} não fecha o ciclo", BOKEH_STYLES[i].uri);
        }
        for i in 0..WAVE_STYLES.len() {
            let p = MotionPreset::Wave(i);
            let a = generate_motion_frame(64, 36, 7.0, p);
            let b = generate_motion_frame(64, 36, 7.0 + MOTION_LOOP_SECS, p);
            assert_eq!(a, b, "ondas {} não fecha o ciclo", WAVE_STYLES[i].uri);
        }
    }

    #[test]
    fn uris_resolve_to_their_style() {
        for (i, s) in BOKEH_STYLES.iter().enumerate() {
            assert_eq!(MotionPreset::from_uri(s.uri), MotionPreset::Bokeh(i));
        }
        for (i, s) in WAVE_STYLES.iter().enumerate() {
            assert_eq!(MotionPreset::from_uri(s.uri), MotionPreset::Wave(i));
        }
        assert_eq!(MotionPreset::from_uri("motion://ocean"), MotionPreset::Ocean);
    }

    #[test]
    fn new_styles_stay_dark_and_low_glow() {
        for s in BOKEH_STYLES {
            assert!(s.bottom.iter().cloned().fold(0.0f32, f32::max) <= 90.0, "{} muito clara", s.uri);
        }
        for s in WAVE_STYLES {
            assert!(s.strength <= 0.5, "{} com brilho alto", s.uri);
            let f = generate_motion_frame(96, 54, 3.0, MotionPreset::Wave(WAVE_STYLES.iter().position(|x| x.uri == s.uri).unwrap()));
            let mean: f32 = f.chunks(4).map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0).sum::<f32>() / (f.len() / 4) as f32;
            assert!(mean < 70.0, "{} média {} clara demais", s.uri, mean);
        }
    }
}

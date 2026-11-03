use crate::libmpv_ffi::{LibMpvPlayer, probe_libmpv_path};
use overture_core::MediaKind;
use overture_render::SharedVideoFrame;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub enum MediaCommand {
    /// Starts looping background video (muted) or procedural motion preset (`motion://aurora`, `motion://ocean`, `motion://warm`).
    PlayBackgroundVideo {
        path: String,
        loop_playback: bool,
    },
    /// Plays foreground media item (video with audio, or audio-only).
    PlayMedia {
        path: String,
        kind: MediaKind,
        loop_playback: bool,
    },
    SetPaused(bool),
    SetVolume(u8),
    Stop,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionPreset {
    Aurora,
    Ocean,
    WarmGold,
    /// Índice em `DARK_MOTIONS` (fundos escuros, em loop).
    Dark(usize),
    /// Índice em `BOKEH_STYLES` (partículas tipo bokeh, em loop).
    Bokeh(usize),
}

/// Fundo de partículas bokeh: discos suaves e faíscas que sobem devagar sobre um gradiente escuro.
pub struct BokehStyle {
    pub uri: &'static str,
    pub name: &'static str,
    /// Cor do topo e da base do gradiente.
    pub top: [f32; 3],
    pub bottom: [f32; 3],
    /// Cor do brilho das partículas.
    pub tint: [f32; 3],
    /// Quantidade de partículas.
    pub count: u32,
}

pub const BOKEH_STYLES: &[BokehStyle] = &[
    BokehStyle { uri: "motion://bokeh-teal", name: "Bokeh Azul-Petróleo", top: [12.0, 14.0, 16.0], bottom: [14.0, 62.0, 78.0], tint: [150.0, 210.0, 230.0], count: 170 },
    BokehStyle { uri: "motion://bokeh-graphite", name: "Bokeh Grafite", top: [12.0, 12.0, 13.0], bottom: [40.0, 42.0, 46.0], tint: [200.0, 205.0, 212.0], count: 140 },
    BokehStyle { uri: "motion://bokeh-violet", name: "Bokeh Violeta", top: [10.0, 8.0, 16.0], bottom: [48.0, 30.0, 78.0], tint: [200.0, 170.0, 235.0], count: 150 },
];

/// Fundo escuro animado, gerado do zero. Todo movimento tem período fixo de `MOTION_LOOP_SECS`,
/// então o ciclo fecha sem salto e o loop não aparece.
pub struct DarkMotion {
    pub uri: &'static str,
    pub name: &'static str,
    /// Cor de fundo (quase preto).
    pub base: [f32; 3],
    /// Cor de brilho suave que passa pela tela.
    pub accent: [f32; 3],
    /// Raio das manchas em fração da altura.
    pub radius: f32,
    /// Número de manchas (3 a 6).
    pub blobs: usize,
    /// Intensidade máxima do brilho (0..1).
    pub strength: f32,
}

pub const MOTION_LOOP_SECS: f32 = 24.0;

pub const DARK_MOTIONS: &[DarkMotion] = &[
    DarkMotion { uri: "motion://graphite", name: "Grafite", base: [14.0, 14.0, 16.0], accent: [72.0, 74.0, 80.0], radius: 0.42, blobs: 4, strength: 0.55 },
    DarkMotion { uri: "motion://charcoal-fog", name: "Névoa Carvão", base: [12.0, 12.0, 13.0], accent: [60.0, 62.0, 68.0], radius: 0.55, blobs: 3, strength: 0.50 },
    DarkMotion { uri: "motion://midnight", name: "Meia-Noite", base: [6.0, 9.0, 18.0], accent: [32.0, 54.0, 98.0], radius: 0.45, blobs: 4, strength: 0.60 },
    DarkMotion { uri: "motion://ember-dim", name: "Brasa Apagada", base: [12.0, 8.0, 8.0], accent: [112.0, 54.0, 30.0], radius: 0.30, blobs: 3, strength: 0.40 },
    DarkMotion { uri: "motion://violet-haze", name: "Violeta Escuro", base: [10.0, 7.0, 16.0], accent: [66.0, 42.0, 106.0], radius: 0.45, blobs: 4, strength: 0.55 },
    DarkMotion { uri: "motion://teal-deep", name: "Verde-Azulado Profundo", base: [6.0, 13.0, 14.0], accent: [26.0, 86.0, 90.0], radius: 0.42, blobs: 4, strength: 0.55 },
    DarkMotion { uri: "motion://dusk-rose", name: "Crepúsculo Rosé", base: [14.0, 9.0, 12.0], accent: [98.0, 48.0, 72.0], radius: 0.42, blobs: 3, strength: 0.50 },
    DarkMotion { uri: "motion://steel-blue", name: "Aço Azulado", base: [8.0, 10.0, 13.0], accent: [48.0, 72.0, 94.0], radius: 0.50, blobs: 3, strength: 0.50 },
    DarkMotion { uri: "motion://olive-dark", name: "Oliva Escuro", base: [10.0, 11.0, 7.0], accent: [64.0, 72.0, 38.0], radius: 0.45, blobs: 3, strength: 0.45 },
    DarkMotion { uri: "motion://ink-specks", name: "Tinta e Partículas", base: [7.0, 7.0, 8.0], accent: [90.0, 90.0, 98.0], radius: 0.22, blobs: 6, strength: 0.45 },
];

impl MotionPreset {
    pub fn from_uri(uri: &str) -> Self {
        let lower = uri.to_ascii_lowercase();
        if let Some(i) = DARK_MOTIONS.iter().position(|m| m.uri == lower) {
            return Self::Dark(i);
        }
        if let Some(i) = BOKEH_STYLES.iter().position(|m| m.uri == lower) {
            return Self::Bokeh(i);
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

/// Asynchronous media engine driving `libmpv` video/audio playback and procedural motion backgrounds,
/// publishing decoded RGBA frames directly to `OutputManager` and UI previews.
#[derive(Clone)]
pub struct MediaEngine {
    cmd_tx: Sender<MediaCommand>,
    frame_rx: watch::Receiver<Option<SharedVideoFrame>>,
    libmpv_available: Arc<AtomicBool>,
}

impl MediaEngine {
    pub fn spawn(render_width: u32, render_height: u32) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<MediaCommand>();
        let (frame_tx, frame_rx) = watch::channel::<Option<SharedVideoFrame>>(None);
        let libmpv_available = Arc::new(AtomicBool::new(probe_libmpv_path().is_some()));
        let mpv_flag = libmpv_available.clone();

        std::thread::Builder::new()
            .name("overture-media-engine".to_string())
            .spawn(move || {
                let mut mpv = match LibMpvPlayer::try_new() {
                    Ok(p) => {
                        mpv_flag.store(true, Ordering::Relaxed);
                        info!("libmpv initialized successfully for hardware/software media playback");
                        Some(p)
                    }
                    Err(e) => {
                        mpv_flag.store(false, Ordering::Relaxed);
                        info!("libmpv not loaded ({}); procedural motion engine active", e);
                        None
                    }
                };

                let w = render_width.max(64);
                let h = render_height.max(36);
                let mut active_mode: ActivePlayback = ActivePlayback::Idle;
                let mut paused = false;
                let start_instant = Instant::now();
                let frame_interval = Duration::from_millis(33); // ~30 fps smooth video background

                loop {
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            MediaCommand::PlayBackgroundVideo {
                                path,
                                loop_playback,
                            } => {
                                paused = false;
                                if path.starts_with("motion://") || mpv.is_none() {
                                    if let Some(p) = &mut mpv {
                                        p.stop();
                                    }
                                    active_mode = ActivePlayback::Procedural(
                                        MotionPreset::from_uri(&path),
                                    );
                                } else if let Some(p) = &mut mpv {
                                    match p.load_file(&path, loop_playback, true) {
                                        Ok(()) => active_mode = ActivePlayback::MpvVideo,
                                        Err(e) => {
                                            warn!("libmpv load_file error: {}; using motion fallback", e);
                                            active_mode = ActivePlayback::Procedural(
                                                MotionPreset::Aurora,
                                            );
                                        }
                                    }
                                }
                            }
                            MediaCommand::PlayMedia {
                                path,
                                kind,
                                loop_playback,
                            } => {
                                paused = false;
                                match kind {
                                    MediaKind::Video => {
                                        if let Some(p) = &mut mpv
                                            && !path.starts_with("motion://")
                                        {
                                            if p.load_file(&path, loop_playback, false).is_ok() {
                                                active_mode = ActivePlayback::MpvVideo;
                                            } else {
                                                active_mode = ActivePlayback::Procedural(
                                                    MotionPreset::from_uri(&path),
                                                );
                                            }
                                        } else {
                                            active_mode = ActivePlayback::Procedural(
                                                MotionPreset::from_uri(&path),
                                            );
                                        }
                                    }
                                    MediaKind::Audio => {
                                        if let Some(p) = &mut mpv {
                                            let _ = p.load_file(&path, loop_playback, false);
                                        }
                                        active_mode = ActivePlayback::AudioOnly;
                                        let _ = frame_tx.send(None);
                                    }
                                    MediaKind::Image => {
                                        active_mode = ActivePlayback::Idle;
                                        let _ = frame_tx.send(None);
                                    }
                                }
                            }
                            MediaCommand::SetPaused(p_state) => {
                                paused = p_state;
                                if let Some(p) = &mut mpv {
                                    p.set_paused(p_state);
                                }
                            }
                            MediaCommand::SetVolume(vol) => {
                                if let Some(p) = &mut mpv {
                                    p.set_volume(vol);
                                }
                            }
                            MediaCommand::Stop => {
                                active_mode = ActivePlayback::Idle;
                                if let Some(p) = &mut mpv {
                                    p.stop();
                                }
                                let _ = frame_tx.send(None);
                            }
                            MediaCommand::Shutdown => return,
                        }
                    }

                    if !paused {
                        let elapsed = start_instant.elapsed();
                        let pts_ms = elapsed.as_millis() as u64;
                        match active_mode {
                            ActivePlayback::Idle | ActivePlayback::AudioOnly => {}
                            ActivePlayback::MpvVideo => {
                                if let Some(p) = &mut mpv {
                                    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
                                    if p.render_rgba(w, h, &mut buf).is_ok() {
                                        let _ = frame_tx.send(Some(SharedVideoFrame {
                                            width: w,
                                            height: h,
                                            pixels: Arc::new(buf),
                                            pts_ms,
                                        }));
                                    }
                                }
                            }
                            ActivePlayback::Procedural(preset) => {
                                let buf = generate_motion_frame(w, h, elapsed.as_secs_f32(), preset);
                                let _ = frame_tx.send(Some(SharedVideoFrame {
                                    width: w,
                                    height: h,
                                    pixels: Arc::new(buf),
                                    pts_ms,
                                }));
                            }
                        }
                    }

                    std::thread::sleep(frame_interval);
                }
            })
            .expect("spawn media engine thread");

        Self {
            cmd_tx,
            frame_rx,
            libmpv_available,
        }
    }

    pub fn send(&self, cmd: MediaCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn subscribe_frames(&self) -> watch::Receiver<Option<SharedVideoFrame>> {
        self.frame_rx.clone()
    }

    pub fn is_libmpv_loaded(&self) -> bool {
        self.libmpv_available.load(Ordering::Relaxed)
    }

    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(MediaCommand::Shutdown);
    }
}

#[derive(Debug, Clone, Copy)]
enum ActivePlayback {
    Idle,
    AudioOnly,
    MpvVideo,
    Procedural(MotionPreset),
}

/// Generates a smooth, atmospheric RGBA worship motion-background frame at `(width, height)`.
pub fn generate_motion_frame(
    width: u32,
    height: u32,
    t_secs: f32,
    preset: MotionPreset,
) -> Vec<u8> {
    if let MotionPreset::Dark(i) = preset {
        return generate_dark_frame(width, height, t_secs, &DARK_MOTIONS[i.min(DARK_MOTIONS.len() - 1)]);
    }
    if let MotionPreset::Bokeh(i) = preset {
        return generate_bokeh_frame(width, height, t_secs, &BOKEH_STYLES[i.min(BOKEH_STYLES.len() - 1)]);
    }
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
            let v = ((wave_x + wave_y + diag) / 3.0) * 0.5 + 0.5; // 0.0 .. 1.0

            let (r, g, b) = match preset {
                MotionPreset::Dark(_) | MotionPreset::Bokeh(_) => (0, 0, 0),
                MotionPreset::Aurora => (
                    (12.0 + v * 36.0) as u8,
                    (22.0 + v * 92.0 * (1.0 - ny * 0.4)) as u8,
                    (48.0 + v * 135.0) as u8,
                ),
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

/// Frame de um fundo escuro em loop. Cada mancha percorre uma órbita elíptica cujo número de
/// voltas por ciclo é inteiro, então a posição no tempo `t + MOTION_LOOP_SECS` é igual à de `t`.
fn generate_dark_frame(width: u32, height: u32, t_secs: f32, m: &DarkMotion) -> Vec<u8> {
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    let phase = (t_secs.rem_euclid(MOTION_LOOP_SECS)) / MOTION_LOOP_SECS; // 0..1
    let tau = std::f32::consts::TAU;

    // Posições das manchas (coordenadas normalizadas pela altura, para não distorcer a forma).
    let mut blobs: [(f32, f32, f32); 6] = [(0.0, 0.0, 0.0); 6];
    for i in 0..m.blobs.min(6) {
        let k = i as f32;
        let kx = 1.0 + (i % 3) as f32; // voltas inteiras por ciclo
        let ky = 1.0 + ((i + 1) % 2) as f32;
        let ax = 0.30 + 0.06 * (k * 1.3).sin();
        let ay = 0.22 + 0.05 * (k * 0.9).cos();
        let cx = 0.5 + ax * (tau * phase * kx + k * 2.1).cos() * (w / h).min(2.0) * 0.5;
        let cy = 0.5 + ay * (tau * phase * ky + k * 1.4).sin();
        blobs[i] = (cx * w / h, cy, m.radius * (0.85 + 0.15 * (tau * phase + k).sin()));
    }

    let inv_2s2 = |r: f32| 1.0 / (2.0 * r * r);
    for y in 0..height {
        let ny = y as f32 / h;
        let row_offset = (y * width) as usize * 4;
        for x in 0..width {
            let nx = x as f32 / h;
            let mut f = 0.0f32;
            for &(cx, cy, r) in blobs.iter().take(m.blobs.min(6)) {
                let dx = nx - cx;
                let dy = ny - cy;
                f += (-(dx * dx + dy * dy) * inv_2s2(r)).exp();
            }
            let v = (f.min(1.0) * m.strength).clamp(0.0, 1.0);
            // Leve queda para o topo: a parte de cima fica mais escura que o centro.
            let vign = 0.70 + 0.30 * ny;
            let idx = row_offset + (x as usize * 4);
            for c in 0..3 {
                let col = m.base[c] + (m.accent[c] - m.base[c]) * v;
                pixels[idx + c] = (col * vign).clamp(0.0, 255.0) as u8;
            }
            pixels[idx + 3] = 255;
        }
    }
    pixels
}

/// Hash determinístico (0..1) para espalhar as partículas sem depender de aleatoriedade.
fn hash01(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B1) ^ salt.wrapping_mul(0x85EB_CA77);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0x00FF_FFFF) as f32 / 16_777_216.0
}

/// Frame bokeh em loop. Cada partícula sobe de forma linear por um número inteiro de alturas de tela
/// por ciclo e balança em seno com período do ciclo; assim o frame em `t + LOOP` é igual ao de `t`.
fn generate_bokeh_frame(width: u32, height: u32, t_secs: f32, style: &BokehStyle) -> Vec<u8> {
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    let phase = t_secs.rem_euclid(MOTION_LOOP_SECS) / MOTION_LOOP_SECS;
    let tau = std::f32::consts::TAU;

    // Acumulador de brilho das partículas.
    let mut acc = vec![0.0f32; (width as usize) * (height as usize) * 3];

    for i in 0..style.count {
        let bx = hash01(i, 1);
        let by = hash01(i, 2);
        let depth = hash01(i, 3); // 0 = longe (pequena, lenta), 1 = perto (grande, rápida)
        let sparkle = hash01(i, 4) < 0.18;
        let radius = if sparkle { 0.006 } else { 0.014 + depth * 0.040 } * h;
        let bright = if sparkle { 1.0 } else { 0.10 + depth * 0.24 };
        let speed = if !sparkle && depth > 0.6 { 2.0 } else { 1.0 }; // alturas de tela por ciclo (inteiro)
        let sway_amp = 0.012 + 0.02 * hash01(i, 5);
        let sway_ph = tau * hash01(i, 6);

        let cx = (bx + sway_amp * (tau * phase + sway_ph).sin()).rem_euclid(1.0) * w;
        let cy0 = (by - phase * speed).rem_euclid(1.0) * h;

        // Desenha cópias em y-h, y, y+h para o disco atravessar a borda sem cortar.
        for k in -1..=1 {
            let cy = cy0 + k as f32 * h;
            let r = radius;
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
                    if d >= 1.0 {
                        continue;
                    }
                    // Disco com borda levemente mais clara (bokeh) e centro suave.
                    let edge = ((d - 0.85) / 0.15).clamp(0.0, 1.0);
                    let v = bright * (1.0 - d * d) * (0.75 + 0.5 * edge);
                    let idx = ((py * width + px) * 3) as usize;
                    acc[idx] += v;
                    acc[idx + 1] += v;
                    acc[idx + 2] += v;
                }
            }
        }
    }

    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    for y in 0..height {
        let ny = y as f32 / h;
        let grad = ((ny - 0.15) / 0.85).clamp(0.0, 1.0);
        let grad = grad * grad * (3.0 - 2.0 * grad); // smoothstep
        for x in 0..width {
            let p = (y * width + x) as usize;
            let a = acc[p * 3];
            let idx = p * 4;
            for c in 0..3 {
                let base = style.top[c] + (style.bottom[c] - style.top[c]) * grad;
                let v = base + style.tint[c] * a.min(1.0);
                pixels[idx + c] = v.clamp(0.0, 255.0) as u8;
            }
            pixels[idx + 3] = 255;
        }
    }
    pixels
}

#[cfg(test)]
mod motion_loop_tests {
    use super::*;

    #[test]
    fn dark_motions_loop_seamlessly() {
        for i in 0..DARK_MOTIONS.len() {
            let p = MotionPreset::Dark(i);
            let a = generate_motion_frame(64, 36, 5.0, p);
            let b = generate_motion_frame(64, 36, 5.0 + MOTION_LOOP_SECS, p);
            assert_eq!(a, b, "preset {} não fecha o ciclo", DARK_MOTIONS[i].uri);
        }
    }

    #[test]
    fn dark_motions_resolve_by_uri() {
        for (i, m) in DARK_MOTIONS.iter().enumerate() {
            assert!(matches!(MotionPreset::from_uri(m.uri), MotionPreset::Dark(j) if j == i));
        }
    }

    #[test]
    fn bokeh_styles_loop_seamlessly() {
        for i in 0..BOKEH_STYLES.len() {
            let p = MotionPreset::Bokeh(i);
            let a = generate_motion_frame(64, 36, 7.0, p);
            let b = generate_motion_frame(64, 36, 7.0 + MOTION_LOOP_SECS, p);
            assert_eq!(a, b, "bokeh {} não fecha o ciclo", BOKEH_STYLES[i].uri);
        }
        for (i, b) in BOKEH_STYLES.iter().enumerate() {
            assert!(matches!(MotionPreset::from_uri(b.uri), MotionPreset::Bokeh(j) if j == i));
        }
    }

    #[test]
    fn dark_motions_stay_dark() {
        for m in DARK_MOTIONS {
            let max_base = m.base.iter().cloned().fold(0.0f32, f32::max);
            assert!(max_base <= 20.0, "{} base não é quase preta", m.uri);
            let f = generate_motion_frame(96, 54, 3.0, MotionPreset::Dark(DARK_MOTIONS.iter().position(|x| x.uri == m.uri).unwrap()));
            let mean: f32 = f.chunks(4).map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0).sum::<f32>() / (f.len() / 4) as f32;
            assert!(mean < 90.0, "{} média {} muito clara", m.uri, mean);
        }
    }
}

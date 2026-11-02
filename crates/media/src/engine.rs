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
}

impl MotionPreset {
    pub fn from_uri(uri: &str) -> Self {
        let lower = uri.to_ascii_lowercase();
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

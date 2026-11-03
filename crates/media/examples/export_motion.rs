//! Exporta os fundos de letra (`motion://...`) para vídeo em loop, 1920×1080 a 30 fps, via ffmpeg.
//!
//! Uso:
//!   cargo run --release -p overture-media --example export_motion -- <pasta_saida> [filtro]
//!
//! Requer `ffmpeg` com libx264 no PATH. Gera `<pasta_saida>/<nome>.mp4` para cada estilo.

use overture_media::{MOTION_FPS, MOTION_LOOP_SECS, MotionPreset, STYLES, generate_motion_frame};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const W: u32 = 1920;
const H: u32 = 1080;
const CRF: &str = "22";

fn main() -> anyhow_like::Result {
    let mut args = std::env::args().skip(1);
    let out_dir = PathBuf::from(args.next().ok_or("uso: export_motion <pasta_saida> [filtro]")?);
    let filter = args.next();
    std::fs::create_dir_all(&out_dir)?;

    let frames = (MOTION_LOOP_SECS * MOTION_FPS as f32).round() as u32;
    for (i, style) in STYLES.iter().enumerate() {
        let name = style.uri.trim_start_matches("motion://");
        if let Some(f) = &filter
            && !name.contains(f.as_str())
        {
            continue;
        }
        let out = out_dir.join(format!("{name}.mp4"));
        println!("[{:02}/{}] {} -> {}", i + 1, STYLES.len(), style.name, out.display());

        let mut child = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s"])
            .arg(format!("{W}x{H}"))
            .args(["-r", &MOTION_FPS.to_string(), "-i", "-", "-c:v", "libx264", "-preset", "slow", "-crf", CRF])
            .args(["-pix_fmt", "yuv420p", "-movflags", "+faststart"])
            .arg(&out)
            .stdin(Stdio::piped())
            .spawn()?;
        {
            let stdin = child.stdin.as_mut().ok_or("ffmpeg sem stdin")?;
            for f in 0..frames {
                let t = f as f32 / MOTION_FPS as f32;
                let rgba = generate_motion_frame(W, H, t, MotionPreset::Style(i));
                let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
                stdin.write_all(&rgb)?;
            }
        }
        let status = child.wait()?;
        if !status.success() {
            return Err(format!("ffmpeg falhou em {name}").into());
        }
    }
    println!("pronto");
    Ok(())
}

mod anyhow_like {
    pub type Result = std::result::Result<(), Box<dyn std::error::Error>>;
}

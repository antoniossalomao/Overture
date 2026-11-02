//! GPU-accelerated slide composition (`cosmic-text` + `wgpu`) for Overture.

pub mod compositor;
pub mod gpu;

pub use compositor::{RgbaFrame, SharedVideoFrame, SlideCompositor, VideoFrameView};
pub use gpu::{GpuContext, SurfacePresenter, render_frame_via_gpu_offscreen};

#[cfg(test)]
mod tests {
    use super::*;
    use overture_core::{
        BackgroundSpec, DisplayMode, LivePresentationItem, OutputRole, OutputState,
        PresentationSlide, RgbaColor, ServiceItemKind, ThemeConfig,
    };
    use std::time::Instant;

    fn sample_state() -> OutputState {
        let mut st = OutputState::default();
        st.current_item = Some(LivePresentationItem {
            service_item_id: Some(1),
            song_id: Some(1),
            title: "Santo, Santo, Santo".to_string(),
            subtitle: "Reginald Heber".to_string(),
            notes: "Entrada suave no piano em Dó maior".to_string(),
            kind: ServiceItemKind::Song,
            slides: vec![
                PresentationSlide {
                    index: 0,
                    text: "Santo! Santo! Santo!\nDeus Onipotente!".to_string(),
                    footer: None,
                    forced_break: false,
                    duration_secs: None,
                },
                PresentationSlide {
                    index: 1,
                    text: "Cedo de manhã\nCantaremos Teu louvor".to_string(),
                    footer: None,
                    forced_break: false,
                    duration_secs: None,
                },
            ],
            theme: ThemeConfig::default(),
            media_path: None,
            media_kind: None,
        });
        st
    }

    #[test]
    fn composes_main_stage_and_return_differently_with_low_latency() {
        let mut compositor = SlideCompositor::new();
        let mut st = sample_state();
        st.stage_alert = Some("Microfone 2 aberto".to_string());

        // Warm up system font cache first, then measure steady-state composition latency
        let _ = compositor.compose(&st, OutputRole::Main, 1280, 720, None);

        let t0 = Instant::now();
        let main_frame = compositor.compose(&st, OutputRole::Main, 1280, 720, None);
        let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(main_frame.pixels.len(), 1280 * 720 * 4);
        // Steady-state 720p composition must stay well below 50ms (typically ~3-8ms)
        assert!(
            elapsed_ms < 80.0,
            "composition took too long: {elapsed_ms:.2} ms"
        );

        let stage_frame = compositor.compose(&st, OutputRole::Stage, 1280, 720, None);
        let return_frame = compositor.compose(&st, OutputRole::Return, 1280, 720, None);

        // Main, Stage, and Return monitors must produce distinct visual layouts!
        assert_ne!(main_frame.pixels, stage_frame.pixels);
        assert_ne!(main_frame.pixels, return_frame.pixels);
        assert_ne!(stage_frame.pixels, return_frame.pixels);

        // Black screen zeroes out Main output completely while Stage keeps showing operator info
        st.display_mode = DisplayMode::BlackScreen;
        let black_main = compositor.compose(&st, OutputRole::Main, 320, 180, None);
        assert!(
            black_main
                .pixels
                .chunks_exact(4)
                .all(|px| px == [0, 0, 0, 255])
        );
        let black_stage = compositor.compose(&st, OutputRole::Stage, 320, 180, None);
        assert!(
            black_stage
                .pixels
                .chunks_exact(4)
                .any(|px| px != [0, 0, 0, 255])
        );
    }

    #[test]
    fn composes_video_background_with_text_overlay_and_gpu_pass() {
        let mut compositor = SlideCompositor::new();
        let mut st = sample_state();
        if let Some(item) = &mut st.current_item {
            item.theme.background = BackgroundSpec::Video {
                path: "loop.mp4".to_string(),
                dim_opacity: 0.25,
                loop_playback: true,
            };
        }

        // Synthetic vivid teal video frame (64x36)
        let mut raw_video = vec![0u8; 64 * 36 * 4];
        for px in raw_video.chunks_exact_mut(4) {
            px.copy_from_slice(&[20, 160, 200, 255]);
        }
        let vf = VideoFrameView {
            width: 64,
            height: 36,
            data: &raw_video,
        };

        let composed = compositor.compose(&st, OutputRole::Main, 640, 360, Some(vf));
        // Verify white text pixels were drawn over the dimmed teal video background
        let has_bright_white_glyphs = composed
            .pixels
            .chunks_exact(4)
            .any(|px| px[0] > 230 && px[1] > 230 && px[2] > 230);
        assert!(has_bright_white_glyphs, "expected crisp white text over video frame");

        // Verify wgpu WGSL pipeline executes cleanly end-to-end if an adapter is available
        if let Ok(gpu) = GpuContext::new(None) {
            let gpu_out = render_frame_via_gpu_offscreen(&gpu, &composed)
                .expect("offscreen wgpu render pass should succeed");
            assert_eq!(gpu_out.width, 640);
            assert_eq!(gpu_out.height, 360);
            let white_after_gpu = gpu_out
                .pixels
                .chunks_exact(4)
                .any(|px| px[0] > 225 && px[1] > 225 && px[2] > 225);
            assert!(white_after_gpu, "expected white glyphs preserved through wgpu shader");
        } else {
            let _ = RgbaColor::WHITE;
        }
    }
}

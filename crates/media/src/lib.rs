//! Video background, standalone video, and audio playback engine via `libmpv` + procedural motion presets.

pub mod engine;
pub mod libmpv_ffi;

pub use engine::{MediaCommand, MediaEngine, MotionPreset, generate_motion_frame};
pub use libmpv_ffi::{LibMpvPlayer, probe_libmpv_path};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn streams_motion_video_background_frames_smoothly() {
        let media = MediaEngine::spawn(160, 90);
        let mut rx = media.subscribe_frames();

        media.send(MediaCommand::PlayBackgroundVideo {
            path: "motion://aurora".to_string(),
            loop_playback: true,
        });

        tokio::time::timeout(Duration::from_secs(2), rx.changed())
            .await
            .expect("timed out waiting for first video frame")
            .expect("watch channel open");

        let frame1 = rx.borrow().clone().expect("expected video frame");
        assert_eq!(frame1.width, 160);
        assert_eq!(frame1.height, 90);
        assert_eq!(frame1.pixels.len(), 160 * 90 * 4);

        // Verify next frame advances PTS and animates pixels smoothly
        tokio::time::timeout(Duration::from_secs(2), rx.changed())
            .await
            .expect("timed out waiting for second video frame")
            .expect("watch channel open");
        let frame2 = rx.borrow().clone().expect("expected second video frame");
        assert!(frame2.pts_ms > frame1.pts_ms);
        assert_ne!(*frame1.pixels, *frame2.pixels);

        media.send(MediaCommand::Stop);
        media.shutdown();
    }
}

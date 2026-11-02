//! Multi-monitor fullscreen window management via Win32, `winit`, and `wgpu`.

pub mod manager;
pub mod win32;

pub use manager::{LatencyTelemetry, OutputLoopMessage, OutputManager, RoleWindowTarget};
pub use win32::{
    OVERTURE_APP_USER_MODEL_ID, apply_win32_fullscreen, enumerate_monitors,
    init_windows_process_identity,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_at_least_one_monitor_or_headless_list_safely() {
        init_windows_process_identity(OVERTURE_APP_USER_MODEL_ID);
        let monitors = enumerate_monitors();
        // On desktop Windows sessions there is >= 1 monitor; in headless session 0 it must not panic
        for m in &monitors {
            assert!(m.width > 0);
            assert!(m.height > 0);
            assert!(!m.friendly_name.is_empty());
        }
    }
}

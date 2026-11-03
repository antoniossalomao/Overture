use crate::win32::{apply_win32_fullscreen, enumerate_monitors};
use anyhow::{Context, Result};
use overture_core::{MonitorDescriptor, OutputRole, OutputState, TransitionKind, now_epoch_us};
use overture_render::{GpuContext, RgbaFrame, SharedVideoFrame, SlideCompositor, SurfacePresenter};
use std::time::Instant;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use tokio::sync::watch;
use tracing::{error, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

#[cfg(target_os = "windows")]
use winit::platform::windows::EventLoopBuilderExtWindows;

/// Real-time latency telemetry from `StateEngine::dispatch` to `wgpu` swapchain `present()`.
#[derive(Debug, Default)]
pub struct LatencyTelemetry {
    last_latency_us: AtomicU64,
    max_latency_us: AtomicU64,
    frames_presented: AtomicU64,
}

impl LatencyTelemetry {
    pub fn record(&self, latency_us: u64) {
        self.last_latency_us.store(latency_us, Ordering::Relaxed);
        self.max_latency_us.fetch_max(latency_us, Ordering::Relaxed);
        self.frames_presented.fetch_add(1, Ordering::Relaxed);
    }

    pub fn last_ms(&self) -> f64 {
        self.last_latency_us.load(Ordering::Relaxed) as f64 / 1000.0
    }

    pub fn max_ms(&self) -> f64 {
        self.max_latency_us.load(Ordering::Relaxed) as f64 / 1000.0
    }

    pub fn frames_presented(&self) -> u64 {
        self.frames_presented.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone)]
pub struct RoleWindowTarget {
    pub role: OutputRole,
    pub monitor_index: usize,
    pub fullscreen: bool,
    pub topmost: bool,
}

#[derive(Debug, Clone)]
pub enum OutputLoopMessage {
    ConfigureWindow(RoleWindowTarget),
    CloseWindow(OutputRole),
    RedrawState,
    RedrawVideo,
    Shutdown,
}

struct ManagedWindow {
    role: OutputRole,
    window: Arc<Window>,
    presenter: SurfacePresenter,
    last_rendered_revision: u64,
    /// Último quadro publicado (base para o cross-fade).
    last_frame: Option<RgbaFrame>,
    /// Chave do slide exibido, para detectar troca de slide.
    last_slide_key: Option<String>,
    /// Cross-fade em andamento: início, duração e quadro anterior.
    fade: Option<(Instant, u32, RgbaFrame)>,
}

struct OutputAppHandler {
    state_rx: watch::Receiver<OutputState>,
    video_rx: Option<watch::Receiver<Option<SharedVideoFrame>>>,
    telemetry: Arc<LatencyTelemetry>,
    gpu: Option<GpuContext>,
    compositor: SlideCompositor,
    by_role: HashMap<OutputRole, WindowId>,
    windows: HashMap<WindowId, ManagedWindow>,
    pending_targets: Vec<RoleWindowTarget>,
}

impl OutputAppHandler {
    fn new(
        state_rx: watch::Receiver<OutputState>,
        video_rx: Option<watch::Receiver<Option<SharedVideoFrame>>>,
        telemetry: Arc<LatencyTelemetry>,
        initial_targets: Vec<RoleWindowTarget>,
    ) -> Self {
        Self {
            state_rx,
            video_rx,
            telemetry,
            gpu: None,
            compositor: SlideCompositor::new(),
            by_role: HashMap::new(),
            windows: HashMap::new(),
            pending_targets: initial_targets,
        }
    }

    fn resolve_monitor(index: usize) -> MonitorDescriptor {
        let monitors = enumerate_monitors();
        monitors
            .get(index)
            .cloned()
            .or_else(|| monitors.into_iter().next())
            .unwrap_or(MonitorDescriptor {
                index: 0,
                system_name: "DISPLAY1".to_string(),
                friendly_name: "Monitor Principal".to_string(),
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
                scale_factor: 1.0,
                is_primary: true,
            })
    }

    fn open_or_move_window(&mut self, event_loop: &ActiveEventLoop, target: RoleWindowTarget) {
        if let Some(old_id) = self.by_role.remove(&target.role) {
            self.windows.remove(&old_id);
        }

        let mon = Self::resolve_monitor(target.monitor_index);
        let title = match target.role {
            OutputRole::Main => "Overture — Saída Principal",
            OutputRole::Stage => "Overture — Monitor de Palco",
            OutputRole::Return => "Overture — Monitor de Retorno",
        };

        let (win_w, win_h, win_x, win_y) = if target.fullscreen {
            (mon.width, mon.height, mon.x, mon.y)
        } else {
            let w = (mon.width * 2 / 3).max(640);
            let h = (mon.height * 2 / 3).max(360);
            let x = mon.x + ((mon.width - w) / 2) as i32;
            let y = mon.y + ((mon.height - h) / 2) as i32;
            (w, h, x, y)
        };

        let attrs = WindowAttributes::default()
            .with_title(title)
            .with_decorations(!target.fullscreen)
            .with_inner_size(PhysicalSize::new(win_w, win_h))
            .with_position(PhysicalPosition::new(win_x, win_y));

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                error!("Failed to create output window {:?}: {}", target.role, e);
                return;
            }
        };

        if target.fullscreen
            && let Ok(wh) = window.window_handle()
            && let RawWindowHandle::Win32(h) = wh.as_raw()
        {
            apply_win32_fullscreen(h.hwnd.get(), &mon, target.topmost);
        }

        // Initialize GpuContext against the first real window surface if not yet initialized
        if self.gpu.is_none() {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
                flags: wgpu::InstanceFlags::empty(),
                ..Default::default()
            });
            let temp_surface = match instance.create_surface(window.clone()) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to create initial wgpu surface: {}", e);
                    return;
                }
            };
            match GpuContext::from_instance(instance, Some(&temp_surface)) {
                Ok(ctx) => {
                    let size = window.inner_size();
                    match SurfacePresenter::new(
                        &ctx,
                        temp_surface,
                        size.width.max(1),
                        size.height.max(1),
                    ) {
                        Ok(presenter) => {
                            let id = window.id();
                            self.by_role.insert(target.role, id);
                            self.windows.insert(
                                id,
                                ManagedWindow {
                                    role: target.role,
                                    window,
                                    presenter,
                                    last_rendered_revision: 0,
                                    last_frame: None,
                                    last_slide_key: None,
                                    fade: None,
                                },
                            );
                            self.gpu = Some(ctx);
                            info!(
                                "Opened {:?} output on monitor {} ({}x{})",
                                target.role, mon.friendly_name, mon.width, mon.height
                            );
                            self.render_window_by_id(id);
                            return;
                        }
                        Err(e) => {
                            error!("Failed to configure surface presenter: {}", e);
                            return;
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to initialize GpuContext: {}", e);
                    return;
                }
            }
        }

        let Some(gpu) = &self.gpu else {
            return;
        };
        let surface = match gpu.instance.create_surface(window.clone()) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to create wgpu surface for {:?}: {}", target.role, e);
                return;
            }
        };
        let size = window.inner_size();
        match SurfacePresenter::new(gpu, surface, size.width.max(1), size.height.max(1)) {
            Ok(presenter) => {
                let id = window.id();
                self.by_role.insert(target.role, id);
                self.windows.insert(
                    id,
                    ManagedWindow {
                        role: target.role,
                        window,
                        presenter,
                        last_rendered_revision: 0,
                        last_frame: None,
                        last_slide_key: None,
                        fade: None,
                    },
                );
                info!(
                    "Opened {:?} output on monitor {} ({}x{})",
                    target.role, mon.friendly_name, mon.width, mon.height
                );
                self.render_window_by_id(id);
            }
            Err(e) => error!("SurfacePresenter init failed for {:?}: {}", target.role, e),
        }
    }

    fn render_all_windows(&mut self) {
        let ids: Vec<WindowId> = self.windows.keys().copied().collect();
        for id in ids {
            self.render_window_by_id(id);
        }
    }

    fn render_window_by_id(&mut self, id: WindowId) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        let Some(win) = self.windows.get_mut(&id) else {
            return;
        };
        let size = win.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        win.presenter.resize(gpu, size.width, size.height);

        let state = self.state_rx.borrow().clone();
        let vf_shared = self
            .video_rx
            .as_ref()
            .and_then(|rx| rx.borrow().clone());
        let vf_view = vf_shared.as_ref().map(|v| v.as_view());

        let mut frame = self
            .compositor
            .compose(&state, win.role, size.width, size.height, vf_view);

        // Cross-fade: ao trocar de slide, mistura o quadro anterior com o novo pela duração do tema.
        let slide_key = format!(
            "{:?}|{}|{}",
            state.display_mode,
            state.current_slide_index,
            state.current_slide().map(|s| s.text.as_str()).unwrap_or("")
        );
        if win.last_slide_key.as_deref() != Some(slide_key.as_str()) {
            if let (Some(prev), TransitionKind::Fade { duration_ms }) =
                (win.last_frame.take(), state.main_theme.transition)
            {
                if prev.width == frame.width && prev.height == frame.height && duration_ms > 0 {
                    win.fade = Some((Instant::now(), duration_ms, prev));
                }
            }
            win.last_slide_key = Some(slide_key);
        }
        if let Some((start, dur_ms, prev)) = &win.fade {
            let t = (start.elapsed().as_millis() as f32 / *dur_ms as f32).clamp(0.0, 1.0);
            let k = (t * 255.0) as u32;
            let inv = 255 - k;
            for (dst, p) in frame.pixels.iter_mut().zip(prev.pixels.iter()) {
                *dst = ((*p as u32 * inv + *dst as u32 * k) / 255) as u8;
            }
            if t >= 1.0 {
                win.fade = None;
            } else {
                win.window.request_redraw();
            }
        }
        win.last_frame = Some(frame.clone());

        if let Err(e) = win.presenter.present(gpu, &frame) {
            warn!("Frame present warning on {:?}: {}", win.role, e);
        } else if win.last_rendered_revision != state.revision {
            win.last_rendered_revision = state.revision;
            let latency_us = now_epoch_us().saturating_sub(state.dispatched_at_epoch_us);
            self.telemetry.record(latency_us);
        }
    }
}

impl ApplicationHandler<OutputLoopMessage> for OutputAppHandler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
        let targets = std::mem::take(&mut self.pending_targets);
        for t in targets {
            self.open_or_move_window(event_loop, t);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: OutputLoopMessage) {
        match event {
            OutputLoopMessage::ConfigureWindow(target) => {
                self.open_or_move_window(event_loop, target);
            }
            OutputLoopMessage::CloseWindow(role) => {
                if let Some(id) = self.by_role.remove(&role) {
                    self.windows.remove(&id);
                }
            }
            OutputLoopMessage::RedrawState | OutputLoopMessage::RedrawVideo => {
                self.render_all_windows();
            }
            OutputLoopMessage::Shutdown => {
                self.windows.clear();
                self.by_role.clear();
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(removed) = self.windows.remove(&window_id) {
                    self.by_role.remove(&removed.role);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                // Emergency Escape closes the focused projector window so operator is never trapped
                if let Some(removed) = self.windows.remove(&window_id) {
                    self.by_role.remove(&removed.role);
                }
            }
            WindowEvent::Resized(new_size) => {
                if let (Some(gpu), Some(win)) = (&self.gpu, self.windows.get_mut(&window_id)) {
                    win.presenter.resize(gpu, new_size.width, new_size.height);
                }
                self.render_window_by_id(window_id);
            }
            WindowEvent::RedrawRequested => {
                self.render_window_by_id(window_id);
            }
            _ => {}
        }
    }
}

/// Thread-safe handle managing the dedicated multi-monitor output event loop.
pub struct OutputManager {
    proxy: EventLoopProxy<OutputLoopMessage>,
    telemetry: Arc<LatencyTelemetry>,
    join_handle: Option<JoinHandle<()>>,
}

impl OutputManager {
    pub fn spawn(
        state_rx: watch::Receiver<OutputState>,
        video_rx: Option<watch::Receiver<Option<SharedVideoFrame>>>,
        initial_targets: Vec<RoleWindowTarget>,
    ) -> Result<Self> {
        let telemetry = Arc::new(LatencyTelemetry::default());
        let telemetry_clone = telemetry.clone();
        let state_rx_for_watcher = state_rx.clone();
        let video_rx_for_watcher = video_rx.clone();

        let (proxy_tx, proxy_rx) = mpsc::channel::<Result<EventLoopProxy<OutputLoopMessage>>>();

        let join_handle = std::thread::Builder::new()
            .name("overture-output-loop".to_string())
            .spawn(move || {
                let mut builder = EventLoop::<OutputLoopMessage>::with_user_event();
                #[cfg(target_os = "windows")]
                builder.with_any_thread(true);

                let event_loop = match builder.build() {
                    Ok(el) => el,
                    Err(e) => {
                        let _ = proxy_tx
                            .send(Err(anyhow::anyhow!("Failed to build output EventLoop: {e}")));
                        return;
                    }
                };

                let proxy = event_loop.create_proxy();
                let _ = proxy_tx.send(Ok(proxy));

                let mut app = OutputAppHandler::new(
                    state_rx,
                    video_rx,
                    telemetry_clone,
                    initial_targets,
                );
                if let Err(e) = event_loop.run_app(&mut app) {
                    error!("Output EventLoop exited with error: {}", e);
                }
            })
            .context("Failed to spawn output manager thread")?;

        let proxy = proxy_rx
            .recv()
            .context("Output loop thread terminated before sending proxy")??;

        // Spawn lightweight watcher threads forwarding state and video updates immediately
        let proxy_state = proxy.clone();
        std::thread::Builder::new()
            .name("overture-state-watcher".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("watcher runtime");
                let mut rx = state_rx_for_watcher;
                rt.block_on(async move {
                    while rx.changed().await.is_ok() {
                        if proxy_state
                            .send_event(OutputLoopMessage::RedrawState)
                            .is_err()
                        {
                            break;
                        }
                    }
                });
            })?;

        if let Some(mut v_rx) = video_rx_for_watcher {
            let proxy_video = proxy.clone();
            std::thread::Builder::new()
                .name("overture-video-watcher".to_string())
                .spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("video watcher runtime");
                    rt.block_on(async move {
                        while v_rx.changed().await.is_ok() {
                            if proxy_video
                                .send_event(OutputLoopMessage::RedrawVideo)
                                .is_err()
                            {
                                break;
                            }
                        }
                    });
                })?;
        }

        Ok(Self {
            proxy,
            telemetry,
            join_handle: Some(join_handle),
        })
    }

    pub fn open_window(&self, target: RoleWindowTarget) -> Result<()> {
        self.proxy
            .send_event(OutputLoopMessage::ConfigureWindow(target))
            .map_err(|_| anyhow::anyhow!("Output loop is no longer running"))
    }

    pub fn close_window(&self, role: OutputRole) -> Result<()> {
        self.proxy
            .send_event(OutputLoopMessage::CloseWindow(role))
            .map_err(|_| anyhow::anyhow!("Output loop is no longer running"))
    }

    pub fn telemetry(&self) -> &Arc<LatencyTelemetry> {
        &self.telemetry
    }

    pub fn shutdown(mut self) {
        let _ = self.proxy.send_event(OutputLoopMessage::Shutdown);
        if let Some(h) = self.join_handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for OutputManager {
    fn drop(&mut self) {
        let _ = self.proxy.send_event(OutputLoopMessage::Shutdown);
    }
}

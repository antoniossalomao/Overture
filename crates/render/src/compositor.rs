use chrono::Local;
use cosmic_text::{Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight};
use image::RgbaImage;
use overture_core::{
    BackgroundSpec, CountdownState, DisplayMode, HorizontalAlign, OutputRole, OutputState,
    RgbaColor, ServiceItemKind, ShadowSpec, ThemeConfig, VerticalAlign,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Thread-safe shared RGBA video frame published by `overture-media` to output renderers and UI previews.
#[derive(Debug, Clone)]
pub struct SharedVideoFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<Vec<u8>>,
    pub pts_ms: u64,
}

impl SharedVideoFrame {
    pub fn as_view(&self) -> VideoFrameView<'_> {
        VideoFrameView {
            width: self.width,
            height: self.height,
            data: &self.pixels,
        }
    }
}

/// Borrowed RGBA video frame supplied by `overture-media` (`libmpv` or procedural generator).
#[derive(Debug, Clone, Copy)]
pub struct VideoFrameView<'a> {
    pub width: u32,
    pub height: u32,
    pub data: &'a [u8],
}

/// Composed RGBA8 pixel buffer ready for GPU upload or UI preview.
#[derive(Debug, Clone)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaFrame {
    pub fn new(width: u32, height: u32, fill: RgbaColor) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let total_px = (width as usize) * (height as usize);
        let mut pixels = vec![0u8; total_px * 4];
        let packed = u32::from_ne_bytes([fill.r, fill.g, fill.b, fill.a]);
        if packed != 0 {
            let words: &mut [u32] = bytemuck::cast_slice_mut(&mut pixels);
            words.fill(packed);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    pub fn new_uninit_zero(width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            pixels: vec![0u8; (width as usize) * (height as usize) * 4],
        }
    }

    #[inline]
    pub fn blend_pixel(&mut self, x: i32, y: i32, src: RgbaColor) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || src.a == 0 {
            return;
        }
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        if src.a == 255 {
            self.pixels[idx] = src.r;
            self.pixels[idx + 1] = src.g;
            self.pixels[idx + 2] = src.b;
            self.pixels[idx + 3] = 255;
            return;
        }
        let alpha = src.a as u32;
        let inv = 255 - alpha;
        self.pixels[idx] = ((src.r as u32 * alpha + self.pixels[idx] as u32 * inv) / 255) as u8;
        self.pixels[idx + 1] =
            ((src.g as u32 * alpha + self.pixels[idx + 1] as u32 * inv) / 255) as u8;
        self.pixels[idx + 2] =
            ((src.b as u32 * alpha + self.pixels[idx + 2] as u32 * inv) / 255) as u8;
        self.pixels[idx + 3] = 255;
    }

    pub fn fill_rect(&mut self, rx: i32, ry: i32, rw: u32, rh: u32, color: RgbaColor) {
        if color.a == 0 || rw == 0 || rh == 0 {
            return;
        }
        let x0 = rx.max(0) as u32;
        let y0 = ry.max(0) as u32;
        let x1 = (rx.saturating_add(rw as i32)).min(self.width as i32).max(0) as u32;
        let y1 = (ry.saturating_add(rh as i32)).min(self.height as i32).max(0) as u32;
        if x0 >= x1 || y0 >= y1 {
            return;
        }

        if color.a == 255 {
            let pat = [color.r, color.g, color.b, 255];
            for y in y0..y1 {
                let row_start = ((y * self.width + x0) as usize) * 4;
                let row_end = ((y * self.width + x1) as usize) * 4;
                for px in self.pixels[row_start..row_end].chunks_exact_mut(4) {
                    px.copy_from_slice(&pat);
                }
            }
        } else {
            for y in y0..y1 {
                for x in x0..x1 {
                    self.blend_pixel(x as i32, y as i32, color);
                }
            }
        }
    }

    pub fn fill_vertical_gradient(&mut self, top: RgbaColor, bottom: RgbaColor) {
        let h_max = (self.height.saturating_sub(1)).max(1);
        let w = self.width as usize;
        let words: &mut [u32] = bytemuck::cast_slice_mut(&mut self.pixels);
        for y in 0..self.height {
            let t = y as u32;
            let inv = h_max - t;
            let r = ((top.r as u32 * inv + bottom.r as u32 * t) / h_max) as u8;
            let g = ((top.g as u32 * inv + bottom.g as u32 * t) / h_max) as u8;
            let b = ((top.b as u32 * inv + bottom.b as u32 * t) / h_max) as u8;
            let packed = u32::from_ne_bytes([r, g, b, 255]);
            let row_start = (y as usize) * w;
            words[row_start..row_start + w].fill(packed);
        }
    }

    pub fn blit_scaled_cover(&mut self, src_w: u32, src_h: u32, src_rgba: &[u8], dim_opacity: f32) {
        if src_w == 0 || src_h == 0 || src_rgba.len() < (src_w as usize * src_h as usize * 4) {
            return;
        }
        // Aspect-preserving cover crop
        let dst_aspect = self.width as f32 / self.height as f32;
        let src_aspect = src_w as f32 / src_h as f32;
        let (crop_w, crop_h, off_x, off_y) = if src_aspect > dst_aspect {
            let cw = (src_h as f32 * dst_aspect).round().max(1.0) as u32;
            let ox = (src_w.saturating_sub(cw)) / 2;
            (cw.min(src_w), src_h, ox, 0)
        } else {
            let ch = (src_w as f32 / dst_aspect).round().max(1.0) as u32;
            let oy = (src_h.saturating_sub(ch)) / 2;
            (src_w, ch.min(src_h), 0, oy)
        };

        let dim_factor = ((1.0 - dim_opacity.clamp(0.0, 0.95)) * 256.0) as u32;
        for dy in 0..self.height {
            let sy = (off_y + (dy * crop_h) / self.height).min(src_h - 1);
            let dst_row = (dy * self.width) as usize * 4;
            let src_row = (sy * src_w) as usize * 4;
            for dx in 0..self.width {
                let sx = (off_x + (dx * crop_w) / self.width).min(src_w - 1);
                let s_idx = src_row + (sx as usize * 4);
                let d_idx = dst_row + (dx as usize * 4);
                self.pixels[d_idx] = ((src_rgba[s_idx] as u32 * dim_factor) >> 8) as u8;
                self.pixels[d_idx + 1] = ((src_rgba[s_idx + 1] as u32 * dim_factor) >> 8) as u8;
                self.pixels[d_idx + 2] = ((src_rgba[s_idx + 2] as u32 * dim_factor) >> 8) as u8;
                self.pixels[d_idx + 3] = 255;
            }
        }
    }

    pub fn blit_centered_contain(&mut self, img: &RgbaImage, max_fraction: f32) {
        let src_w = img.width();
        let src_h = img.height();
        if src_w == 0 || src_h == 0 {
            return;
        }
        let max_w = (self.width as f32 * max_fraction).max(16.0);
        let max_h = (self.height as f32 * max_fraction).max(16.0);
        let scale = (max_w / src_w as f32).min(max_h / src_h as f32);
        let dst_w = (src_w as f32 * scale).round().max(1.0) as u32;
        let dst_h = (src_h as f32 * scale).round().max(1.0) as u32;
        let start_x = (self.width as i32 - dst_w as i32) / 2;
        let start_y = (self.height as i32 - dst_h as i32) / 2;
        let raw = img.as_raw();

        for dy in 0..dst_h {
            let sy = ((dy * src_h) / dst_h).min(src_h - 1);
            for dx in 0..dst_w {
                let sx = ((dx * src_w) / dst_w).min(src_w - 1);
                let s_idx = ((sy * src_w + sx) as usize) * 4;
                let color = RgbaColor::new(
                    raw[s_idx],
                    raw[s_idx + 1],
                    raw[s_idx + 2],
                    raw[s_idx + 3],
                );
                self.blend_pixel(start_x + dx as i32, start_y + dy as i32, color);
            }
        }
    }
}

struct TextBlockSpec<'a> {
    text: &'a str,
    font_family: &'a str,
    font_size_px: f32,
    line_height_px: f32,
    bold: bool,
    italic: bool,
    glow: overture_core::GlowSpec,
    color: RgbaColor,
    h_align: HorizontalAlign,
    v_align: VerticalAlign,
    shadow: ShadowSpec,
    box_x: i32,
    box_y: i32,
    box_w: u32,
    box_h: u32,
}

/// Stateful slide compositor holding `cosmic-text` font/glyph caches and decoded image cache.
pub struct SlideCompositor {
    font_system: FontSystem,
    swash_cache: SwashCache,
    image_cache: HashMap<String, RgbaImage>,
}

impl Default for SlideCompositor {
    fn default() -> Self {
        Self::new()
    }
}

impl SlideCompositor {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
            image_cache: HashMap::new(),
        }
    }

    pub fn compose(
        &mut self,
        state: &OutputState,
        role: OutputRole,
        width: u32,
        height: u32,
        video_frame: Option<VideoFrameView<'_>>,
    ) -> RgbaFrame {
        match role {
            OutputRole::Main => self.compose_projector(state, state.active_theme(), false, width, height, video_frame),
            OutputRole::Return => {
                self.compose_projector(state, &state.return_theme, true, width, height, None)
            }
            OutputRole::Stage => self.compose_stage(state, width, height),
        }
    }

    pub fn compose_theme_preview(
        &mut self,
        theme: &ThemeConfig,
        sample_text: &str,
        width: u32,
        height: u32,
        video_frame: Option<VideoFrameView<'_>>,
    ) -> RgbaFrame {
        let mut frame = RgbaFrame::new(width, height, RgbaColor::BLACK);
        self.draw_background(&mut frame, &theme.background, video_frame);
        let scale = height as f32 / 1080.0;
        let margin_x = ((width as f32) * theme.margin_percent).round() as i32;
        let margin_y = ((height as f32) * theme.margin_percent).round() as i32;
        let box_w = width.saturating_sub((margin_x as u32) * 2).max(32);
        let box_h = height.saturating_sub((margin_y as u32) * 2).max(32);
        let font_px = (theme.font_size * scale).max(11.0);

        self.draw_text_block(
            &mut frame,
            TextBlockSpec {
                text: sample_text,
                font_family: &theme.font_family,
                font_size_px: font_px,
                line_height_px: font_px * theme.line_height_scale,
                bold: theme.bold,
                italic: theme.italic,
                glow: overture_core::GlowSpec { radius: theme.glow.radius * scale, ..theme.glow },
                color: theme.text_color,
                h_align: theme.h_align,
                v_align: theme.v_align,
                shadow: ShadowSpec {
                    enabled: theme.shadow.enabled,
                    color: theme.shadow.color,
                    offset_x: theme.shadow.offset_x * scale,
                    offset_y: theme.shadow.offset_y * scale,
                },
                box_x: margin_x,
                box_y: margin_y,
                box_w,
                box_h,
            },
        );
        frame
    }

    pub fn compose_countdown_preview(
        &mut self,
        theme: &ThemeConfig,
        countdown: &CountdownState,
        width: u32,
        height: u32,
        video_frame: Option<VideoFrameView<'_>>,
    ) -> RgbaFrame {
        let mut frame = RgbaFrame::new(width, height, RgbaColor::BLACK);
        self.draw_background(&mut frame, &theme.background, video_frame);
        let scale = height as f32 / 1080.0;
        self.draw_fullscreen_countdown(&mut frame, countdown, theme, scale);
        frame
    }

    fn compose_projector(
        &mut self,
        state: &OutputState,
        theme: &ThemeConfig,
        is_return_monitor: bool,
        width: u32,
        height: u32,
        video_frame: Option<VideoFrameView<'_>>,
    ) -> RgbaFrame {
        let scale = height as f32 / 1080.0;

        if state.display_mode == DisplayMode::BlackScreen {
            let mut frame = RgbaFrame::new(width, height, RgbaColor::BLACK);
            if is_return_monitor {
                self.draw_stage_alert_banner(&mut frame, state, scale);
            }
            return frame;
        }

        let mut frame = RgbaFrame::new(width, height, RgbaColor::BLACK);

        if state.display_mode == DisplayMode::Logo {
            frame.fill_vertical_gradient(
                RgbaColor::new(10, 14, 28, 255),
                RgbaColor::new(3, 5, 12, 255),
            );
            if let Some(logo_path) = state.logo_path.clone()
                && let Some(img) = self.load_cached_image(&logo_path)
            {
                frame.blit_centered_contain(&img, 0.42);
            } else {
                self.draw_text_block(
                    &mut frame,
                    TextBlockSpec {
                        text: "OVERTURE",
                        font_family: &theme.font_family,
                        font_size_px: (76.0 * scale).max(16.0),
                        line_height_px: (92.0 * scale).max(20.0),
                        bold: true,
                        italic: false,
                        glow: overture_core::GlowSpec::OFF,
                        color: RgbaColor::new(226, 232, 240, 220),
                        h_align: HorizontalAlign::Center,
                        v_align: VerticalAlign::Middle,
                        shadow: ShadowSpec::default(),
                        box_x: 0,
                        box_y: 0,
                        box_w: width,
                        box_h: height,
                    },
                );
            }
            if is_return_monitor {
                self.draw_stage_alert_banner(&mut frame, state, scale);
            }
            return frame;
        }

        // Media item image/video overrides background on main output
        let mut drew_item_media = false;
        if !is_return_monitor
            && let Some(item) = &state.current_item
            && item.kind == ServiceItemKind::Media
        {
            if let Some(vf) = video_frame {
                frame.blit_scaled_cover(vf.width, vf.height, vf.data, 0.0);
                drew_item_media = true;
            } else if let Some(path) = item.media_path.clone()
                && let Some(img) = self.load_cached_image(&path)
            {
                frame.blit_scaled_cover(img.width(), img.height(), img.as_raw(), 0.0);
                drew_item_media = true;
            }
        }

        if !drew_item_media {
            self.draw_background(&mut frame, &theme.background, video_frame);
        }

        let margin_x = ((width as f32) * theme.margin_percent).round() as i32;
        let margin_y = ((height as f32) * theme.margin_percent).round() as i32;
        let box_w = width.saturating_sub((margin_x as u32) * 2).max(32);
        let box_h = height.saturating_sub((margin_y as u32) * 2).max(32);
        let font_px = (theme.font_size * scale).max(12.0);

        if state.display_mode != DisplayMode::ClearText
            && let Some(slide) = state.current_slide()
        {
            if !slide.text.trim().is_empty() {
                self.draw_text_block(
                    &mut frame,
                    TextBlockSpec {
                        text: &slide.text,
                        font_family: &theme.font_family,
                        font_size_px: font_px,
                        line_height_px: font_px * theme.line_height_scale,
                        bold: theme.bold,
                        italic: theme.italic,
                        glow: overture_core::GlowSpec { radius: theme.glow.radius * scale, ..theme.glow },
                        color: theme.text_color,
                        h_align: theme.h_align,
                        v_align: theme.v_align,
                        shadow: ShadowSpec {
                            enabled: theme.shadow.enabled,
                            color: theme.shadow.color,
                            offset_x: theme.shadow.offset_x * scale,
                            offset_y: theme.shadow.offset_y * scale,
                        },
                        box_x: margin_x,
                        box_y: margin_y,
                        box_w,
                        box_h,
                    },
                );
            }

            if let Some(footer) = &slide.footer {
                let footer_px = (font_px * 0.52).max(10.0);
                let footer_h = (footer_px * 2.0).round() as u32;
                let footer_y = (height as i32) - margin_y - (footer_h as i32);
                self.draw_text_block(
                    &mut frame,
                    TextBlockSpec {
                        text: footer,
                        font_family: &theme.font_family,
                        font_size_px: footer_px,
                        line_height_px: footer_px * 1.25,
                        bold: true,
                        italic: false,
                        glow: overture_core::GlowSpec::OFF,
                        color: theme.footer_color,
                        h_align: theme.h_align,
                        v_align: VerticalAlign::Bottom,
                        shadow: ShadowSpec {
                            enabled: theme.shadow.enabled,
                            color: theme.shadow.color,
                            offset_x: 2.0 * scale,
                            offset_y: 2.0 * scale,
                        },
                        box_x: margin_x,
                        box_y: footer_y.max(0),
                        box_w,
                        box_h: footer_h,
                    },
                );
            }
        }

        // Countdown overlay on main/return when enabled
        if state.countdown.show_on_main && (state.countdown.active || state.countdown.remaining_secs > 0) {
            let has_slide_text = state.display_mode != DisplayMode::ClearText
                && state
                    .current_slide()
                    .map(|s| !s.text.trim().is_empty())
                    .unwrap_or(false)
                && state
                    .current_item
                    .as_ref()
                    .map(|i| i.kind != ServiceItemKind::Countdown)
                    .unwrap_or(false);
            if has_slide_text {
                self.draw_countdown_overlay(&mut frame, state, &theme.font_family, scale);
            } else {
                self.draw_fullscreen_countdown(&mut frame, &state.countdown, theme, scale);
            }
        }

        // Relógio (HH:MM) no canto superior direito da saída principal
        if theme.show_clock && !is_return_monitor {
            let now = chrono::Local::now().format("%H:%M").to_string();
            let px = (44.0 * scale).max(14.0);
            let box_w = ((frame.width as f32) * 0.22).round().max(60.0) as u32;
            let box_x = frame.width as i32 - box_w as i32 - (36.0 * scale) as i32;
            self.draw_text_block(
                &mut frame,
                TextBlockSpec {
                    text: &now,
                    font_family: &theme.font_family,
                    font_size_px: px,
                    line_height_px: px * 1.2,
                    bold: true,
                    italic: false,
                    glow: overture_core::GlowSpec::OFF,
                    color: RgbaColor::new(255, 255, 255, 220),
                    h_align: HorizontalAlign::Right,
                    v_align: VerticalAlign::Top,
                    shadow: ShadowSpec {
                        enabled: true,
                        color: RgbaColor::new(0, 0, 0, 200),
                        offset_x: 2.0 * scale,
                        offset_y: 2.0 * scale,
                    },
                    box_x,
                    box_y: (28.0 * scale) as i32,
                    box_w,
                    box_h: (px * 1.4) as u32,
                },
            );
        }

        // Manual Scoreboard overlay
        if state.scoreboard.visible {
            self.draw_scoreboard_overlay(&mut frame, state, &theme.font_family, scale);
        }

        // Return monitor receives stage communication alerts
        if is_return_monitor {
            self.draw_stage_alert_banner(&mut frame, state, scale);
        }

        frame
    }

    fn compose_stage(&mut self, state: &OutputState, width: u32, height: u32) -> RgbaFrame {
        let scale = height as f32 / 1080.0;
        let mut frame = RgbaFrame::new(width, height, RgbaColor::new(9, 11, 16, 255));
        let pad = (28.0 * scale).round().max(6.0) as i32;

        // Top header bar
        let header_h = (96.0 * scale).round().max(24.0) as u32;
        frame.fill_rect(0, 0, width, header_h, RgbaColor::new(18, 24, 38, 255));
        frame.fill_rect(
            0,
            header_h as i32 - 2,
            width,
            2,
            RgbaColor::new(56, 189, 248, 180),
        );

        let clock_now = Local::now().format("%H:%M:%S").to_string();
        let header_font = (38.0 * scale).max(10.0);
        self.draw_text_block(
            &mut frame,
            TextBlockSpec {
                text: &clock_now,
                font_family: "Segoe UI",
                font_size_px: header_font,
                line_height_px: header_font * 1.2,
                bold: true,
                italic: false,
                glow: overture_core::GlowSpec::OFF,
                color: RgbaColor::new(148, 163, 184, 255),
                h_align: HorizontalAlign::Left,
                v_align: VerticalAlign::Middle,
                shadow: ShadowSpec {
                    enabled: false,
                    ..Default::default()
                },
                box_x: pad,
                box_y: 0,
                box_w: width / 3,
                box_h: header_h,
            },
        );

        let title_label = match &state.current_item {
            Some(item) => {
                let mode_tag = match state.display_mode {
                    DisplayMode::BlackScreen => " [TELA PRETA]",
                    DisplayMode::Logo => " [LOGO]",
                    DisplayMode::ClearText => " [SEM TEXTO]",
                    DisplayMode::Normal => "",
                };
                format!(
                    "{} ({}/{}){}",
                    item.title,
                    (state.current_slide_index + 1).min(item.slides.len().max(1)),
                    item.slides.len(),
                    mode_tag
                )
            }
            None => "Aguardando item...".to_string(),
        };
        self.draw_text_block(
            &mut frame,
            TextBlockSpec {
                text: &title_label,
                font_family: "Segoe UI",
                font_size_px: (32.0 * scale).max(9.0),
                line_height_px: (38.0 * scale).max(11.0),
                bold: true,
                italic: false,
                glow: overture_core::GlowSpec::OFF,
                color: RgbaColor::WHITE,
                h_align: HorizontalAlign::Center,
                v_align: VerticalAlign::Middle,
                shadow: ShadowSpec {
                    enabled: false,
                    ..Default::default()
                },
                box_x: (width / 4) as i32,
                box_y: 0,
                box_w: width / 2,
                box_h: header_h,
            },
        );

        let timer_text = if state.countdown.active {
            format!("REG {}", state.countdown.formatted_mm_ss())
        } else {
            format!("CRON {}", state.formatted_stage_timer())
        };
        let timer_color = if state.countdown.active && state.countdown.remaining_secs <= 30 {
            RgbaColor::new(248, 113, 113, 255)
        } else {
            RgbaColor::new(52, 211, 153, 255)
        };
        self.draw_text_block(
            &mut frame,
            TextBlockSpec {
                text: &timer_text,
                font_family: "Segoe UI",
                font_size_px: header_font,
                line_height_px: header_font * 1.2,
                bold: true,
                italic: false,
                glow: overture_core::GlowSpec::OFF,
                color: timer_color,
                h_align: HorizontalAlign::Right,
                v_align: VerticalAlign::Middle,
                shadow: ShadowSpec {
                    enabled: false,
                    ..Default::default()
                },
                box_x: (width * 2 / 3) as i32,
                box_y: 0,
                box_w: (width / 3).saturating_sub(pad as u32),
                box_h: header_h,
            },
        );

        // Current slide area (54% of height)
        let body_top = header_h as i32 + pad;
        let current_h = ((height as f32) * 0.50).round().max(32.0) as u32;
        let current_w = width.saturating_sub((pad as u32) * 2).max(32);
        frame.fill_rect(
            pad,
            body_top,
            current_w,
            current_h,
            RgbaColor::new(15, 23, 42, 255),
        );

        let current_text = state
            .current_slide()
            .map(|s| s.text.as_str())
            .unwrap_or("—");
        let cur_font = (62.0 * scale).max(12.0);
        self.draw_text_block(
            &mut frame,
            TextBlockSpec {
                text: current_text,
                font_family: "Segoe UI",
                font_size_px: cur_font,
                line_height_px: cur_font * 1.28,
                bold: true,
                italic: false,
                glow: overture_core::GlowSpec::OFF,
                color: RgbaColor::WHITE,
                h_align: HorizontalAlign::Center,
                v_align: VerticalAlign::Middle,
                shadow: ShadowSpec {
                    enabled: false,
                    ..Default::default()
                },
                box_x: pad * 2,
                box_y: body_top + pad / 2,
                box_w: current_w.saturating_sub((pad as u32) * 2).max(16),
                box_h: current_h.saturating_sub(pad as u32).max(16),
            },
        );

        // Next slide + Notes lower split pane
        let lower_top = body_top + current_h as i32 + pad;
        let lower_h = height
            .saturating_sub(lower_top.max(0) as u32 + pad as u32)
            .max(24);
        let next_w = ((current_w as f32) * 0.65).round().max(16.0) as u32;
        let notes_w = current_w.saturating_sub(next_w + pad as u32).max(16);

        frame.fill_rect(
            pad,
            lower_top,
            next_w,
            lower_h,
            RgbaColor::new(23, 31, 50, 255),
        );
        frame.fill_rect(
            pad + next_w as i32 + pad,
            lower_top,
            notes_w,
            lower_h,
            RgbaColor::new(23, 31, 50, 255),
        );

        let next_text = match state.next_slide() {
            Some(ns) => format!("PRÓXIMO:\n{}", ns.text),
            None => "PRÓXIMO:\n[Fim do item]".to_string(),
        };
        let next_font = (38.0 * scale).max(10.0);
        self.draw_text_block(
            &mut frame,
            TextBlockSpec {
                text: &next_text,
                font_family: "Segoe UI",
                font_size_px: next_font,
                line_height_px: next_font * 1.26,
                bold: false,
                italic: false,
                glow: overture_core::GlowSpec::OFF,
                color: RgbaColor::new(253, 224, 71, 255),
                h_align: HorizontalAlign::Left,
                v_align: VerticalAlign::Top,
                shadow: ShadowSpec {
                    enabled: false,
                    ..Default::default()
                },
                box_x: pad + pad / 2,
                box_y: lower_top + pad / 2,
                box_w: next_w.saturating_sub(pad as u32).max(16),
                box_h: lower_h.saturating_sub(pad as u32).max(16),
            },
        );

        let notes_body = state
            .current_item
            .as_ref()
            .map(|i| i.notes.trim())
            .filter(|n| !n.is_empty())
            .unwrap_or("Sem notas");
        let notes_text = format!("NOTAS:\n{}", notes_body);
        let notes_font = (28.0 * scale).max(9.0);
        self.draw_text_block(
            &mut frame,
            TextBlockSpec {
                text: &notes_text,
                font_family: "Segoe UI",
                font_size_px: notes_font,
                line_height_px: notes_font * 1.25,
                bold: false,
                italic: false,
                glow: overture_core::GlowSpec::OFF,
                color: RgbaColor::new(203, 213, 225, 255),
                h_align: HorizontalAlign::Left,
                v_align: VerticalAlign::Top,
                shadow: ShadowSpec {
                    enabled: false,
                    ..Default::default()
                },
                box_x: pad + next_w as i32 + pad + pad / 2,
                box_y: lower_top + pad / 2,
                box_w: notes_w.saturating_sub(pad as u32).max(16),
                box_h: lower_h.saturating_sub(pad as u32).max(16),
            },
        );

        self.draw_stage_alert_banner(&mut frame, state, scale);
        frame
    }

    fn draw_background(
        &mut self,
        frame: &mut RgbaFrame,
        bg: &BackgroundSpec,
        video_frame: Option<VideoFrameView<'_>>,
    ) {
        match bg {
            BackgroundSpec::SolidColor { color } => {
                frame.fill_rect(0, 0, frame.width, frame.height, *color);
            }
            BackgroundSpec::Gradient { top, bottom } => {
                frame.fill_vertical_gradient(*top, *bottom);
            }
            BackgroundSpec::Image { path, dim_opacity } => {
                let path_owned = path.clone();
                let dim = *dim_opacity;
                if let Some(img) = self.load_cached_image(&path_owned) {
                    frame.blit_scaled_cover(img.width(), img.height(), img.as_raw(), dim);
                } else {
                    frame.fill_vertical_gradient(
                        RgbaColor::new(15, 23, 42, 255),
                        RgbaColor::new(3, 7, 18, 255),
                    );
                }
            }
            BackgroundSpec::Video { dim_opacity, .. } => {
                if let Some(vf) = video_frame {
                    frame.blit_scaled_cover(vf.width, vf.height, vf.data, *dim_opacity);
                } else {
                    frame.fill_vertical_gradient(
                        RgbaColor::new(12, 20, 44, 255),
                        RgbaColor::new(4, 8, 20, 255),
                    );
                }
            }
        }
    }

    fn draw_fullscreen_countdown(
        &mut self,
        frame: &mut RgbaFrame,
        cd: &CountdownState,
        theme: &ThemeConfig,
        scale: f32,
    ) {
        let clock_str = cd.formatted_mm_ss();
        let before = cd.text_before.trim();
        let after = if !cd.text_after.trim().is_empty() {
            cd.text_after.trim()
        } else {
            cd.label.trim()
        };

        let margin_x = ((frame.width as f32) * 0.06).round() as i32;
        let box_w = frame.width.saturating_sub((margin_x as u32) * 2).max(32);
        let shadow = ShadowSpec {
            enabled: true,
            color: RgbaColor::new(0, 0, 0, 220),
            offset_x: (4.0 * scale).max(1.0),
            offset_y: (4.0 * scale).max(1.0),
        };

        let clock_px = (188.0 * scale).max(24.0);
        let clock_box_h = (clock_px * 1.25).round() as u32;
        let center_y = (frame.height as i32 / 2) - (clock_box_h as i32 / 2)
            - if !after.is_empty() {
                (38.0 * scale).round() as i32
            } else {
                0
            };

        if !before.is_empty() {
            let sub_px = (58.0 * scale).max(12.0);
            let sub_h = (sub_px * 2.2).round() as u32;
            let by = (center_y - sub_h as i32 - (12.0 * scale).round() as i32).max(8);
            self.draw_text_block(
                frame,
                TextBlockSpec {
                    text: before,
                    font_family: &theme.font_family,
                    font_size_px: sub_px,
                    line_height_px: sub_px * 1.22,
                    bold: true,
                    italic: false,
                    glow: overture_core::GlowSpec::OFF,
                    color: theme.text_color,
                    h_align: HorizontalAlign::Center,
                    v_align: VerticalAlign::Bottom,
                    shadow,
                    box_x: margin_x,
                    box_y: by,
                    box_w,
                    box_h: sub_h,
                },
            );
        }

        self.draw_text_block(
            frame,
            TextBlockSpec {
                text: &clock_str,
                font_family: &theme.font_family,
                font_size_px: clock_px,
                line_height_px: clock_px * 1.12,
                bold: true,
                italic: false,
                glow: overture_core::GlowSpec::OFF,
                color: theme.text_color,
                h_align: HorizontalAlign::Center,
                v_align: VerticalAlign::Middle,
                shadow,
                box_x: margin_x,
                box_y: center_y.max(0),
                box_w,
                box_h: clock_box_h,
            },
        );

        if !after.is_empty() {
            let after_px = (68.0 * scale).max(13.0);
            let after_h = (after_px * 2.4).round() as u32;
            let ay = (center_y + clock_box_h as i32 + (14.0 * scale).round() as i32)
                .min(frame.height as i32 - 16);
            self.draw_text_block(
                frame,
                TextBlockSpec {
                    text: after,
                    font_family: &theme.font_family,
                    font_size_px: after_px,
                    line_height_px: after_px * 1.24,
                    bold: true,
                    italic: false,
                    glow: overture_core::GlowSpec::OFF,
                    color: theme.text_color,
                    h_align: HorizontalAlign::Center,
                    v_align: VerticalAlign::Top,
                    shadow,
                    box_x: margin_x,
                    box_y: ay.max(0),
                    box_w,
                    box_h: after_h,
                },
            );
        }
    }

    fn draw_countdown_overlay(
        &mut self,
        frame: &mut RgbaFrame,
        state: &OutputState,
        font_family: &str,
        scale: f32,
    ) {
        let card_w = ((frame.width as f32) * 0.42).round().max(120.0) as u32;
        let card_h = ((frame.height as f32) * 0.24).round().max(60.0) as u32;
        let card_x = ((frame.width - card_w) / 2) as i32;
        let card_y = ((frame.height as f32) * 0.05).round() as i32;

        frame.fill_rect(
            card_x,
            card_y,
            card_w,
            card_h,
            RgbaColor::new(9, 13, 26, 225),
        );
        frame.fill_rect(
            card_x,
            card_y + card_h as i32 - 4,
            card_w,
            4,
            RgbaColor::new(251, 191, 36, 255),
        );

        let text = if state.countdown.label.trim().is_empty() {
            state.countdown.formatted_mm_ss()
        } else {
            format!(
                "{}\n{}",
                state.countdown.label,
                state.countdown.formatted_mm_ss()
            )
        };
        let font_px = (68.0 * scale).max(14.0);
        self.draw_text_block(
            frame,
            TextBlockSpec {
                text: &text,
                font_family,
                font_size_px: font_px,
                line_height_px: font_px * 1.18,
                bold: true,
                italic: false,
                glow: overture_core::GlowSpec::OFF,
                color: RgbaColor::new(251, 191, 36, 255),
                h_align: HorizontalAlign::Center,
                v_align: VerticalAlign::Middle,
                shadow: ShadowSpec::default(),
                box_x: card_x + 12,
                box_y: card_y + 8,
                box_w: card_w.saturating_sub(24).max(16),
                box_h: card_h.saturating_sub(16).max(16),
            },
        );
    }

    fn draw_scoreboard_overlay(
        &mut self,
        frame: &mut RgbaFrame,
        state: &OutputState,
        font_family: &str,
        scale: f32,
    ) {
        let sb = &state.scoreboard;
        let card_w = ((frame.width as f32) * 0.62).round().max(160.0) as u32;
        let card_h = ((frame.height as f32) * 0.18).round().max(48.0) as u32;
        let card_x = ((frame.width - card_w) / 2) as i32;
        let card_y = ((frame.height as f32) * 0.04).round() as i32;

        frame.fill_rect(
            card_x,
            card_y,
            card_w,
            card_h,
            RgbaColor::new(15, 23, 42, 235),
        );
        frame.fill_rect(
            card_x,
            card_y,
            card_w,
            4,
            RgbaColor::new(56, 189, 248, 255),
        );

        let score_line = format!(
            "{}\n{}   {}  ×  {}   {}",
            sb.title, sb.team_a_name, sb.team_a_score, sb.team_b_score, sb.team_b_name
        );
        let font_px = (44.0 * scale).max(11.0);
        self.draw_text_block(
            frame,
            TextBlockSpec {
                text: &score_line,
                font_family,
                font_size_px: font_px,
                line_height_px: font_px * 1.25,
                bold: true,
                italic: false,
                glow: overture_core::GlowSpec::OFF,
                color: RgbaColor::WHITE,
                h_align: HorizontalAlign::Center,
                v_align: VerticalAlign::Middle,
                shadow: ShadowSpec::default(),
                box_x: card_x + 16,
                box_y: card_y + 8,
                box_w: card_w.saturating_sub(32).max(16),
                box_h: card_h.saturating_sub(16).max(16),
            },
        );
    }

    fn draw_stage_alert_banner(&mut self, frame: &mut RgbaFrame, state: &OutputState, scale: f32) {
        let Some(alert) = &state.stage_alert else {
            return;
        };
        let banner_h = (88.0 * scale).round().max(24.0) as u32;
        let banner_y = frame.height.saturating_sub(banner_h) as i32;
        frame.fill_rect(
            0,
            banner_y,
            frame.width,
            banner_h,
            RgbaColor::new(185, 28, 28, 242),
        );
        let font_px = (42.0 * scale).max(10.0);
        let msg = format!("ALERTA: {}", alert);
        self.draw_text_block(
            frame,
            TextBlockSpec {
                text: &msg,
                font_family: "Segoe UI",
                font_size_px: font_px,
                line_height_px: font_px * 1.2,
                bold: true,
                italic: false,
                glow: overture_core::GlowSpec::OFF,
                color: RgbaColor::WHITE,
                h_align: HorizontalAlign::Center,
                v_align: VerticalAlign::Middle,
                shadow: ShadowSpec {
                    enabled: false,
                    ..Default::default()
                },
                box_x: 16,
                box_y: banner_y,
                box_w: frame.width.saturating_sub(32).max(16),
                box_h: banner_h,
            },
        );
    }

    fn draw_text_block(&mut self, frame: &mut RgbaFrame, spec: TextBlockSpec<'_>) {
        if spec.text.is_empty() || spec.box_w == 0 || spec.box_h == 0 {
            return;
        }
        let metrics = Metrics::new(spec.font_size_px, spec.line_height_px);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(
            &mut self.font_system,
            Some(spec.box_w as f32),
            Some(spec.box_h as f32 * 2.0),
        );

        let family = if spec.font_family.trim().is_empty() {
            Family::SansSerif
        } else {
            Family::Name(spec.font_family)
        };
        let attrs = Attrs::new()
            .family(family)
            .weight(if spec.bold { Weight::BOLD } else { Weight::NORMAL })
            .style(if spec.italic { Style::Italic } else { Style::Normal });
        buffer.set_text(&mut self.font_system, spec.text, attrs, Shaping::Advanced);

        let align = match spec.h_align {
            HorizontalAlign::Left => Align::Left,
            HorizontalAlign::Center => Align::Center,
            HorizontalAlign::Right => Align::Right,
        };
        for line in buffer.lines.iter_mut() {
            line.set_align(Some(align));
        }
        buffer.shape_until_scroll(&mut self.font_system, false);

        let run_count = buffer.layout_runs().count();
        if run_count == 0 {
            return;
        }
        let total_text_h = (run_count as f32) * spec.line_height_px;
        let y_offset = match spec.v_align {
            VerticalAlign::Top => spec.box_y,
            VerticalAlign::Middle => {
                spec.box_y + ((spec.box_h as f32 - total_text_h) * 0.5).round() as i32
            }
            VerticalAlign::Bottom => {
                spec.box_y + (spec.box_h as f32 - total_text_h).round() as i32
            }
        };

        // Brilho: o texto é desenhado em anel de deslocamentos com alfa baixo, formando um halo.
        if spec.glow.enabled && spec.glow.radius > 0.0 && spec.glow.color.a > 0 {
            let r = spec.glow.radius;
            let steps = 24u32;
            let pass_alpha = (spec.glow.color.a as f32 / 5.0).round().clamp(1.0, 255.0) as u8;
            let g_col = Color::rgba(spec.glow.color.r, spec.glow.color.g, spec.glow.color.b, pass_alpha);
            for ring in [0.5f32, 1.0] {
                for k in 0..steps {
                    let ang = std::f32::consts::TAU * k as f32 / steps as f32;
                    let gx = spec.box_x + (ang.cos() * r * ring).round() as i32;
                    let gy = y_offset + (ang.sin() * r * ring).round() as i32;
                    buffer.draw(&mut self.font_system, &mut self.swash_cache, g_col, |x, y, w, h, drawn| {
                        let a = drawn.a();
                        if a == 0 {
                            return;
                        }
                        let px_col = RgbaColor::new(drawn.r(), drawn.g(), drawn.b(), a);
                        if w == 1 && h == 1 {
                            frame.blend_pixel(gx + x, gy + y, px_col);
                        } else {
                            frame.fill_rect(gx + x, gy + y, w, h, px_col);
                        }
                    });
                }
            }
        }

        if spec.shadow.enabled && spec.shadow.color.a > 0 {
            let sx = spec.box_x + spec.shadow.offset_x.round() as i32;
            let sy = y_offset + spec.shadow.offset_y.round() as i32;
            let s_col = Color::rgba(
                spec.shadow.color.r,
                spec.shadow.color.g,
                spec.shadow.color.b,
                spec.shadow.color.a,
            );
            buffer.draw(
                &mut self.font_system,
                &mut self.swash_cache,
                s_col,
                |x, y, w, h, drawn| {
                    let a = drawn.a();
                    if a == 0 {
                        return;
                    }
                    let px_col = RgbaColor::new(drawn.r(), drawn.g(), drawn.b(), a);
                    if w == 1 && h == 1 {
                        frame.blend_pixel(sx + x, sy + y, px_col);
                    } else {
                        frame.fill_rect(sx + x, sy + y, w, h, px_col);
                    }
                },
            );
        }

        let fg_col = Color::rgba(spec.color.r, spec.color.g, spec.color.b, spec.color.a);
        let ox = spec.box_x;
        let oy = y_offset;
        buffer.draw(
            &mut self.font_system,
            &mut self.swash_cache,
            fg_col,
            |x, y, w, h, drawn| {
                let a = drawn.a();
                if a == 0 {
                    return;
                }
                let px_col = RgbaColor::new(drawn.r(), drawn.g(), drawn.b(), a);
                if w == 1 && h == 1 {
                    frame.blend_pixel(ox + x, oy + y, px_col);
                } else {
                    frame.fill_rect(ox + x, oy + y, w, h, px_col);
                }
            },
        );
    }

    fn load_cached_image(&mut self, path: &str) -> Option<RgbaImage> {
        if let Some(cached) = self.image_cache.get(path) {
            return Some(cached.clone());
        }
        if !Path::new(path).exists() {
            return None;
        }
        let decoded = image::open(path).ok()?.to_rgba8();
        if self.image_cache.len() > 24 {
            self.image_cache.clear();
        }
        self.image_cache.insert(path.to_string(), decoded.clone());
        Some(decoded)
    }
}

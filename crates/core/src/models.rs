use serde::{Deserialize, Serialize};

/// RGBA color representation for themes, backgrounds, and shadows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaColor {
    pub const WHITE: Self = Self::new(255, 255, 255, 255);
    pub const BLACK: Self = Self::new(0, 0, 0, 255);
    pub const TRANSPARENT: Self = Self::new(0, 0, 0, 0);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_hex(hex: &str) -> Option<Self> {
        let s = hex.trim().trim_start_matches('#');
        match s.len() {
            6 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                Some(Self::new(r, g, b, 255))
            }
            8 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                let a = u8::from_str_radix(&s[6..8], 16).ok()?;
                Some(Self::new(r, g, b, a))
            }
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }

    pub fn to_f32_array(self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HorizontalAlign {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VerticalAlign {
    Top,
    #[default]
    Middle,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackgroundSpec {
    SolidColor {
        color: RgbaColor,
    },
    Gradient {
        top: RgbaColor,
        bottom: RgbaColor,
    },
    Image {
        path: String,
        dim_opacity: f32,
    },
    Video {
        path: String,
        dim_opacity: f32,
        loop_playback: bool,
    },
}

impl Default for BackgroundSpec {
    fn default() -> Self {
        Self::Gradient {
            top: RgbaColor::new(11, 16, 36, 255),
            bottom: RgbaColor::new(4, 7, 18, 255),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShadowSpec {
    pub enabled: bool,
    pub color: RgbaColor,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl Default for ShadowSpec {
    fn default() -> Self {
        Self {
            enabled: true,
            color: RgbaColor::new(0, 0, 0, 210),
            offset_x: 3.0,
            offset_y: 3.0,
        }
    }
}

/// Brilho (glow) ao redor do texto: halo de cor com raio em pixels de referência 1080p.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GlowSpec {
    pub enabled: bool,
    pub color: RgbaColor,
    pub radius: f32,
}

impl GlowSpec {
    pub const OFF: Self = Self {
        enabled: false,
        color: RgbaColor::BLACK,
        radius: 0.0,
    };
}

impl Default for GlowSpec {
    fn default() -> Self {
        Self::OFF
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransitionKind {
    #[default]
    None,
    Fade {
        duration_ms: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub font_family: String,
    pub font_size: f32,
    pub line_height_scale: f32,
    pub bold: bool,
    pub text_color: RgbaColor,
    pub footer_color: RgbaColor,
    pub h_align: HorizontalAlign,
    pub v_align: VerticalAlign,
    pub margin_percent: f32,
    pub shadow: ShadowSpec,
    pub background: BackgroundSpec,
    pub transition: TransitionKind,
    /// Itálico do texto.
    pub italic: bool,
    /// Brilho ao redor do texto.
    pub glow: GlowSpec,
    /// Mostra o relógio (HH:MM) no canto superior direito da saída principal.
    pub show_clock: bool,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            font_family: "Segoe UI".to_string(),
            font_size: 80.0,
            line_height_scale: 1.30,
            bold: true,
            text_color: RgbaColor::WHITE,
            footer_color: RgbaColor::new(251, 191, 36, 255),
            h_align: HorizontalAlign::Center,
            v_align: VerticalAlign::Middle,
            margin_percent: 0.08,
            shadow: ShadowSpec::default(),
            background: BackgroundSpec::default(),
            transition: TransitionKind::Fade { duration_ms: 180 },
            italic: false,
            glow: GlowSpec::OFF,
            show_clock: false,
        }
    }
}

impl ThemeConfig {
    pub fn scripture_preset() -> Self {
        Self {
            font_family: "Georgia".to_string(),
            font_size: 62.0,
            line_height_scale: 1.35,
            bold: false,
            text_color: RgbaColor::new(248, 250, 252, 255),
            footer_color: RgbaColor::new(251, 191, 36, 255),
            h_align: HorizontalAlign::Center,
            v_align: VerticalAlign::Middle,
            margin_percent: 0.09,
            shadow: ShadowSpec::default(),
            background: BackgroundSpec::Gradient {
                top: RgbaColor::new(15, 23, 42, 255),
                bottom: RgbaColor::new(2, 6, 23, 255),
            },
            transition: TransitionKind::Fade { duration_ms: 180 },
            italic: false,
            glow: GlowSpec::OFF,
            show_clock: false,
        }
    }

    pub fn return_monitor_preset() -> Self {
        Self {
            font_family: "Segoe UI".to_string(),
            font_size: 72.0,
            line_height_scale: 1.28,
            bold: true,
            text_color: RgbaColor::new(250, 204, 21, 255),
            footer_color: RgbaColor::WHITE,
            h_align: HorizontalAlign::Center,
            v_align: VerticalAlign::Middle,
            margin_percent: 0.06,
            shadow: ShadowSpec {
                enabled: false,
                color: RgbaColor::BLACK,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            background: BackgroundSpec::SolidColor {
                color: RgbaColor::BLACK,
            },
            transition: TransitionKind::None,
            italic: false,
            glow: GlowSpec::OFF,
            show_clock: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    pub id: i64,
    pub name: String,
    pub is_return_theme: bool,
    pub config: ThemeConfig,
}

impl Theme {
    pub fn default_main() -> Self {
        Self {
            id: 1,
            name: "Culto Principal".to_string(),
            is_return_theme: false,
            config: ThemeConfig::default(),
        }
    }

    pub fn default_scripture() -> Self {
        Self {
            id: 2,
            name: "Escrituras".to_string(),
            is_return_theme: false,
            config: ThemeConfig::scripture_preset(),
        }
    }

    pub fn default_return_monitor() -> Self {
        Self {
            id: 3,
            name: "Retorno Palco (Alto Contraste)".to_string(),
            is_return_theme: true,
            config: ThemeConfig::return_monitor_preset(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SongSlide {
    pub id: i64,
    pub song_id: i64,
    pub order: i32,
    pub text: String,
    pub forced_break: bool,
    pub duration_secs: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Song {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub author: String,
    pub group_id: Option<i64>,
    pub notes: String,
    pub source: String,
    pub archived: bool,
    pub duplicate_marked: bool,
    pub created_at: String,
    pub slides: Vec<SongSlide>,
}

impl Song {
    pub fn new_draft(title: impl Into<String>, artist: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            id: 0,
            title: title.into(),
            artist: artist.into(),
            author: String::new(),
            group_id: None,
            notes: String::new(),
            source: source.into(),
            archived: false,
            duplicate_marked: false,
            created_at: chrono::Utc::now().to_rfc3339(),
            slides: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Video,
    Audio,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }

    pub fn from_str_name(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "video" => Self::Video,
            "audio" => Self::Audio,
            _ => Self::Image,
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "webp" | "bmp" => Some(Self::Image),
            "mp4" | "mkv" | "webm" | "mov" | "avi" | "wmv" => Some(Self::Video),
            "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" => Some(Self::Audio),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub kind: MediaKind,
    pub duration_secs: Option<f64>,
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BibleVersion {
    pub id: i64,
    pub abbr: String,
    pub name: String,
    pub lang: String,
    pub license: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BibleVerse {
    pub version_id: i64,
    pub book: String,
    pub book_order: i32,
    pub chapter: i32,
    pub verse: i32,
    pub text: String,
}

impl BibleVerse {
    pub fn reference(&self, version_abbr: Option<&str>) -> String {
        match version_abbr {
            Some(abbr) if !abbr.is_empty() => {
                format!("{} {}:{} ({})", self.book, self.chapter, self.verse, abbr)
            }
            _ => format!("{} {}:{}", self.book, self.chapter, self.verse),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BibleFavorite {
    pub id: i64,
    pub version_id: i64,
    pub book: String,
    pub chapter: i32,
    pub verse: i32,
    pub text: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceItemKind {
    Song,
    Bible,
    Media,
    Announcement,
    Countdown,
    Presentation,
}

impl ServiceItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Song => "song",
            Self::Bible => "bible",
            Self::Media => "media",
            Self::Announcement => "announcement",
            Self::Countdown => "countdown",
            Self::Presentation => "presentation",
        }
    }

    pub fn from_str_name(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "song" => Self::Song,
            "bible" => Self::Bible,
            "media" => Self::Media,
            "countdown" => Self::Countdown,
            "presentation" => Self::Presentation,
            _ => Self::Announcement,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceItemParams {
    pub custom_title: Option<String>,
    pub custom_slides: Vec<String>,
    pub bible_version_id: Option<i64>,
    pub bible_book: Option<String>,
    pub bible_chapter: Option<i32>,
    pub bible_verses: Vec<i32>,
    pub countdown_seconds: Option<u64>,
    pub countdown_label: Option<String>,
    pub theme_id: Option<i64>,
    pub auto_advance_secs: Option<u32>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceItem {
    pub id: i64,
    pub service_id: i64,
    pub order: i32,
    pub kind: ServiceItemKind,
    pub ref_id: Option<i64>,
    pub title: String,
    pub subtitle: String,
    pub params: ServiceItemParams,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Service {
    pub id: i64,
    pub date: String,
    pub name: String,
    pub is_temporary: bool,
    pub archived: bool,
    pub items: Vec<ServiceItem>,
}

impl Service {
    pub fn new_temporary(name: impl Into<String>) -> Self {
        Self {
            id: -1,
            date: chrono::Local::now().format("%Y-%m-%d").to_string(),
            name: name.into(),
            is_temporary: true,
            archived: false,
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMember {
    pub id: i64,
    pub name: String,
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleEntry {
    pub id: i64,
    pub service_id: i64,
    pub member_id: i64,
    pub member_name: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberScheduleCount {
    pub member_id: i64,
    pub member_name: String,
    pub times_scheduled: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SongPlayStat {
    pub song_id: i64,
    pub title: String,
    pub artist: String,
    pub play_count: i64,
    /// Optional 0..=6 (Sunday = 0 .. Saturday = 6) when grouped by day of week.
    pub weekday: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputRole {
    Main,
    Stage,
    Return,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorDescriptor {
    pub index: usize,
    pub system_name: String,
    pub friendly_name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MonitorAssignment {
    pub main_monitor: Option<usize>,
    pub stage_monitor: Option<usize>,
    pub return_monitor: Option<usize>,
    pub main_enabled: bool,
    pub stage_enabled: bool,
    pub return_enabled: bool,
    pub return_theme_id: Option<i64>,
    #[serde(default)]
    pub windowed_mode: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountdownState {
    pub active: bool,
    pub label: String,
    #[serde(default)]
    pub text_before: String,
    #[serde(default)]
    pub text_after: String,
    #[serde(default = "default_true")]
    pub pad_zero: bool,
    #[serde(default)]
    pub hide_zero_min: bool,
    pub remaining_secs: u64,
    pub total_secs: u64,
    pub show_on_main: bool,
}

impl Default for CountdownState {
    fn default() -> Self {
        Self {
            active: false,
            label: String::new(),
            text_before: String::new(),
            text_after: String::new(),
            pad_zero: true,
            hide_zero_min: false,
            remaining_secs: 0,
            total_secs: 0,
            show_on_main: false,
        }
    }
}

impl CountdownState {
    pub fn formatted_mm_ss(&self) -> String {
        let mins = self.remaining_secs / 60;
        let secs = self.remaining_secs % 60;
        if self.hide_zero_min && mins == 0 {
            if self.pad_zero {
                format!("{:02}", secs)
            } else {
                format!("{}", secs)
            }
        } else if self.pad_zero {
            format!("{:02}:{:02}", mins, secs)
        } else {
            format!("{}:{:02}", mins, secs)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreboardState {
    pub visible: bool,
    pub title: String,
    pub team_a_name: String,
    pub team_a_score: i32,
    pub team_b_name: String,
    pub team_b_score: i32,
}

impl Default for ScoreboardState {
    fn default() -> Self {
        Self {
            visible: false,
            title: "Placar".to_string(),
            team_a_name: "Equipe A".to_string(),
            team_a_score: 0,
            team_b_name: "Equipe B".to_string(),
            team_b_score: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutMap {
    pub next_slide: String,
    pub prev_slide: String,
    pub black_screen: String,
    pub logo_screen: String,
    pub clear_text: String,
    pub first_slide: String,
    pub last_slide: String,
}

impl Default for ShortcutMap {
    fn default() -> Self {
        Self {
            next_slide: "Right".to_string(),
            prev_slide: "Left".to_string(),
            black_screen: "B".to_string(),
            logo_screen: "L".to_string(),
            clear_text: "Escape".to_string(),
            first_slide: "Home".to_string(),
            last_slide: "End".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    pub locale: String,
    pub api_port: u16,
    pub api_token: String,
    pub logo_path: Option<String>,
    pub auto_advance_default_secs: Option<u32>,
    pub shortcuts: ShortcutMap,
    pub monitor_assignment: MonitorAssignment,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            locale: "pt-BR".to_string(),
            api_port: 8787,
            api_token: "overture-local-2026".to_string(),
            logo_path: None,
            auto_advance_default_secs: None,
            shortcuts: ShortcutMap::default(),
            monitor_assignment: MonitorAssignment::default(),
        }
    }
}

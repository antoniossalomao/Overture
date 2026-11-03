use crate::models::{
    CountdownState, MediaKind, ScoreboardState, ServiceItemKind, Song, ThemeConfig,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, watch};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationSlide {
    pub index: usize,
    pub text: String,
    pub footer: Option<String>,
    pub forced_break: bool,
    pub duration_secs: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LivePresentationItem {
    pub service_item_id: Option<i64>,
    pub song_id: Option<i64>,
    pub title: String,
    pub subtitle: String,
    pub notes: String,
    pub kind: ServiceItemKind,
    pub slides: Vec<PresentationSlide>,
    pub theme: ThemeConfig,
    pub media_path: Option<String>,
    pub media_kind: Option<MediaKind>,
}

impl LivePresentationItem {
    pub fn from_song(song: &Song, service_item_id: Option<i64>, theme: ThemeConfig) -> Self {
        let slides = song
            .slides
            .iter()
            .enumerate()
            .map(|(idx, s)| PresentationSlide {
                index: idx,
                text: s.text.clone(),
                footer: None, // Title and artist shown on operator/stage panel, not main output
                forced_break: s.forced_break,
                duration_secs: s.duration_secs,
            })
            .collect();

        Self {
            service_item_id,
            song_id: Some(song.id),
            title: song.title.clone(),
            subtitle: song.artist.clone(),
            notes: song.notes.clone(),
            kind: ServiceItemKind::Song,
            slides,
            theme,
            media_path: None,
            media_kind: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    #[default]
    Normal,
    BlackScreen,
    Logo,
    ClearText,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputState {
    pub revision: u64,
    pub dispatched_at_epoch_us: u64,
    pub display_mode: DisplayMode,
    pub current_item: Option<LivePresentationItem>,
    pub current_slide_index: usize,
    pub auto_play_enabled: bool,
    pub auto_play_elapsed_secs: u32,
    /// Repetir do início ao fim da lista na apresentação automática. Se desligado, para no último slide.
    pub auto_play_repeat: bool,
    pub default_slide_duration_secs: u32,
    pub countdown: CountdownState,
    pub scoreboard: ScoreboardState,
    pub stage_alert: Option<String>,
    pub stage_elapsed_secs: u64,
    pub stage_timer_running: bool,
    pub main_theme: ThemeConfig,
    pub return_theme: ThemeConfig,
    pub logo_path: Option<String>,
}

impl Default for OutputState {
    fn default() -> Self {
        Self {
            revision: 0,
            dispatched_at_epoch_us: now_epoch_us(),
            display_mode: DisplayMode::Normal,
            current_item: None,
            current_slide_index: 0,
            auto_play_enabled: false,
            auto_play_elapsed_secs: 0,
            auto_play_repeat: true,
            default_slide_duration_secs: 6,
            countdown: CountdownState::default(),
            scoreboard: ScoreboardState::default(),
            stage_alert: None,
            stage_elapsed_secs: 0,
            stage_timer_running: false,
            main_theme: ThemeConfig::default(),
            return_theme: ThemeConfig::return_monitor_preset(),
            logo_path: None,
        }
    }
}

impl OutputState {
    pub fn current_slide(&self) -> Option<&PresentationSlide> {
        let item = self.current_item.as_ref()?;
        item.slides.get(self.current_slide_index)
    }

    pub fn next_slide(&self) -> Option<&PresentationSlide> {
        let item = self.current_item.as_ref()?;
        item.slides.get(self.current_slide_index + 1)
    }

    pub fn total_slides(&self) -> usize {
        self.current_item
            .as_ref()
            .map(|i| i.slides.len())
            .unwrap_or(0)
    }

    pub fn active_theme(&self) -> &ThemeConfig {
        self.current_item
            .as_ref()
            .map(|i| &i.theme)
            .unwrap_or(&self.main_theme)
    }

    pub fn formatted_stage_timer(&self) -> String {
        let hrs = self.stage_elapsed_secs / 3600;
        let mins = (self.stage_elapsed_secs % 3600) / 60;
        let secs = self.stage_elapsed_secs % 60;
        if hrs > 0 {
            format!("{:02}:{:02}:{:02}", hrs, mins, secs)
        } else {
            format!("{:02}:{:02}", mins, secs)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum CoreCommand {
    PresentItem {
        item: Box<LivePresentationItem>,
        start_slide: usize,
    },
    NextSlide,
    PrevSlide,
    FirstSlide,
    LastSlide,
    GotoSlide {
        index: usize,
    },
    SetDisplayMode {
        mode: DisplayMode,
    },
    ToggleBlackScreen,
    ToggleLogo,
    ToggleClearText,
    ClearPresentation,
    SetMainTheme {
        theme: Box<ThemeConfig>,
    },
    SetReturnTheme {
        theme: Box<ThemeConfig>,
    },
    SetLogoPath {
        path: Option<String>,
    },
    SetAutoPlay {
        enabled: bool,
        default_duration_secs: Option<u32>,
    },
    SetAutoPlayRepeat {
        repeat: bool,
    },
    StartCountdown {
        label: String,
        seconds: u64,
        show_on_main: bool,
    },
    StartStudioCountdown {
        seconds: u64,
        text_before: String,
        text_after: String,
        pad_zero: bool,
        hide_zero_min: bool,
        show_on_main: bool,
    },
    StopCountdown,
    UpdateCurrentItemNotes {
        notes: String,
    },
    UpdateScoreboard {
        scoreboard: ScoreboardState,
    },
    IncrementScore {
        team_a_delta: i32,
        team_b_delta: i32,
    },
    SetStageAlert {
        message: Option<String>,
    },
    StartStageTimer,
    PauseStageTimer,
    ResetStageTimer,
    /// Called every 1 second by the application heartbeat timer.
    TickSecond,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum CoreEvent {
    OutputUpdated {
        state: Box<OutputState>,
    },
    ItemExecuted {
        service_item_id: Option<i64>,
        song_id: Option<i64>,
    },
    LanguageChanged {
        locale: String,
    },
}

#[derive(Clone)]
pub struct StateEngine {
    state: Arc<RwLock<OutputState>>,
    watch_tx: Arc<watch::Sender<OutputState>>,
    event_tx: broadcast::Sender<CoreEvent>,
}

impl Default for StateEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl StateEngine {
    pub fn new() -> Self {
        let initial = OutputState::default();
        let (watch_tx, _) = watch::channel(initial.clone());
        let (event_tx, _) = broadcast::channel(256);
        Self {
            state: Arc::new(RwLock::new(initial)),
            watch_tx: Arc::new(watch_tx),
            event_tx,
        }
    }

    pub fn snapshot(&self) -> OutputState {
        self.state.read().expect("state lock poisoned").clone()
    }

    pub fn subscribe_watch(&self) -> watch::Receiver<OutputState> {
        self.watch_tx.subscribe()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<CoreEvent> {
        self.event_tx.subscribe()
    }

    pub fn notify_language_changed(&self, locale: impl Into<String>) {
        let _ = self.event_tx.send(CoreEvent::LanguageChanged {
            locale: locale.into(),
        });
    }

    pub fn dispatch(&self, cmd: CoreCommand) -> OutputState {
        let mut extra_event: Option<CoreEvent> = None;
        let updated = {
            let mut st = self.state.write().expect("state lock poisoned");
            let mut changed = true;

            match cmd {
                CoreCommand::PresentItem { item, start_slide } => {
                    let max_idx = item.slides.len().saturating_sub(1);
                    extra_event = Some(CoreEvent::ItemExecuted {
                        service_item_id: item.service_item_id,
                        song_id: item.song_id,
                    });
                    st.current_slide_index = start_slide.min(max_idx);
                    st.current_item = Some(*item);
                    st.display_mode = DisplayMode::Normal;
                    st.auto_play_elapsed_secs = 0;
                }
                CoreCommand::NextSlide => {
                    if let Some(item) = &st.current_item {
                        if st.current_slide_index + 1 < item.slides.len() {
                            st.current_slide_index += 1;
                            st.auto_play_elapsed_secs = 0;
                        } else if st.auto_play_enabled && !item.slides.is_empty() {
                            st.current_slide_index = 0;
                            st.auto_play_elapsed_secs = 0;
                        } else {
                            changed = false;
                        }
                    } else {
                        changed = false;
                    }
                }
                CoreCommand::PrevSlide => {
                    if st.current_item.is_some() && st.current_slide_index > 0 {
                        st.current_slide_index -= 1;
                        st.auto_play_elapsed_secs = 0;
                    } else {
                        changed = false;
                    }
                }
                CoreCommand::FirstSlide => {
                    if st.current_item.is_some() && st.current_slide_index != 0 {
                        st.current_slide_index = 0;
                        st.auto_play_elapsed_secs = 0;
                    } else {
                        changed = false;
                    }
                }
                CoreCommand::LastSlide => {
                    if let Some(item) = &st.current_item {
                        let last = item.slides.len().saturating_sub(1);
                        if st.current_slide_index != last {
                            st.current_slide_index = last;
                            st.auto_play_elapsed_secs = 0;
                        } else {
                            changed = false;
                        }
                    } else {
                        changed = false;
                    }
                }
                CoreCommand::GotoSlide { index } => {
                    if let Some(item) = &st.current_item {
                        if index < item.slides.len() {
                            st.current_slide_index = index;
                            st.display_mode = DisplayMode::Normal;
                            st.auto_play_elapsed_secs = 0;
                        } else {
                            changed = false;
                        }
                    } else {
                        changed = false;
                    }
                }
                CoreCommand::SetDisplayMode { mode } => {
                    st.display_mode = mode;
                }
                CoreCommand::ToggleBlackScreen => {
                    st.display_mode = if st.display_mode == DisplayMode::BlackScreen {
                        DisplayMode::Normal
                    } else {
                        DisplayMode::BlackScreen
                    };
                }
                CoreCommand::ToggleLogo => {
                    st.display_mode = if st.display_mode == DisplayMode::Logo {
                        DisplayMode::Normal
                    } else {
                        DisplayMode::Logo
                    };
                }
                CoreCommand::ToggleClearText => {
                    st.display_mode = if st.display_mode == DisplayMode::ClearText {
                        DisplayMode::Normal
                    } else {
                        DisplayMode::ClearText
                    };
                }
                CoreCommand::ClearPresentation => {
                    st.current_item = None;
                    st.current_slide_index = 0;
                    st.display_mode = DisplayMode::Normal;
                    st.auto_play_elapsed_secs = 0;
                }
                CoreCommand::SetMainTheme { theme } => {
                    st.main_theme = (*theme).clone();
                    if let Some(item) = &mut st.current_item {
                        item.theme = *theme;
                    }
                }
                CoreCommand::SetReturnTheme { theme } => {
                    st.return_theme = *theme;
                }
                CoreCommand::SetLogoPath { path } => {
                    st.logo_path = path;
                }
                CoreCommand::SetAutoPlay {
                    enabled,
                    default_duration_secs,
                } => {
                    st.auto_play_enabled = enabled;
                    st.auto_play_elapsed_secs = 0;
                    if let Some(d) = default_duration_secs {
                        st.default_slide_duration_secs = d.max(1);
                    }
                }
                CoreCommand::SetAutoPlayRepeat { repeat } => {
                    st.auto_play_repeat = repeat;
                }
                CoreCommand::StartCountdown {
                    label,
                    seconds,
                    show_on_main,
                } => {
                    st.countdown = CountdownState {
                        active: seconds > 0,
                        text_after: label.clone(),
                        label,
                        text_before: String::new(),
                        pad_zero: true,
                        hide_zero_min: false,
                        remaining_secs: seconds,
                        total_secs: seconds,
                        show_on_main,
                    };
                }
                CoreCommand::StartStudioCountdown {
                    seconds,
                    text_before,
                    text_after,
                    pad_zero,
                    hide_zero_min,
                    show_on_main,
                } => {
                    let combined_label = match (text_before.trim(), text_after.trim()) {
                        ("", "") => String::new(),
                        (b, "") => b.to_string(),
                        ("", a) => a.to_string(),
                        (b, a) => format!("{b} — {a}"),
                    };
                    st.countdown = CountdownState {
                        active: seconds > 0,
                        label: combined_label,
                        text_before,
                        text_after,
                        pad_zero,
                        hide_zero_min,
                        remaining_secs: seconds,
                        total_secs: seconds,
                        show_on_main,
                    };
                    if show_on_main {
                        st.display_mode = DisplayMode::Normal;
                    }
                }
                CoreCommand::StopCountdown => {
                    st.countdown.active = false;
                    st.countdown.show_on_main = false;
                }
                CoreCommand::UpdateCurrentItemNotes { notes } => {
                    if let Some(item) = &mut st.current_item {
                        item.notes = notes;
                    } else {
                        changed = false;
                    }
                }
                CoreCommand::UpdateScoreboard { scoreboard } => {
                    st.scoreboard = scoreboard;
                }
                CoreCommand::IncrementScore {
                    team_a_delta,
                    team_b_delta,
                } => {
                    st.scoreboard.team_a_score =
                        (st.scoreboard.team_a_score + team_a_delta).max(0);
                    st.scoreboard.team_b_score =
                        (st.scoreboard.team_b_score + team_b_delta).max(0);
                }
                CoreCommand::SetStageAlert { message } => {
                    st.stage_alert = message.filter(|m| !m.trim().is_empty());
                }
                CoreCommand::StartStageTimer => {
                    st.stage_timer_running = true;
                }
                CoreCommand::PauseStageTimer => {
                    st.stage_timer_running = false;
                }
                CoreCommand::ResetStageTimer => {
                    st.stage_elapsed_secs = 0;
                }
                CoreCommand::TickSecond => {
                    changed = false;
                    // Relógio na saída: redesenha a cada segundo quando ativo.
                    if st.main_theme.show_clock {
                        changed = true;
                    }
                    if st.countdown.active {
                        if st.countdown.remaining_secs > 0 {
                            st.countdown.remaining_secs -= 1;
                            changed = true;
                        }
                        if st.countdown.remaining_secs == 0 {
                            st.countdown.active = false;
                            changed = true;
                        }
                    }
                    if st.stage_timer_running {
                        st.stage_elapsed_secs += 1;
                        changed = true;
                    }
                    if st.auto_play_enabled && st.display_mode == DisplayMode::Normal {
                        let slide_info = st.current_item.as_ref().and_then(|item| {
                            if item.slides.is_empty() {
                                None
                            } else {
                                let dur = item
                                    .slides
                                    .get(st.current_slide_index)
                                    .and_then(|s| s.duration_secs)
                                    .unwrap_or(st.default_slide_duration_secs)
                                    .max(1);
                                Some((item.slides.len(), dur))
                            }
                        });
                        if let Some((slides_len, target_dur)) = slide_info {
                            st.auto_play_elapsed_secs += 1;
                            if st.auto_play_elapsed_secs >= target_dur {
                                st.auto_play_elapsed_secs = 0;
                                let at_end = st.current_slide_index + 1 >= slides_len;
                                if at_end && !st.auto_play_repeat {
                                    st.auto_play_enabled = false;
                                } else {
                                    st.current_slide_index = (st.current_slide_index + 1) % slides_len;
                                }
                                changed = true;
                            }
                        }
                    }
                }
            }

            if changed {
                st.revision += 1;
                st.dispatched_at_epoch_us = now_epoch_us();
            }
            st.clone()
        };

        let _ = self.watch_tx.send(updated.clone());
        if let Some(ev) = extra_event {
            let _ = self.event_tx.send(ev);
        }
        let _ = self.event_tx.send(CoreEvent::OutputUpdated {
            state: Box::new(updated.clone()),
        });
        updated
    }
}

pub fn now_epoch_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

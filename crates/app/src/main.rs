#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use chrono::Timelike;
use directories::ProjectDirs;
use overture_control::{ControlState, start_control_server};
use overture_core::{
    AppSettings, BackgroundSpec, BibleVerse, BibleVersion, CoreCommand, DisplayMode,
    HorizontalAlign, LivePresentationItem, MediaItem, MediaKind, OutputRole, PresentationSlide,
    RgbaColor, Service, ServiceItem, ServiceItemKind, ServiceItemParams, Song, SongSlide,
    StateEngine, Theme, ThemeConfig,
};
use overture_i18n::{I18n, Locale};
use overture_import::{
    DEFAULT_MAX_LINE_CHARS, OvertureJsonBundle, export_json_bundle_to_file,
    export_songs_to_txt_directory, import_json_bundle_from_file, parse_openlp_file,
    parse_opensong_file, parse_pptx_file, parse_txt_directory, parse_zefania_bible_xml,
    split_lyrics_into_slides,
};
use overture_media::{MediaCommand, MediaEngine};
use overture_output::{
    OVERTURE_APP_USER_MODEL_ID, OutputManager, RoleWindowTarget, enumerate_monitors,
    init_windows_process_identity,
};
use overture_render::SlideCompositor;
use overture_storage::{
    CANONICAL_BIBLE_BOOKS, Storage, canonical_book_by_order, create_full_backup,
    restore_full_backup,
};
use overture_ui::{
    MainWindow, UiBibleBookGridRow, UiBibleBookTile, UiBibleChapterVerse, UiBibleSearchHit,
    UiMediaRow, UiNumberGridRow, UiPlaylistRow, UiReportRow, UiScheduleRow, UiSlideCard,
    UiSongRow, UiThemeRow, ui_strings_for_locale,
};
use slint::{
    ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode,
    VecModel,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

struct AppContext {
    rt: Arc<tokio::runtime::Runtime>,
    storage: Storage,
    db_path: PathBuf,
    media_dir: PathBuf,
    song_themes_path: PathBuf,
    engine: StateEngine,
    media_engine: MediaEngine,
    output_mgr: Option<OutputManager>,
    ctrl_state: ControlState,
    i18n: I18n,
    settings: AppSettings,
    persistent_service: Service,
    temp_service: Service,
    using_temp_service: bool,

    // Studio staged / history state
    staged_song: Option<Song>,
    session_song_history: VecDeque<Song>,
    song_theme_map: HashMap<i64, i64>,
    cached_themes: Vec<Theme>,
    active_theme_id: i64,

    // Holyrics 66-book Bible Studio state
    bible_versions: Vec<BibleVersion>,
    bible_version_idx: usize,
    bible_book_order: i32,
    bible_chapter: i32,
    bible_verse: i32,
    cached_chapter_verses: Vec<BibleVerse>,

    preview_compositor: SlideCompositor,
}

impl AppContext {
    fn active_service(&self) -> &Service {
        if self.using_temp_service {
            &self.temp_service
        } else {
            &self.persistent_service
        }
    }

    fn current_bible_version(&self) -> Option<&BibleVersion> {
        self.bible_versions.get(self.bible_version_idx)
    }

    fn current_bible_version_id(&self) -> i64 {
        self.current_bible_version().map(|v| v.id).unwrap_or(1)
    }

    fn current_bible_version_abbr(&self) -> String {
        self.current_bible_version()
            .map(|v| v.abbr.clone())
            .unwrap_or_else(|| "TB1917".to_string())
    }

    fn sync_background_video_for_theme(&self, theme: &ThemeConfig) {
        match &theme.background {
            BackgroundSpec::Video {
                path,
                loop_playback,
                ..
            } => {
                self.media_engine.send(MediaCommand::PlayBackgroundVideo {
                    path: path.clone(),
                    loop_playback: *loop_playback,
                });
            }
            _ => {
                self.media_engine.send(MediaCommand::Stop);
            }
        }
    }

    fn save_song_theme_bindings(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.song_theme_map) {
            let _ = std::fs::write(&self.song_themes_path, json);
        }
    }

    fn record_song_in_session_history(&mut self, song: &Song) {
        self.session_song_history.retain(|s| s.id != song.id);
        self.session_song_history.push_front(song.clone());
        while self.session_song_history.len() > 25 {
            self.session_song_history.pop_back();
        }
    }

    fn theme_for_song(&self, song_id: i64) -> ThemeConfig {
        if let Some(&tid) = self.song_theme_map.get(&song_id)
            && let Some(t) = self.cached_themes.iter().find(|t| t.id == tid)
        {
            return t.config.clone();
        }
        self.engine.snapshot().main_theme
    }
}

fn main() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    init_windows_process_identity(OVERTURE_APP_USER_MODEL_ID);

    let args: Vec<String> = std::env::args().collect();
    let is_smoke_test = args.iter().any(|a| a == "--smoke-test");

    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?,
    );

    let (db_path, media_dir) = resolve_app_paths(is_smoke_test)?;
    std::fs::create_dir_all(&media_dir)?;
    let song_themes_path = db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("song_themes.json");
    let song_theme_map: HashMap<i64, i64> = std::fs::read_to_string(&song_themes_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let storage = rt.block_on(Storage::connect_file(&db_path))?;
    rt.block_on(seed_initial_demo_data_if_empty(&storage))?;

    let settings = rt.block_on(storage.load_settings()).unwrap_or_default();
    let locale = Locale::from_code(&settings.locale);
    let i18n = I18n::new(locale);

    let engine = StateEngine::new();
    let themes = rt.block_on(storage.list_themes()).unwrap_or_default();
    let mut initial_theme_id = 1i64;
    if let Some(main_t) = themes.iter().find(|t| !t.is_return_theme) {
        initial_theme_id = main_t.id;
        engine.dispatch(CoreCommand::SetMainTheme {
            theme: Box::new(main_t.config.clone()),
        });
    }
    if let Some(ret_t) = themes.iter().find(|t| t.is_return_theme) {
        engine.dispatch(CoreCommand::SetReturnTheme {
            theme: Box::new(ret_t.config.clone()),
        });
    }

    let bundled_logo = find_bundled_logo();
    engine.dispatch(CoreCommand::SetLogoPath {
        path: settings.logo_path.clone().or(bundled_logo),
    });

    let services = rt.block_on(storage.list_services(false))?;
    let persistent_service = match services.into_iter().next() {
        Some(s) => s,
        None => {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            rt.block_on(storage.create_service(&today, "Culto Dominical"))?
        }
    };

    let bible_versions = rt
        .block_on(storage.list_bible_versions())
        .unwrap_or_default();

    let media_engine = MediaEngine::spawn(640, 360);
    if let Some(main_t) = themes.iter().find(|t| t.id == initial_theme_id)
        && let BackgroundSpec::Video {
            path,
            loop_playback,
            ..
        } = &main_t.config.background
    {
        media_engine.send(MediaCommand::PlayBackgroundVideo {
            path: path.clone(),
            loop_playback: *loop_playback,
        });
    }

    let ctrl_state = ControlState::new(
        engine.clone(),
        storage.clone(),
        settings.api_token.clone(),
    );

    let bind_port = if is_smoke_test { 0 } else { settings.api_port };
    let bind_addr: SocketAddr = format!("0.0.0.0:{bind_port}").parse()?;
    let ctrl_handle = rt.block_on(start_control_server(bind_addr, ctrl_state.clone()))?;
    let lan_url = ctrl_handle
        .lan_urls()
        .into_iter()
        .next()
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", ctrl_handle.bound_addr.port()));

    // Execution history logger on background runtime
    {
        let mut ev_rx = engine.subscribe_events();
        let store_log = storage.clone();
        rt.spawn(async move {
            while let Ok(ev) = ev_rx.recv().await {
                if let overture_core::CoreEvent::ItemExecuted {
                    service_item_id,
                    song_id,
                } = ev
                {
                    let _ = store_log.record_execution(service_item_id, song_id).await;
                }
            }
        });
    }

    let ui = MainWindow::new()?;
    ui.set_tr(ui_strings_for_locale(locale));
    ui.set_settings_locale_index(if locale == Locale::En { 1 } else { 0 });
    ui.set_settings_api_port(settings.api_port.to_string().into());
    ui.set_settings_api_token(settings.api_token.clone().into());
    ui.set_remote_url_hint(lan_url.into());
    ui.set_shortcut_next(settings.shortcuts.next_slide.clone().into());
    ui.set_shortcut_prev(settings.shortcuts.prev_slide.clone().into());
    ui.set_shortcut_black(settings.shortcuts.black_screen.clone().into());
    ui.set_shortcut_logo(settings.shortcuts.logo_screen.clone().into());

    populate_monitor_options(&ui);

    let output_mgr = if is_smoke_test {
        None
    } else {
        OutputManager::spawn(
            engine.subscribe_watch(),
            Some(media_engine.subscribe_frames()),
            vec![],
        )
        .ok()
    };

    let ctx = Rc::new(RefCell::new(AppContext {
        rt: rt.clone(),
        storage,
        db_path,
        media_dir,
        song_themes_path,
        engine,
        media_engine,
        output_mgr,
        ctrl_state,
        i18n,
        settings,
        persistent_service,
        temp_service: Service::new_temporary("Temporário"),
        using_temp_service: false,
        staged_song: None,
        session_song_history: VecDeque::new(),
        song_theme_map,
        cached_themes: themes,
        active_theme_id: initial_theme_id,
        bible_versions,
        bible_version_idx: 0,
        bible_book_order: 1,
        bible_chapter: 1,
        bible_verse: 1,
        cached_chapter_verses: Vec::new(),
        preview_compositor: SlideCompositor::new(),
    }));

    // Stage and present the first song on startup so Column 2 & Column 3 look immediately ready like Holyrics
    {
        let mut c = ctx.borrow_mut();
        refresh_all_ui_models(&ui, &mut c);
        if let Ok(songs) = c.rt.block_on(c.storage.search_songs("", false, None, 1))
            && let Some(first) = songs.into_iter().next()
        {
            stage_and_optionally_project_song(&ui, &mut c, first, true, 0);
        }
    }

    if is_smoke_test {
        println!("OVERTURE_SMOKE_OK");
        ctrl_handle.shutdown();
        ctx.borrow().media_engine.shutdown();
        return Ok(());
    }

    wire_callbacks(&ui, ctx.clone());

    // 200ms UI live preview + GPU telemetry refresh timer
    let preview_timer = Timer::default();
    {
        let ui_weak = ui.as_weak();
        let ctx_t = ctx.clone();
        preview_timer.start(TimerMode::Repeated, Duration::from_millis(200), move || {
            if let Some(win) = ui_weak.upgrade() {
                update_live_preview_and_telemetry(&win, &mut ctx_t.borrow_mut());
            }
        });
    }

    // 1000ms heartbeat timer for countdown, stage chronometer, and slide auto-advance
    let sec_timer = Timer::default();
    {
        let ui_weak = ui.as_weak();
        let ctx_s = ctx.clone();
        sec_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let mut c = ctx_s.borrow_mut();
            c.engine.dispatch(CoreCommand::TickSecond);
            if let Some(win) = ui_weak.upgrade() {
                sync_output_state_to_ui(&win, &mut c);
            }
        });
    }

    ui.run()?;

    ctrl_handle.shutdown();
    let mut final_ctx = ctx.borrow_mut();
    final_ctx.media_engine.shutdown();
    if let Some(mgr) = final_ctx.output_mgr.take() {
        mgr.shutdown();
    }
    Ok(())
}

fn populate_monitor_options(ui: &MainWindow) {
    let monitors = enumerate_monitors();
    let mut labels: Vec<SharedString> = vec!["Desativado".into()];
    if monitors.is_empty() {
        labels.push("Monitor 1 — Principal (1920×1080)".into());
    } else {
        for m in &monitors {
            labels.push(m.friendly_name.clone().into());
        }
    }
    ui.set_monitor_options(ModelRc::new(VecModel::from(labels)));
}

fn wire_callbacks(ui: &MainWindow, ctx: Rc<RefCell<AppContext>>) {
    // ==================== KEYBOARD SHORTCUTS ====================
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_key_shortcut(move |key_str| {
            let mut c = ctx.borrow_mut();
            let k = normalize_key_name(key_str.as_str());
            if eq_key(&k, &c.settings.shortcuts.next_slide) {
                c.engine.dispatch(CoreCommand::NextSlide);
            } else if eq_key(&k, &c.settings.shortcuts.prev_slide) {
                c.engine.dispatch(CoreCommand::PrevSlide);
            } else if eq_key(&k, &c.settings.shortcuts.black_screen) || eq_key(&k, "B") {
                c.engine.dispatch(CoreCommand::ToggleBlackScreen);
            } else if eq_key(&k, &c.settings.shortcuts.logo_screen) || eq_key(&k, "L") {
                c.engine.dispatch(CoreCommand::ToggleLogo);
            } else if eq_key(&k, &c.settings.shortcuts.clear_text) || eq_key(&k, "C") {
                c.engine.dispatch(CoreCommand::ToggleClearText);
            } else if eq_key(&k, &c.settings.shortcuts.first_slide) {
                c.engine.dispatch(CoreCommand::FirstSlide);
            } else if eq_key(&k, &c.settings.shortcuts.last_slide) {
                c.engine.dispatch(CoreCommand::LastSlide);
            } else {
                return;
            }
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }

    // ==================== TRANSPORT & LIVE MODES ====================
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_next_slide(move || {
            let mut c = ctx.borrow_mut();
            c.engine.dispatch(CoreCommand::NextSlide);
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_prev_slide(move || {
            let mut c = ctx.borrow_mut();
            c.engine.dispatch(CoreCommand::PrevSlide);
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_goto_slide(move |idx| {
            if idx < 0 {
                return;
            }
            let mut c = ctx.borrow_mut();
            let target_idx = idx as usize;

            // If a staged song in Column 2 is different from the currently live item, promote it live at `target_idx`!
            let promote_staged = match (&c.staged_song, &c.engine.snapshot().current_item) {
                (Some(staged), Some(live)) => live.song_id != Some(staged.id),
                (Some(_), None) => true,
                _ => false,
            };

            if promote_staged {
                if let Some(song) = c.staged_song.clone()
                    && let Some(w) = ui_weak.upgrade()
                {
                    stage_and_optionally_project_song(&w, &mut c, song, true, target_idx);
                    return;
                }
            }

            c.engine
                .dispatch(CoreCommand::GotoSlide { index: target_idx });
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_toggle_black(move || {
            let mut c = ctx.borrow_mut();
            c.engine.dispatch(CoreCommand::ToggleBlackScreen);
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_toggle_logo(move || {
            let mut c = ctx.borrow_mut();
            c.engine.dispatch(CoreCommand::ToggleLogo);
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_toggle_clear_text(move || {
            let mut c = ctx.borrow_mut();
            c.engine.dispatch(CoreCommand::ToggleClearText);
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_clear_output(move || {
            let mut c = ctx.borrow_mut();
            c.engine.dispatch(CoreCommand::ClearPresentation);
            c.engine.dispatch(CoreCommand::StopCountdown);
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_toggle_auto_play(move |enabled| {
            let mut c = ctx.borrow_mut();
            let dur = c.settings.auto_advance_default_secs.or(Some(6));
            c.engine.dispatch(CoreCommand::SetAutoPlay {
                enabled,
                default_duration_secs: dur,
            });
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }

    // ==================== LIBRARY & SONGS (COL 1 + COL 2) ====================
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_search_songs(move |q| {
            let c = ctx.borrow();
            if let Some(w) = ui_weak.upgrade() {
                refresh_songs_ui(&w, &c, q.as_str(), w.get_show_archived());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_toggle_show_archived(move |show_arch| {
            let c = ctx.borrow();
            if let Some(w) = ui_weak.upgrade() {
                refresh_songs_ui(&w, &c, w.get_library_search_query().as_str(), show_arch);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_song_select(move |song_id| {
            let mut c = ctx.borrow_mut();
            if let (Some(w), Ok(Some(song))) = (
                ui_weak.upgrade(),
                c.rt.block_on(c.storage.get_song(song_id as i64)),
            ) {
                stage_and_optionally_project_song(&w, &mut c, song, false, 0);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_song_project_now(move |song_id| {
            let mut c = ctx.borrow_mut();
            if song_id <= 0 {
                if let Some(staged) = c.staged_song.clone()
                    && let Some(w) = ui_weak.upgrade()
                {
                    stage_and_optionally_project_song(&w, &mut c, staged, true, 0);
                }
                return;
            }
            if let (Some(w), Ok(Some(song))) = (
                ui_weak.upgrade(),
                c.rt.block_on(c.storage.get_song(song_id as i64)),
            ) {
                w.set_workspace_mode(0);
                stage_and_optionally_project_song(&w, &mut c, song, true, 0);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_song_add_to_service(move |song_id| {
            let mut c = ctx.borrow_mut();
            if let Ok(Some(song)) = c.rt.block_on(c.storage.get_song(song_id as i64)) {
                let item = ServiceItem {
                    id: 0,
                    service_id: c.active_service().id,
                    order: c.active_service().items.len() as i32,
                    kind: ServiceItemKind::Song,
                    ref_id: Some(song.id),
                    title: song.title.clone(),
                    subtitle: song.artist,
                    params: ServiceItemParams::default(),
                };
                append_item_to_active_service(&mut c, item);
                if let Some(w) = ui_weak.upgrade() {
                    refresh_playlist_ui(&w, &c);
                    w.set_status_banner(format!("'{}' adicionada ao culto.", song.title).into());
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_song_set_archived(move |song_id, archived| {
            let c = ctx.borrow();
            let _ = c
                .rt
                .block_on(c.storage.set_song_archived(song_id as i64, archived));
            if let Some(w) = ui_weak.upgrade() {
                refresh_songs_ui(
                    &w,
                    &c,
                    w.get_library_search_query().as_str(),
                    w.get_show_archived(),
                );
                w.set_status_banner(
                    if archived {
                        "Música arquivada."
                    } else {
                        "Música desarquivada."
                    }
                    .into(),
                );
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_deduplicate_songs(move || {
            let c = ctx.borrow();
            let removed = c.rt.block_on(c.storage.remove_exact_duplicates()).unwrap_or(0);
            if let Some(w) = ui_weak.upgrade() {
                refresh_songs_ui(
                    &w,
                    &c,
                    w.get_library_search_query().as_str(),
                    w.get_show_archived(),
                );
                w.set_status_banner(format!("{removed} duplicatas exatas removidas.").into());
            }
        });
    }

    // ==================== QUICK TEXT / ANNOUNCEMENTS ====================
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_project_quick_text(move |title, body| {
            if body.trim().is_empty() {
                return;
            }
            let mut c = ctx.borrow_mut();
            let parsed = split_lyrics_into_slides(body.as_str(), DEFAULT_MAX_LINE_CHARS);
            let slides: Vec<PresentationSlide> = parsed
                .into_iter()
                .enumerate()
                .map(|(idx, s)| PresentationSlide {
                    index: idx,
                    text: s.text,
                    footer: None,
                    forced_break: s.forced_break,
                    duration_secs: None,
                })
                .collect();
            let ttl = if title.trim().is_empty() {
                "Aviso".to_string()
            } else {
                title.trim().to_string()
            };
            let theme = c.engine.snapshot().main_theme;
            c.staged_song = None;
            c.engine.dispatch(CoreCommand::PresentItem {
                item: Box::new(LivePresentationItem {
                    service_item_id: None,
                    song_id: None,
                    title: ttl,
                    subtitle: "Texto Rápido".to_string(),
                    notes: String::new(),
                    kind: ServiceItemKind::Announcement,
                    slides,
                    theme,
                    media_path: None,
                    media_kind: None,
                }),
                start_slide: 0,
            });
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_add_quick_text_to_service(move |title, body| {
            if body.trim().is_empty() {
                return;
            }
            let mut c = ctx.borrow_mut();
            let slides: Vec<String> =
                split_lyrics_into_slides(body.as_str(), DEFAULT_MAX_LINE_CHARS)
                    .into_iter()
                    .map(|s| s.text)
                    .collect();
            let ttl = if title.trim().is_empty() {
                body.lines().next().unwrap_or("Aviso").to_string()
            } else {
                title.trim().to_string()
            };
            let item = ServiceItem {
                id: 0,
                service_id: c.active_service().id,
                order: c.active_service().items.len() as i32,
                kind: ServiceItemKind::Announcement,
                ref_id: None,
                title: ttl,
                subtitle: "Aviso".to_string(),
                params: ServiceItemParams {
                    custom_slides: slides,
                    ..Default::default()
                },
            };
            append_item_to_active_service(&mut c, item);
            if let Some(w) = ui_weak.upgrade() {
                refresh_playlist_ui(&w, &c);
                w.set_status_banner("Texto rápido adicionado à ordem do culto.".into());
            }
        });
    }

    // ==================== MEDIA & NATIVE FILE DIALOGS (`rfd`) ====================
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_pick_and_add_media(move || {
            let files = rfd::FileDialog::new()
                .set_title("Adicionar Mídias (Áudio, Vídeo ou Imagem)")
                .add_filter(
                    "Mídias Suportadas",
                    &[
                        "mp4", "mkv", "webm", "mov", "avi", "mp3", "wav", "ogg", "flac", "m4a",
                        "png", "jpg", "jpeg", "webp", "bmp",
                    ],
                )
                .pick_files();
            let Some(paths) = files else {
                return;
            };
            let c = ctx.borrow();
            let mut added = 0usize;
            for p in paths {
                let kind = p
                    .extension()
                    .and_then(|e| MediaKind::from_extension(&e.to_string_lossy()))
                    .unwrap_or(MediaKind::Video);
                let name = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Mídia".to_string());
                if c.rt
                    .block_on(c.storage.upsert_media(&MediaItem {
                        id: 0,
                        name,
                        path: p.to_string_lossy().to_string(),
                        kind,
                        duration_secs: None,
                        archived: false,
                    }))
                    .is_ok()
                {
                    added += 1;
                }
            }
            if let Some(w) = ui_weak.upgrade() {
                refresh_media_ui(&w, &c);
                w.set_status_banner(format!("{added} mídia(s) adicionada(s) à biblioteca.").into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_media_project_now(move |media_id| {
            let mut c = ctx.borrow_mut();
            if let Ok(list) = c.rt.block_on(c.storage.list_media(false))
                && let Some(m) = list.into_iter().find(|x| x.id == media_id as i64)
            {
                c.staged_song = None;
                c.media_engine.send(MediaCommand::PlayMedia {
                    path: m.path.clone(),
                    kind: m.kind,
                    loop_playback: m.kind == MediaKind::Video,
                });
                let theme = c.engine.snapshot().main_theme;
                c.engine.dispatch(CoreCommand::PresentItem {
                    item: Box::new(LivePresentationItem {
                        service_item_id: None,
                        song_id: None,
                        title: m.name.clone(),
                        subtitle: m.kind.as_str().to_uppercase(),
                        notes: String::new(),
                        kind: ServiceItemKind::Media,
                        slides: vec![PresentationSlide {
                            index: 0,
                            text: if m.kind == MediaKind::Audio {
                                format!("♪ {}", m.name)
                            } else {
                                String::new()
                            },
                            footer: None,
                            forced_break: false,
                            duration_secs: None,
                        }],
                        theme,
                        media_path: Some(m.path),
                        media_kind: Some(m.kind),
                    }),
                    start_slide: 0,
                });
                if let Some(w) = ui_weak.upgrade() {
                    sync_output_state_to_ui(&w, &mut c);
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_media_add_service(move |media_id| {
            let mut c = ctx.borrow_mut();
            if let Ok(list) = c.rt.block_on(c.storage.list_media(false))
                && let Some(m) = list.into_iter().find(|x| x.id == media_id as i64)
            {
                let item = ServiceItem {
                    id: 0,
                    service_id: c.active_service().id,
                    order: c.active_service().items.len() as i32,
                    kind: ServiceItemKind::Media,
                    ref_id: Some(m.id),
                    title: m.name.clone(),
                    subtitle: m.kind.as_str().to_uppercase(),
                    params: ServiceItemParams::default(),
                };
                append_item_to_active_service(&mut c, item);
                if let Some(w) = ui_weak.upgrade() {
                    refresh_playlist_ui(&w, &c);
                    w.set_status_banner(format!("Mídia '{}' adicionada ao culto.", m.name).into());
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_media_remove(move |media_id| {
            let c = ctx.borrow();
            let _ = c
                .rt
                .block_on(c.storage.set_media_archived(media_id as i64, true));
            if let Some(w) = ui_weak.upgrade() {
                refresh_media_ui(&w, &c);
            }
        });
    }

    // Native File Dialogs (`rfd`)
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_dialog_import_txt_folder(move || {
            let Some(folder) = rfd::FileDialog::new()
                .set_title("Selecionar Pasta com Letras .TXT")
                .pick_folder()
            else {
                return;
            };
            let c = ctx.borrow();
            let status = match parse_txt_directory(&folder, "user_txt", DEFAULT_MAX_LINE_CHARS) {
                Ok((songs, report)) => {
                    let mut db_inserted = 0usize;
                    let mut db_skipped = 0usize;
                    for s in &songs {
                        if let Ok(outcome) = c.rt.block_on(c.storage.insert_song(s, true)) {
                            match outcome {
                                overture_storage::SongInsertOutcome::Inserted { .. } => {
                                    db_inserted += 1
                                }
                                overture_storage::SongInsertOutcome::SkippedExactDuplicate {
                                    ..
                                } => db_skipped += 1,
                            }
                        }
                    }
                    format!(
                        "Importação TXT concluída: {} músicas | {} com quebra automática (>38c) | {} duplicatas ignoradas",
                        db_inserted,
                        report.forced_break_songs,
                        report.exact_duplicates_removed + db_skipped
                    )
                }
                Err(e) => format!("Erro ao importar pasta TXT: {e}"),
            };
            if let Some(w) = ui_weak.upgrade() {
                refresh_songs_ui(
                    &w,
                    &c,
                    w.get_library_search_query().as_str(),
                    w.get_show_archived(),
                );
                w.set_status_banner(status.into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_dialog_import_pptx_or_xml(move || {
            let Some(paths) = rfd::FileDialog::new()
                .set_title("Importar Apresentação (.pptx), OpenLP/OpenSong (.xml) ou Pacote (.json)")
                .add_filter("Apresentações e Letras", &["pptx", "xml", "json"])
                .pick_files()
            else {
                return;
            };
            let mut c = ctx.borrow_mut();
            let mut summary = String::new();
            for path in paths {
                summary = import_single_file(&mut c, &path);
            }
            if let Some(w) = ui_weak.upgrade() {
                refresh_all_ui_models(&w, &mut c);
                if !summary.is_empty() {
                    w.set_status_banner(summary.into());
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_dialog_import_bible(move || {
            let Some(path) = rfd::FileDialog::new()
                .set_title("Importar Bíblia Zefania XML")
                .add_filter("Bíblia XML", &["xml"])
                .pick_file()
            else {
                return;
            };
            let mut c = ctx.borrow_mut();
            let msg = match std::fs::read_to_string(&path) {
                Ok(raw) => match parse_zefania_bible_xml(&raw, "IMPORT", "pt-BR", "Importada pelo Usuário") {
                    Ok((ver, verses)) => match c.rt.block_on(c.storage.import_bible_version(&ver, &verses)) {
                        Ok(_) => {
                            c.bible_versions = c.rt.block_on(c.storage.list_bible_versions()).unwrap_or_default();
                            format!("Bíblia '{}' ({} versículos) importada com sucesso!", ver.name, verses.len())
                        }
                        Err(e) => format!("Erro ao gravar Bíblia no SQLite: {e}"),
                    },
                    Err(e) => format!("Formato Zefania XML inválido: {e}"),
                },
                Err(e) => format!("Falha ao ler arquivo XML: {e}"),
            };
            if let Some(w) = ui_weak.upgrade() {
                refresh_bible_studio_ui(&w, &mut c);
                w.set_status_banner(msg.into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_dialog_export_txt(move || {
            let Some(out_dir) = rfd::FileDialog::new()
                .set_title("Escolher Pasta para Exportar Letras em .TXT")
                .pick_folder()
            else {
                return;
            };
            let c = ctx.borrow();
            let songs = c
                .rt
                .block_on(c.storage.search_songs("", false, None, 10_000))
                .unwrap_or_default();
            let msg = match export_songs_to_txt_directory(&songs, &out_dir) {
                Ok(files) => format!(
                    "{} músicas exportadas em TXT para {}",
                    files.len(),
                    out_dir.display()
                ),
                Err(e) => format!("Erro na exportação TXT: {e}"),
            };
            if let Some(w) = ui_weak.upgrade() {
                w.set_status_banner(msg.into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_dialog_export_json(move || {
            let Some(dest) = rfd::FileDialog::new()
                .set_title("Exportar Biblioteca Completa (JSON Overture)")
                .set_file_name("overture-biblioteca.json")
                .add_filter("JSON Overture", &["json"])
                .save_file()
            else {
                return;
            };
            let c = ctx.borrow();
            let songs = c
                .rt
                .block_on(c.storage.search_songs("", true, None, 10_000))
                .unwrap_or_default();
            let groups = c.rt.block_on(c.storage.list_groups()).unwrap_or_default();
            let themes = c.rt.block_on(c.storage.list_themes()).unwrap_or_default();
            let services = c
                .rt
                .block_on(c.storage.list_services(true))
                .unwrap_or_default();
            let bundle = OvertureJsonBundle::new(songs, groups, themes, services);
            let msg = match export_json_bundle_to_file(&bundle, &dest) {
                Ok(()) => format!("Biblioteca exportada para {}", dest.display()),
                Err(e) => format!("Erro na exportação JSON: {e}"),
            };
            if let Some(w) = ui_weak.upgrade() {
                w.set_status_banner(msg.into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_dialog_create_backup(move || {
            let default_name = format!(
                "overture-backup-{}.zip",
                chrono::Local::now().format("%Y%m%d-%H%M")
            );
            let Some(dest) = rfd::FileDialog::new()
                .set_title("Salvar Backup Completo (.zip)")
                .set_file_name(&default_name)
                .add_filter("Arquivo ZIP", &["zip"])
                .save_file()
            else {
                return;
            };
            let c = ctx.borrow();
            let msg = match c
                .rt
                .block_on(create_full_backup(c.storage.pool(), &c.media_dir, &dest))
            {
                Ok(m) => format!(
                    "Backup completo salvo em {} ({} mídias empacotadas).",
                    dest.display(),
                    m.media_file_count
                ),
                Err(e) => format!("Falha ao gerar backup: {e}"),
            };
            if let Some(w) = ui_weak.upgrade() {
                w.set_status_banner(msg.into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_dialog_restore_backup(move || {
            let Some(src) = rfd::FileDialog::new()
                .set_title("Selecionar Backup Overture (.zip) para Restaurar")
                .add_filter("Arquivo ZIP", &["zip"])
                .pick_file()
            else {
                return;
            };
            let mut c = ctx.borrow_mut();
            c.rt.block_on(c.storage.close());
            let msg = match restore_full_backup(&src, &c.db_path, &c.media_dir) {
                Ok(_) => match c.rt.block_on(Storage::connect_file(&c.db_path)) {
                    Ok(reopened) => {
                        c.storage = reopened.clone();
                        c.ctrl_state.storage = reopened;
                        c.bible_versions = c
                            .rt
                            .block_on(c.storage.list_bible_versions())
                            .unwrap_or_default();
                        "Backup restaurado com sucesso (SQLite + mídias)!".to_string()
                    }
                    Err(e) => format!("Erro ao reabrir banco após restauração: {e}"),
                },
                Err(e) => format!("Erro na restauração: {e}"),
            };
            if let Some(w) = ui_weak.upgrade() {
                refresh_all_ui_models(&w, &mut c);
                w.set_status_banner(msg.into());
            }
        });
    }

    // ==================== PLAYLIST / CULTO (COL 4) ====================
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_toggle_playlist_mode(move |use_temp| {
            let mut c = ctx.borrow_mut();
            c.using_temp_service = use_temp;
            if let Some(w) = ui_weak.upgrade() {
                w.set_using_temp_playlist(use_temp);
                refresh_playlist_ui(&w, &c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_create_new_service(move || {
            let mut c = ctx.borrow_mut();
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let name = format!("Culto {}", chrono::Local::now().format("%d/%m %H:%M"));
            if let Ok(svc) = c.rt.block_on(c.storage.create_service(&today, &name)) {
                c.persistent_service = svc;
                c.using_temp_service = false;
                if let Some(w) = ui_weak.upgrade() {
                    w.set_using_temp_playlist(false);
                    refresh_playlist_ui(&w, &c);
                    w.set_status_banner(format!("Novo culto criado: {name}").into());
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_activate_playlist_item(move |row_idx| {
            if row_idx < 0 {
                return;
            }
            let mut c = ctx.borrow_mut();
            let Some(item) = c.active_service().items.get(row_idx as usize).cloned() else {
                return;
            };
            let fallback = item
                .ref_id
                .filter(|_| item.kind == ServiceItemKind::Song)
                .map(|sid| c.theme_for_song(sid))
                .unwrap_or_else(|| c.engine.snapshot().main_theme);

            if let Ok(live) = c.rt.block_on(c.storage.resolve_service_item(&item, &fallback)) {
                if let Some(sid) = live.song_id
                    && let Ok(Some(song)) = c.rt.block_on(c.storage.get_song(sid))
                {
                    c.record_song_in_session_history(&song);
                    c.staged_song = Some(song);
                } else {
                    c.staged_song = None;
                }

                if live.kind == ServiceItemKind::Media
                    && let (Some(path), Some(mk)) = (&live.media_path, live.media_kind)
                {
                    c.media_engine.send(MediaCommand::PlayMedia {
                        path: path.clone(),
                        kind: mk,
                        loop_playback: mk == MediaKind::Video,
                    });
                } else {
                    c.sync_background_video_for_theme(&live.theme);
                }

                if item.kind == ServiceItemKind::Countdown
                    && let Some(secs) = item.params.countdown_seconds
                {
                    c.engine.dispatch(CoreCommand::StartCountdown {
                        label: item.title.clone(),
                        seconds: secs,
                        show_on_main: true,
                    });
                }

                c.engine.dispatch(CoreCommand::PresentItem {
                    item: Box::new(live),
                    start_slide: 0,
                });

                if let Some(w) = ui_weak.upgrade() {
                    w.set_active_playlist_row(row_idx);
                    refresh_session_history_ui(&w, &c);
                    sync_output_state_to_ui(&w, &mut c);
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_move_playlist_item(move |row_idx, delta| {
            if row_idx < 0 {
                return;
            }
            let mut c = ctx.borrow_mut();
            let len = c.active_service().items.len() as i32;
            let dst = row_idx + delta;
            if dst < 0 || dst >= len || dst == row_idx {
                return;
            }
            if c.using_temp_service {
                c.temp_service.items.swap(row_idx as usize, dst as usize);
            } else {
                c.persistent_service
                    .items
                    .swap(row_idx as usize, dst as usize);
                let svc_id = c.persistent_service.id;
                let items = c.persistent_service.items.clone();
                if let Ok(saved) = c.rt.block_on(c.storage.replace_service_items(svc_id, &items)) {
                    c.persistent_service.items = saved;
                }
            }
            if let Some(w) = ui_weak.upgrade() {
                refresh_playlist_ui(&w, &c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_remove_playlist_item(move |row_idx| {
            if row_idx < 0 {
                return;
            }
            let mut c = ctx.borrow_mut();
            let idx = row_idx as usize;
            if c.using_temp_service {
                if idx < c.temp_service.items.len() {
                    c.temp_service.items.remove(idx);
                }
            } else if idx < c.persistent_service.items.len() {
                c.persistent_service.items.remove(idx);
                let svc_id = c.persistent_service.id;
                let items = c.persistent_service.items.clone();
                if let Ok(saved) = c.rt.block_on(c.storage.replace_service_items(svc_id, &items)) {
                    c.persistent_service.items = saved;
                }
            }
            if let Some(w) = ui_weak.upgrade() {
                refresh_playlist_ui(&w, &c);
            }
        });
    }

    // ==================== THEMES & STRIP (COL 5 + THEME STUDIO MODAL) ====================
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_apply_theme_live(move |theme_id| {
            let mut c = ctx.borrow_mut();
            let Some(t) = c
                .cached_themes
                .iter()
                .find(|x| x.id == theme_id as i64)
                .cloned()
            else {
                return;
            };
            if t.is_return_theme {
                c.engine.dispatch(CoreCommand::SetReturnTheme {
                    theme: Box::new(t.config.clone()),
                });
            } else {
                c.active_theme_id = t.id;
                c.sync_background_video_for_theme(&t.config);
                c.engine.dispatch(CoreCommand::SetMainTheme {
                    theme: Box::new(t.config.clone()),
                });
                if let Some(w) = ui_weak.upgrade() {
                    let sel_song = w.get_selected_song_id() as i64;
                    if w.get_save_theme_to_song() && sel_song > 0 {
                        c.song_theme_map.insert(sel_song, t.id);
                        c.save_song_theme_bindings();
                    }
                    update_active_theme_indicator(&w, &mut c);
                }
            }
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_open_theme_editor(move |theme_id| {
            let mut c = ctx.borrow_mut();
            let Some(t) = c
                .cached_themes
                .iter()
                .find(|x| x.id == theme_id as i64)
                .cloned()
            else {
                return;
            };
            if let Some(w) = ui_weak.upgrade() {
                populate_theme_editor_fields(&w, &t);
                render_theme_preview_into_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_pick_theme_media_file(move || {
            let Some(w) = ui_weak.upgrade() else {
                return;
            };
            let bg_kind = w.get_theme_bg_kind();
            let picked = if bg_kind == 2 {
                rfd::FileDialog::new()
                    .set_title("Selecionar Imagem de Fundo do Tema")
                    .add_filter("Imagens", &["png", "jpg", "jpeg", "webp", "bmp"])
                    .pick_file()
            } else {
                rfd::FileDialog::new()
                    .set_title("Selecionar Vídeo em Loop do Tema")
                    .add_filter("Vídeos", &["mp4", "mkv", "webm", "mov"])
                    .pick_file()
            };
            if let Some(path) = picked {
                w.set_theme_bg_media(path.to_string_lossy().as_ref().into());
                let mut c = ctx.borrow_mut();
                render_theme_preview_into_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_refresh_theme_preview(move || {
            let mut c = ctx.borrow_mut();
            if let Some(w) = ui_weak.upgrade() {
                render_theme_preview_into_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_save_current_theme(move || {
            let mut c = ctx.borrow_mut();
            let Some(w) = ui_weak.upgrade() else {
                return;
            };
            let cfg = read_theme_config_from_ui(&w);
            let name_str = w.get_theme_edit_name().trim().to_string();
            let theme = Theme {
                id: w.get_theme_edit_id() as i64,
                name: if name_str.is_empty() {
                    "Tema Personalizado".to_string()
                } else {
                    name_str
                },
                is_return_theme: w.get_theme_is_return(),
                config: cfg.clone(),
            };
            if let Ok(id) = c.rt.block_on(c.storage.save_theme(&theme)) {
                w.set_theme_edit_id(id as i32);
                if theme.is_return_theme {
                    c.engine
                        .dispatch(CoreCommand::SetReturnTheme { theme: Box::new(cfg) });
                } else {
                    c.active_theme_id = id;
                    c.sync_background_video_for_theme(&cfg);
                    c.engine
                        .dispatch(CoreCommand::SetMainTheme { theme: Box::new(cfg) });
                }
                refresh_themes_ui(&w, &mut c);
                sync_output_state_to_ui(&w, &mut c);
                w.set_status_banner(format!("Tema '{}' salvo e aplicado.", theme.name).into());
            }
        });
    }

    // ==================== SONG EDITOR MODAL ====================
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_song_open_editor(move |song_id| {
            let c = ctx.borrow();
            if let (Some(w), Ok(Some(song))) = (
                ui_weak.upgrade(),
                c.rt.block_on(c.storage.get_song(song_id as i64)),
            ) {
                w.set_editor_song_id(song.id as i32);
                w.set_editor_title(song.title.into());
                w.set_editor_artist(song.artist.into());
                w.set_editor_author(song.author.into());
                w.set_editor_notes(song.notes.into());
                let raw = song
                    .slides
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                w.set_editor_raw_lyrics(raw.into());
                w.set_editor_preview_slides(ModelRc::new(VecModel::from(slides_to_cards(
                    &song.slides,
                ))));
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_editor_new_song(move || {
            if let Some(w) = ui_weak.upgrade() {
                w.set_editor_song_id(0);
                w.set_editor_title("".into());
                w.set_editor_artist("".into());
                w.set_editor_author("".into());
                w.set_editor_notes("".into());
                w.set_editor_raw_lyrics("".into());
                w.set_editor_preview_slides(ModelRc::new(VecModel::from(
                    Vec::<UiSlideCard>::new(),
                )));
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_editor_auto_split(move || {
            if let Some(w) = ui_weak.upgrade() {
                let slides = split_lyrics_into_slides(
                    w.get_editor_raw_lyrics().as_str(),
                    DEFAULT_MAX_LINE_CHARS,
                );
                w.set_editor_preview_slides(ModelRc::new(VecModel::from(slides_to_cards(&slides))));
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_editor_save_song(move || {
            let Some(w) = ui_weak.upgrade() else {
                return;
            };
            let title = w.get_editor_title().trim().to_string();
            if title.is_empty() {
                w.set_status_banner("Informe o título da música antes de salvar.".into());
                return;
            }
            let slides = split_lyrics_into_slides(
                w.get_editor_raw_lyrics().as_str(),
                DEFAULT_MAX_LINE_CHARS,
            );
            let forced_count = slides.iter().filter(|s| s.forced_break).count();
            let mut song = Song::new_draft(title, w.get_editor_artist().trim(), "editor");
            song.id = w.get_editor_song_id() as i64;
            song.author = w.get_editor_author().trim().to_string();
            song.notes = w.get_editor_notes().trim().to_string();
            song.slides = slides;

            let mut c = ctx.borrow_mut();
            let saved_id = if song.id > 0 {
                let _ = c.rt.block_on(c.storage.update_song(&song));
                song.id
            } else {
                match c.rt.block_on(c.storage.insert_song(&song, false)) {
                    Ok(overture_storage::SongInsertOutcome::Inserted { id, .. }) => {
                        w.set_editor_song_id(id as i32);
                        id
                    }
                    _ => 0,
                }
            };

            refresh_songs_ui(
                &w,
                &c,
                w.get_library_search_query().as_str(),
                w.get_show_archived(),
            );
            if saved_id > 0
                && let Ok(Some(reloaded)) = c.rt.block_on(c.storage.get_song(saved_id))
            {
                let is_live = c
                    .engine
                    .snapshot()
                    .current_item
                    .and_then(|i| i.song_id)
                    == Some(saved_id);
                let cur_idx = c.engine.snapshot().current_slide_index;
                stage_and_optionally_project_song(&w, &mut c, reloaded, is_live, cur_idx);
            }
            w.set_status_banner(
                format!(
                    "Música '{}' salva ({} slides, {} quebras automáticas).",
                    song.title,
                    song.slides.len(),
                    forced_count
                )
                .into(),
            );
        });
    }

    // ==================== STUDIO COUNTDOWN, SCOREBOARD & STAGE ALERT ====================
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_start_studio_countdown(move |mins_str, secs_str, before_str, after_str, mode| {
            let total_secs = compute_countdown_seconds(mins_str.as_str(), secs_str.as_str(), mode);
            let label = build_countdown_label(before_str.as_str(), after_str.as_str());
            let mut c = ctx.borrow_mut();
            c.engine.dispatch(CoreCommand::StartCountdown {
                label,
                seconds: total_secs,
                show_on_main: true,
            });
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
                w.set_status_banner("Contagem regressiva iniciada na saída ao vivo!".into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_add_studio_countdown_to_service(move |mins_str, secs_str, after_str| {
            let total_secs = compute_countdown_seconds(mins_str.as_str(), secs_str.as_str(), 0);
            let label = if after_str.trim().is_empty() {
                "Contagem Regressiva".to_string()
            } else {
                after_str.trim().to_string()
            };
            let mut c = ctx.borrow_mut();
            let item = ServiceItem {
                id: 0,
                service_id: c.active_service().id,
                order: c.active_service().items.len() as i32,
                kind: ServiceItemKind::Countdown,
                ref_id: None,
                title: label,
                subtitle: format!("{:02}:{:02}", total_secs / 60, total_secs % 60),
                params: ServiceItemParams {
                    countdown_seconds: Some(total_secs),
                    ..Default::default()
                },
            };
            append_item_to_active_service(&mut c, item);
            if let Some(w) = ui_weak.upgrade() {
                refresh_playlist_ui(&w, &c);
                w.set_status_banner("Contagem regressiva adicionada ao culto.".into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_stop_countdown_now(move || {
            let mut c = ctx.borrow_mut();
            c.engine.dispatch(CoreCommand::StopCountdown);
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_trigger_stage_alert(move |msg| {
            let mut c = ctx.borrow_mut();
            let opt = if msg.trim().is_empty() {
                None
            } else {
                Some(msg.trim().to_string())
            };
            c.engine.dispatch(CoreCommand::SetStageAlert { message: opt });
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_stage_timer_action(move |act| {
            let mut c = ctx.borrow_mut();
            match act.as_str() {
                "start" => {
                    c.engine.dispatch(CoreCommand::StartStageTimer);
                }
                "pause" => {
                    c.engine.dispatch(CoreCommand::PauseStageTimer);
                }
                _ => {
                    c.engine.dispatch(CoreCommand::ResetStageTimer);
                }
            }
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_adjust_scoreboard(move |da, db, visible, name_a, name_b| {
            let mut c = ctx.borrow_mut();
            let mut sb = c.engine.snapshot().scoreboard;
            sb.visible = visible;
            sb.team_a_name = name_a.to_string();
            sb.team_b_name = name_b.to_string();
            sb.team_a_score = (sb.team_a_score + da).max(0);
            sb.team_b_score = (sb.team_b_score + db).max(0);
            c.engine
                .dispatch(CoreCommand::UpdateScoreboard { scoreboard: sb });
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }

    // ==================== HOLYRICS 66-BOOK BIBLE STUDIO ====================
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_select_version(move |idx| {
            let mut c = ctx.borrow_mut();
            if idx >= 0 && (idx as usize) < c.bible_versions.len() {
                c.bible_version_idx = idx as usize;
                if let Some(w) = ui_weak.upgrade() {
                    refresh_bible_studio_ui(&w, &mut c);
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_select_book(move |book_order| {
            let mut c = ctx.borrow_mut();
            c.bible_book_order = book_order.clamp(1, 66);
            c.bible_chapter = 1;
            c.bible_verse = 1;
            if let Some(w) = ui_weak.upgrade() {
                refresh_bible_studio_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_select_chapter(move |ch| {
            let mut c = ctx.borrow_mut();
            c.bible_chapter = ch.max(1);
            c.bible_verse = 1;
            if let Some(w) = ui_weak.upgrade() {
                refresh_bible_studio_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_step_chapter(move |delta| {
            let mut c = ctx.borrow_mut();
            let ver_id = c.current_bible_version_id();
            let max_ch = c
                .rt
                .block_on(c.storage.count_bible_chapters(ver_id, c.bible_book_order))
                .unwrap_or(1);
            let next_ch = c.bible_chapter + delta;
            if next_ch >= 1 && next_ch <= max_ch {
                c.bible_chapter = next_ch;
            } else if next_ch > max_ch && c.bible_book_order < 66 {
                c.bible_book_order += 1;
                c.bible_chapter = 1;
            } else if next_ch < 1 && c.bible_book_order > 1 {
                c.bible_book_order -= 1;
                c.bible_chapter = c
                    .rt
                    .block_on(c.storage.count_bible_chapters(ver_id, c.bible_book_order))
                    .unwrap_or(1);
            }
            c.bible_verse = 1;
            if let Some(w) = ui_weak.upgrade() {
                refresh_bible_studio_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_select_verse_project(move |verse_num| {
            let mut c = ctx.borrow_mut();
            if c.cached_chapter_verses.is_empty() {
                return;
            }
            let v_clamped = verse_num.clamp(1, c.cached_chapter_verses.len() as i32);
            c.bible_verse = v_clamped;
            project_active_bible_chapter_at_verse(&mut c, v_clamped);
            if let Some(w) = ui_weak.upgrade() {
                w.set_bible_active_verse(v_clamped);
                refresh_bible_favs_and_history_ui(&w, &c);
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_favorite_active(move || {
            let mut c = ctx.borrow_mut();
            if let Some(v) = c
                .cached_chapter_verses
                .iter()
                .find(|v| v.verse == c.bible_verse)
                .cloned()
            {
                let _ = c.rt.block_on(c.storage.add_bible_favorite(&v));
                if let Some(w) = ui_weak.upgrade() {
                    refresh_bible_studio_ui(&w, &mut c);
                    w.set_status_banner(
                        format!("{} {}:{} salvo nos Favoritos.", v.book, v.chapter, v.verse).into(),
                    );
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_add_active_to_service(move || {
            let mut c = ctx.borrow_mut();
            if let Some(v) = c
                .cached_chapter_verses
                .iter()
                .find(|v| v.verse == c.bible_verse)
                .cloned()
            {
                let abbr = c.current_bible_version_abbr();
                let ref_str = format!("{} {}:{}", v.book, v.chapter, v.verse);
                let item = ServiceItem {
                    id: 0,
                    service_id: c.active_service().id,
                    order: c.active_service().items.len() as i32,
                    kind: ServiceItemKind::Bible,
                    ref_id: None,
                    title: ref_str.clone(),
                    subtitle: abbr,
                    params: ServiceItemParams {
                        bible_version_id: Some(v.version_id),
                        custom_slides: vec![v.text],
                        ..Default::default()
                    },
                };
                append_item_to_active_service(&mut c, item);
                if let Some(w) = ui_weak.upgrade() {
                    refresh_playlist_ui(&w, &c);
                    w.set_status_banner(format!("'{ref_str}' adicionado ao culto.").into());
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_search_edited(move |q| {
            let c = ctx.borrow();
            let Some(w) = ui_weak.upgrade() else {
                return;
            };
            let trimmed = q.trim();
            if trimmed.is_empty() {
                w.set_bible_search_hits(ModelRc::new(VecModel::from(
                    Vec::<UiBibleSearchHit>::new(),
                )));
                return;
            }
            let ver_id = c.current_bible_version_id();
            let abbr = c.current_bible_version_abbr();
            if let Ok(hits) = c.rt.block_on(c.storage.search_bible(ver_id, trimmed, 60)) {
                let ui_hits: Vec<UiBibleSearchHit> = hits
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| UiBibleSearchHit {
                        row_idx: i as i32,
                        reference: v.reference(Some(&abbr)).into(),
                        text: v.text.into(),
                        book_order: v.book_order,
                        chapter: v.chapter,
                        verse: v.verse,
                    })
                    .collect();
                w.set_bible_search_hits(ModelRc::new(VecModel::from(ui_hits)));
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_jump_to_hit(move |book_order, chapter, verse| {
            let mut c = ctx.borrow_mut();
            c.bible_book_order = book_order.clamp(1, 66);
            c.bible_chapter = chapter.max(1);
            c.bible_verse = verse.max(1);
            if let Some(w) = ui_weak.upgrade() {
                refresh_bible_studio_ui(&w, &mut c);
                let target_v = c.bible_verse;
                project_active_bible_chapter_at_verse(&mut c, target_v);
                refresh_bible_favs_and_history_ui(&w, &c);
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }

    // ==================== MULTI-MONITOR, SCHEDULE, REPORTS & SETTINGS ====================
    {
        let ui_weak = ui.as_weak();
        ui.on_refresh_monitors(move || {
            if let Some(w) = ui_weak.upgrade() {
                populate_monitor_options(&w);
                w.set_status_banner("Lista de monitores atualizada.".into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_apply_monitor_outputs(move || {
            let c = ctx.borrow();
            let Some(w) = ui_weak.upgrade() else {
                return;
            };
            let ret_themes: Vec<Theme> = c
                .cached_themes
                .iter()
                .filter(|t| t.is_return_theme)
                .cloned()
                .collect();
            if let Some(rt) = ret_themes.get(w.get_return_theme_index().max(0) as usize) {
                c.engine.dispatch(CoreCommand::SetReturnTheme {
                    theme: Box::new(rt.config.clone()),
                });
            }

            if let Some(mgr) = &c.output_mgr {
                apply_single_role_monitor(mgr, OutputRole::Main, w.get_main_monitor_index(), true);
                apply_single_role_monitor(
                    mgr,
                    OutputRole::Stage,
                    w.get_stage_monitor_index(),
                    false,
                );
                apply_single_role_monitor(
                    mgr,
                    OutputRole::Return,
                    w.get_return_monitor_index(),
                    false,
                );
                w.set_status_banner(
                    "Saídas multi-monitor atualizadas (Esc em qualquer tela fecha aquela saída)."
                        .into(),
                );
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_close_all_outputs(move || {
            let c = ctx.borrow();
            if let Some(mgr) = &c.output_mgr {
                let _ = mgr.close_window(OutputRole::Main);
                let _ = mgr.close_window(OutputRole::Stage);
                let _ = mgr.close_window(OutputRole::Return);
            }
            if let Some(w) = ui_weak.upgrade() {
                w.set_main_monitor_index(0);
                w.set_stage_monitor_index(0);
                w.set_return_monitor_index(0);
                w.set_status_banner("Todas as janelas de saída fullscreen foram fechadas.".into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_add_team_schedule(move |member_name, role| {
            let m_trim = member_name.trim();
            let r_trim = role.trim();
            if m_trim.is_empty() || r_trim.is_empty() {
                return;
            }
            let c = ctx.borrow();
            if let Ok(m) = c.rt.block_on(c.storage.upsert_team_member(m_trim)) {
                let _ = c.rt.block_on(c.storage.add_schedule_entry(
                    c.persistent_service.id,
                    m.id,
                    r_trim,
                ));
                if let Some(w) = ui_weak.upgrade() {
                    refresh_schedule_and_reports_ui(&w, &c, w.get_report_weekday_index());
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_remove_team_schedule(move |schedule_id| {
            let c = ctx.borrow();
            let _ = c
                .rt
                .block_on(c.storage.remove_schedule_entry(schedule_id as i64));
            if let Some(w) = ui_weak.upgrade() {
                refresh_schedule_and_reports_ui(&w, &c, w.get_report_weekday_index());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_refresh_reports(move |wk_idx| {
            let c = ctx.borrow();
            if let Some(w) = ui_weak.upgrade() {
                refresh_schedule_and_reports_ui(&w, &c, wk_idx);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_change_language(move |idx| {
            let mut c = ctx.borrow_mut();
            let next_loc = if idx == 1 { Locale::En } else { Locale::PtBr };
            c.i18n.set_locale(next_loc);
            c.settings.locale = next_loc.code().to_string();
            let _ = c.rt.block_on(c.storage.save_settings(&c.settings));
            c.engine.notify_language_changed(next_loc.code());
            if let Some(w) = ui_weak.upgrade() {
                w.set_settings_locale_index(idx);
                w.set_tr(ui_strings_for_locale(next_loc));
                refresh_bible_studio_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_save_app_settings(move || {
            let mut c = ctx.borrow_mut();
            if let Some(w) = ui_weak.upgrade() {
                if let Ok(p) = w.get_settings_api_port().trim().parse::<u16>() {
                    c.settings.api_port = p;
                }
                c.settings.api_token = w.get_settings_api_token().trim().to_string();
                c.settings.shortcuts.prev_slide = w.get_shortcut_prev().trim().to_string();
                c.settings.shortcuts.next_slide = w.get_shortcut_next().trim().to_string();
                c.settings.shortcuts.black_screen = w.get_shortcut_black().trim().to_string();
                c.settings.shortcuts.logo_screen = w.get_shortcut_logo().trim().to_string();
                c.ctrl_state.set_token(c.settings.api_token.clone());
                let _ = c.rt.block_on(c.storage.save_settings(&c.settings));
                w.set_status_banner("Configurações e atalhos salvos.".into());
            }
        });
    }
}

fn apply_single_role_monitor(
    mgr: &OutputManager,
    role: OutputRole,
    dropdown_idx: i32,
    topmost: bool,
) {
    if dropdown_idx > 0 {
        let _ = mgr.open_window(RoleWindowTarget {
            role,
            monitor_index: (dropdown_idx - 1) as usize,
            fullscreen: true,
            topmost,
        });
    } else {
        let _ = mgr.close_window(role);
    }
}

fn stage_and_optionally_project_song(
    ui: &MainWindow,
    c: &mut AppContext,
    song: Song,
    project_immediately: bool,
    start_slide: usize,
) {
    ui.set_selected_song_id(song.id as i32);
    ui.set_active_header_title(song.title.clone().into());
    ui.set_active_header_subtitle(song.artist.clone().into());
    if !song.notes.trim().is_empty() {
        ui.set_operator_notes(song.notes.clone().into());
    }

    if let Some(&bound_tid) = c.song_theme_map.get(&song.id) {
        ui.set_save_theme_to_song(true);
        if project_immediately {
            if let Some(t) = c.cached_themes.iter().find(|t| t.id == bound_tid).cloned() {
                c.active_theme_id = t.id;
                c.sync_background_video_for_theme(&t.config);
                c.engine.dispatch(CoreCommand::SetMainTheme {
                    theme: Box::new(t.config),
                });
                update_active_theme_indicator(ui, c);
            }
        }
    } else {
        ui.set_save_theme_to_song(false);
    }

    c.record_song_in_session_history(&song);
    refresh_session_history_ui(ui, c);

    ui.set_active_slides(ModelRc::new(VecModel::from(slides_to_cards(&song.slides))));

    if project_immediately {
        let theme = c.theme_for_song(song.id);
        c.sync_background_video_for_theme(&theme);
        let live = LivePresentationItem::from_song(&song, None, theme);
        c.staged_song = Some(song);
        c.engine.dispatch(CoreCommand::PresentItem {
            item: Box::new(live),
            start_slide,
        });
        sync_output_state_to_ui(ui, c);
    } else {
        let live_same = c
            .engine
            .snapshot()
            .current_item
            .and_then(|i| i.song_id)
            == Some(song.id);
        if live_same {
            ui.set_current_slide_index(c.engine.snapshot().current_slide_index as i32);
        } else {
            ui.set_current_slide_index(-1);
        }
        c.staged_song = Some(song);
    }
}

fn project_active_bible_chapter_at_verse(c: &mut AppContext, verse_num: i32) {
    if c.cached_chapter_verses.is_empty() {
        return;
    }
    let abbr = c.current_bible_version_abbr();
    let start_idx = c
        .cached_chapter_verses
        .iter()
        .position(|v| v.verse == verse_num)
        .unwrap_or(0);

    if let Some(v) = c.cached_chapter_verses.get(start_idx) {
        let _ = c.rt.block_on(c.storage.record_bible_history(v));
    }

    let book_name = c
        .cached_chapter_verses
        .first()
        .map(|v| v.book.clone())
        .unwrap_or_else(|| "Bíblia".to_string());
    let ch_num = c.bible_chapter;

    let slides: Vec<PresentationSlide> = c
        .cached_chapter_verses
        .iter()
        .enumerate()
        .map(|(idx, v)| PresentationSlide {
            index: idx,
            text: format!("{} {}", v.verse, v.text),
            footer: Some(format!("{} {}:{} ({})", v.book, v.chapter, v.verse, abbr)),
            forced_break: false,
            duration_secs: None,
        })
        .collect();

    c.staged_song = None;
    let theme = c.engine.snapshot().main_theme;
    c.sync_background_video_for_theme(&theme);
    c.engine.dispatch(CoreCommand::PresentItem {
        item: Box::new(LivePresentationItem {
            service_item_id: None,
            song_id: None,
            title: format!("{book_name} {ch_num}"),
            subtitle: abbr,
            notes: String::new(),
            kind: ServiceItemKind::Bible,
            slides,
            theme,
            media_path: None,
            media_kind: None,
        }),
        start_slide: start_idx,
    });
}

fn import_single_file(c: &mut AppContext, path: &Path) -> String {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if ext == "pptx" {
        match parse_pptx_file(path) {
            Ok(pres) => {
                let count = pres.slide_texts.len();
                let title = pres.title.clone();
                let item = pres.into_service_item(
                    c.active_service().id,
                    c.active_service().items.len() as i32,
                );
                append_item_to_active_service(c, item);
                format!("PowerPoint '{title}' ({count} slides) adicionado ao culto!")
            }
            Err(e) => format!("Erro ao importar PPTX: {e}"),
        }
    } else if ext == "json" {
        match import_json_bundle_from_file(path) {
            Ok(bundle) => {
                let mut count = 0usize;
                for s in &bundle.songs {
                    if c.rt.block_on(c.storage.insert_song(s, true)).is_ok() {
                        count += 1;
                    }
                }
                format!("JSON Overture importado: {count} músicas processadas.")
            }
            Err(e) => format!("Erro JSON: {e}"),
        }
    } else {
        let xml_raw = std::fs::read_to_string(path).unwrap_or_default();
        if xml_raw.contains("openlyrics")
            && let Ok(song) = parse_openlp_file(path, DEFAULT_MAX_LINE_CHARS)
        {
            let _ = c.rt.block_on(c.storage.insert_song(&song, true));
            format!("Música OpenLP XML '{}' importada!", song.title)
        } else if xml_raw.contains("XMLBIBLE")
            && let Ok((ver, verses)) =
                parse_zefania_bible_xml(&xml_raw, "IMPORT", "pt-BR", "Importada")
        {
            let _ = c.rt.block_on(c.storage.import_bible_version(&ver, &verses));
            c.bible_versions = c
                .rt
                .block_on(c.storage.list_bible_versions())
                .unwrap_or_default();
            format!(
                "Bíblia Zefania '{}' ({} versículos) importada!",
                ver.name,
                verses.len()
            )
        } else if let Ok(song) = parse_opensong_file(path, DEFAULT_MAX_LINE_CHARS) {
            let _ = c.rt.block_on(c.storage.insert_song(&song, true));
            format!("Música OpenSong '{}' importada!", song.title)
        } else {
            "Formato de arquivo não reconhecido.".to_string()
        }
    }
}

fn append_item_to_active_service(c: &mut AppContext, item: ServiceItem) {
    if c.using_temp_service {
        c.temp_service.items.push(item);
    } else {
        c.persistent_service.items.push(item);
        let svc_id = c.persistent_service.id;
        let items = c.persistent_service.items.clone();
        if let Ok(saved) = c.rt.block_on(c.storage.replace_service_items(svc_id, &items)) {
            c.persistent_service.items = saved;
        }
    }
}

fn slides_to_cards(slides: &[SongSlide]) -> Vec<UiSlideCard> {
    slides
        .iter()
        .enumerate()
        .map(|(i, s)| UiSlideCard {
            slide_idx: i as i32,
            number_label: format!("{}", i + 1).into(),
            body: s.text.clone().into(),
            forced_break: s.forced_break,
        })
        .collect()
}

fn song_to_ui_row(s: &Song) -> UiSongRow {
    let fb = s.slides.iter().any(|sl| sl.forced_break);
    UiSongRow {
        song_id: s.id as i32,
        title: s.title.clone().into(),
        artist: s.artist.clone().into(),
        meta: format!("{} slides", s.slides.len()).into(),
        has_forced_break: fb,
        duplicate_marked: s.duplicate_marked,
        archived: s.archived,
    }
}

// ==================== UI MODEL REFRESHERS ====================

fn refresh_all_ui_models(ui: &MainWindow, c: &mut AppContext) {
    refresh_playlist_ui(ui, c);
    refresh_songs_ui(ui, c, "", ui.get_show_archived());
    refresh_session_history_ui(ui, c);
    refresh_media_ui(ui, c);
    refresh_themes_ui(ui, c);
    refresh_bible_studio_ui(ui, c);
    refresh_schedule_and_reports_ui(ui, c, ui.get_report_weekday_index());
    sync_output_state_to_ui(ui, c);
}

fn refresh_playlist_ui(ui: &MainWindow, c: &AppContext) {
    let svc = c.active_service();
    ui.set_current_service_name(svc.name.clone().into());

    let mut song_rows = Vec::new();
    let mut media_rows = Vec::new();

    for (idx, it) in svc.items.iter().enumerate() {
        let is_media = matches!(
            it.kind,
            ServiceItemKind::Media | ServiceItemKind::Countdown
        );
        let row = UiPlaylistRow {
            item_id: it.id as i32,
            row_idx: idx as i32,
            kind_badge: match it.kind {
                ServiceItemKind::Song => "MÚS".into(),
                ServiceItemKind::Bible => "BÍB".into(),
                ServiceItemKind::Media => "MÍD".into(),
                ServiceItemKind::Announcement => "TXT".into(),
                ServiceItemKind::Countdown => "REG".into(),
                ServiceItemKind::Presentation => "PPT".into(),
            },
            title: it.title.clone().into(),
            subtitle: it.subtitle.clone().into(),
            is_media,
        };
        if is_media {
            media_rows.push(row);
        } else {
            song_rows.push(row);
        }
    }

    ui.set_playlist_song_rows(ModelRc::new(VecModel::from(song_rows)));
    ui.set_playlist_media_rows(ModelRc::new(VecModel::from(media_rows)));
}

fn refresh_songs_ui(ui: &MainWindow, c: &AppContext, query: &str, include_archived: bool) {
    if let Ok(songs) = c
        .rt
        .block_on(c.storage.search_songs(query, include_archived, None, 500))
    {
        let rows: Vec<UiSongRow> = songs.iter().map(song_to_ui_row).collect();
        ui.set_songs_list(ModelRc::new(VecModel::from(rows)));
    }
}

fn refresh_session_history_ui(ui: &MainWindow, c: &AppContext) {
    let rows: Vec<UiSongRow> = c.session_song_history.iter().map(song_to_ui_row).collect();
    ui.set_song_history_rows(ModelRc::new(VecModel::from(rows)));
}

fn refresh_media_ui(ui: &MainWindow, c: &AppContext) {
    if let Ok(items) = c.rt.block_on(c.storage.list_media(false)) {
        let rows: Vec<UiMediaRow> = items
            .into_iter()
            .map(|m| UiMediaRow {
                media_id: m.id as i32,
                name: m.name.into(),
                kind: m.kind.as_str().to_uppercase().into(),
                path: m.path.into(),
            })
            .collect();
        ui.set_media_list(ModelRc::new(VecModel::from(rows)));
    }
}

fn refresh_themes_ui(ui: &MainWindow, c: &mut AppContext) {
    let Ok(themes) = c.rt.block_on(c.storage.list_themes()) else {
        return;
    };

    let mut rows = Vec::with_capacity(themes.len());
    for t in &themes {
        let sample = if t.is_return_theme {
            "Retorno\nPalco"
        } else {
            "Santo! Santo!\nSenhor Deus"
        };
        let thumb_frame =
            c.preview_compositor
                .compose_theme_preview(&t.config, sample, 160, 90, None);
        let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            &thumb_frame.pixels,
            thumb_frame.width,
            thumb_frame.height,
        );
        rows.push(UiThemeRow {
            theme_id: t.id as i32,
            name: t.name.clone().into(),
            is_return: t.is_return_theme,
            summary: format!("{} • {}pt", t.config.font_family, t.config.font_size as u32).into(),
            thumbnail: Image::from_rgba8(buf),
        });
    }

    let ret_choices: Vec<SharedString> = themes
        .iter()
        .filter(|t| t.is_return_theme)
        .map(|t| t.name.clone().into())
        .collect();
    if !ret_choices.is_empty() {
        ui.set_return_theme_options(ModelRc::new(VecModel::from(ret_choices)));
    }

    c.cached_themes = themes;
    ui.set_themes_list(ModelRc::new(VecModel::from(rows)));

    if let Some(first) = c.cached_themes.first().cloned()
        && ui.get_theme_edit_id() == 0
    {
        populate_theme_editor_fields(ui, &first);
        render_theme_preview_into_ui(ui, c);
    }

    update_active_theme_indicator(ui, c);
}

fn update_active_theme_indicator(ui: &MainWindow, c: &mut AppContext) {
    ui.set_active_theme_id(c.active_theme_id as i32);
    if let Some(active_t) = c
        .cached_themes
        .iter()
        .find(|t| t.id == c.active_theme_id)
        .cloned()
    {
        ui.set_active_theme_name(active_t.name.into());
        let frame = c.preview_compositor.compose_theme_preview(
            &active_t.config,
            "Santo! Santo!",
            240,
            135,
            None,
        );
        let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
            &frame.pixels,
            frame.width,
            frame.height,
        );
        ui.set_active_theme_thumb(Image::from_rgba8(buf));
    }
}

fn refresh_bible_studio_ui(ui: &MainWindow, c: &mut AppContext) {
    if c.bible_versions.is_empty() {
        c.bible_versions = c
            .rt
            .block_on(c.storage.list_bible_versions())
            .unwrap_or_default();
    }
    let abbrs: Vec<SharedString> = c
        .bible_versions
        .iter()
        .map(|v| v.abbr.clone().into())
        .collect();
    if !abbrs.is_empty() {
        ui.set_bible_version_abbrs(ModelRc::new(VecModel::from(abbrs)));
    }
    c.bible_version_idx = c
        .bible_version_idx
        .min(c.bible_versions.len().saturating_sub(1));
    ui.set_bible_version_idx(c.bible_version_idx as i32);

    if let Some(ver) = c.current_bible_version() {
        ui.set_bible_license_notice(format!("{} — {}", ver.name, ver.license).into());
    }

    let is_en = c.i18n.locale() == Locale::En;
    let ver_id = c.current_bible_version_id();

    // 1. 6×11 Color-Coded Canonical 66-Book Grid
    let mut book_rows: Vec<UiBibleBookGridRow> = Vec::with_capacity(6);
    for row_books in CANONICAL_BIBLE_BOOKS.chunks(11) {
        let tiles: Vec<UiBibleBookTile> = row_books
            .iter()
            .map(|b| UiBibleBookTile {
                book_order: b.order,
                abbr: (if is_en { b.abbr_en } else { b.abbr_pt }).into(),
                name: (if is_en { b.name_en } else { b.name_pt }).into(),
                category: b.category as i32,
                active: b.order == c.bible_book_order,
            })
            .collect();
        book_rows.push(UiBibleBookGridRow {
            tiles: ModelRc::new(VecModel::from(tiles)),
        });
    }
    ui.set_bible_book_grid_rows(ModelRc::new(VecModel::from(book_rows)));
    ui.set_bible_active_book(c.bible_book_order);

    // 2. Chapters Grid (9 columns, padded with 0s for uniform tile widths)
    let ch_count = c
        .rt
        .block_on(c.storage.count_bible_chapters(ver_id, c.bible_book_order))
        .unwrap_or(1)
        .max(1);
    c.bible_chapter = c.bible_chapter.clamp(1, ch_count);
    ui.set_bible_active_chapter(c.bible_chapter);

    ui.set_bible_chapter_grid_rows(build_padded_number_grid(ch_count as usize, 9));

    let book_title = canonical_book_by_order(c.bible_book_order)
        .map(|b| if is_en { b.name_en } else { b.name_pt })
        .unwrap_or("Gênesis");
    ui.set_bible_book_chapter_label(format!("{book_title}: {}", c.bible_chapter).into());

    // 3. Chapter Verses List (Left Pane) + Verses Grid (Right Bottom-Right, 7 columns)
    let verses = c
        .rt
        .block_on(
            c.storage
                .list_bible_chapter_verses(ver_id, c.bible_book_order, c.bible_chapter),
        )
        .unwrap_or_default();

    let fav_set: HashSet<(String, i32, i32)> = c
        .rt
        .block_on(c.storage.list_bible_favorites())
        .unwrap_or_default()
        .into_iter()
        .map(|f| (f.book, f.chapter, f.verse))
        .collect();

    let verse_count = verses.len().max(1);
    c.bible_verse = c.bible_verse.clamp(1, verse_count as i32);
    ui.set_bible_active_verse(c.bible_verse);
    ui.set_bible_verse_grid_rows(build_padded_number_grid(verse_count, 7));

    let ui_verses: Vec<UiBibleChapterVerse> = verses
        .iter()
        .map(|v| UiBibleChapterVerse {
            verse: v.verse,
            text: v.text.clone().into(),
            book: v.book.clone().into(),
            chapter: v.chapter,
            is_favorite: fav_set.contains(&(v.book.clone(), v.chapter, v.verse)),
        })
        .collect();
    ui.set_bible_chapter_verses(ModelRc::new(VecModel::from(ui_verses)));
    c.cached_chapter_verses = verses;

    refresh_bible_favs_and_history_ui(ui, c);
}

fn refresh_bible_favs_and_history_ui(ui: &MainWindow, c: &AppContext) {
    let favs = c
        .rt
        .block_on(c.storage.list_bible_favorites())
        .unwrap_or_default();
    let fav_hits: Vec<UiBibleSearchHit> = favs
        .into_iter()
        .enumerate()
        .map(|(i, f)| {
            let b_ord = find_book_order_by_name(&f.book);
            UiBibleSearchHit {
                row_idx: i as i32,
                reference: format!("{} {}:{}", f.book, f.chapter, f.verse).into(),
                text: f.text.into(),
                book_order: b_ord,
                chapter: f.chapter,
                verse: f.verse,
            }
        })
        .collect();
    ui.set_bible_favorite_hits(ModelRc::new(VecModel::from(fav_hits)));

    let hist = c
        .rt
        .block_on(c.storage.list_bible_history(30))
        .unwrap_or_default();
    let hist_hits: Vec<UiBibleSearchHit> = hist
        .into_iter()
        .enumerate()
        .map(|(i, h)| {
            let b_ord = find_book_order_by_name(&h.book);
            UiBibleSearchHit {
                row_idx: i as i32,
                reference: format!("{} {}:{}", h.book, h.chapter, h.verse).into(),
                text: h.text.into(),
                book_order: b_ord,
                chapter: h.chapter,
                verse: h.verse,
            }
        })
        .collect();
    ui.set_bible_history_hits(ModelRc::new(VecModel::from(hist_hits)));
}

fn find_book_order_by_name(name: &str) -> i32 {
    CANONICAL_BIBLE_BOOKS
        .iter()
        .find(|b| {
            b.name_pt.eq_ignore_ascii_case(name)
                || b.name_en.eq_ignore_ascii_case(name)
                || name.starts_with(b.name_pt)
        })
        .map(|b| b.order)
        .unwrap_or(1)
}

fn build_padded_number_grid(count: usize, cols: usize) -> ModelRc<UiNumberGridRow> {
    let nums: Vec<i32> = (1..=count as i32).collect();
    let mut rows = Vec::new();
    for chunk in nums.chunks(cols) {
        let mut cells = chunk.to_vec();
        while cells.len() < cols {
            cells.push(0);
        }
        rows.push(UiNumberGridRow {
            cells: ModelRc::new(VecModel::from(cells)),
        });
    }
    ModelRc::new(VecModel::from(rows))
}

fn refresh_schedule_and_reports_ui(ui: &MainWindow, c: &AppContext, wk_idx: i32) {
    let counts_map: HashMap<i64, i64> = c
        .rt
        .block_on(c.storage.report_member_schedule_counts())
        .unwrap_or_default()
        .into_iter()
        .map(|m| (m.member_id, m.times_scheduled))
        .collect();

    if let Ok(sched) = c
        .rt
        .block_on(c.storage.list_service_schedule(c.persistent_service.id))
    {
        let rows: Vec<UiScheduleRow> = sched
            .into_iter()
            .map(|s| UiScheduleRow {
                schedule_id: s.id as i32,
                member_name: s.member_name.into(),
                role: s.role.into(),
                total_times: *counts_map.get(&s.member_id).unwrap_or(&1) as i32,
            })
            .collect();
        ui.set_schedule_rows(ModelRc::new(VecModel::from(rows)));
    }

    let weekday = if wk_idx > 0 {
        Some((wk_idx - 1) as u32)
    } else {
        None
    };

    if let Ok(stats) = c
        .rt
        .block_on(c.storage.report_most_played_songs(None, None, weekday, 60))
    {
        let rows: Vec<UiReportRow> = stats
            .into_iter()
            .map(|st| UiReportRow {
                title: st.title.into(),
                artist: st.artist.into(),
                executions: st.play_count as i32,
            })
            .collect();
        ui.set_report_rows(ModelRc::new(VecModel::from(rows)));
    }
}

fn sync_output_state_to_ui(ui: &MainWindow, c: &mut AppContext) {
    let st = c.engine.snapshot();
    ui.set_is_black_screen(st.display_mode == DisplayMode::BlackScreen);
    ui.set_is_logo_screen(st.display_mode == DisplayMode::Logo);
    ui.set_is_clear_text(st.display_mode == DisplayMode::ClearText);
    ui.set_auto_play_enabled(st.auto_play_enabled);
    ui.set_stage_timer_display(st.formatted_stage_timer().into());
    ui.set_scoreboard_visible(st.scoreboard.visible);
    ui.set_team_a_score(st.scoreboard.team_a_score);
    ui.set_team_b_score(st.scoreboard.team_b_score);

    let badge = if st.display_mode == DisplayMode::BlackScreen {
        "TELA PRETA"
    } else if st.display_mode == DisplayMode::Logo {
        "LOGO"
    } else if st.display_mode == DisplayMode::ClearText {
        "SEM TEXTO"
    } else if st.countdown.active {
        "CONTAGEM"
    } else if st.current_item.is_some() {
        "AO VIVO"
    } else {
        "NORMAL"
    };
    ui.set_live_mode_badge(badge.into());

    if let Some(item) = &st.current_item {
        // Keep Bible active verse synced when stepping slides in Bible mode!
        if item.kind == ServiceItemKind::Bible
            && let Some(v) = c.cached_chapter_verses.get(st.current_slide_index)
        {
            c.bible_verse = v.verse;
            ui.set_bible_active_verse(v.verse);
        }

        // If no separate song is staged in Column 2, or the staged song IS the live item, reflect live slides
        let show_live_in_col2 = match &c.staged_song {
            Some(staged) => item.song_id == Some(staged.id),
            None => true,
        };

        if show_live_in_col2 {
            ui.set_active_header_title(item.title.clone().into());
            ui.set_active_header_subtitle(item.subtitle.clone().into());
            let cards: Vec<UiSlideCard> = item
                .slides
                .iter()
                .map(|s| UiSlideCard {
                    slide_idx: s.index as i32,
                    number_label: format!("{}", s.index + 1).into(),
                    body: s.text.clone().into(),
                    forced_break: s.forced_break,
                })
                .collect();
            ui.set_active_slides(ModelRc::new(VecModel::from(cards)));
            ui.set_current_slide_index(st.current_slide_index as i32);
        }
    } else if c.staged_song.is_none() {
        ui.set_current_slide_index(-1);
    }

    update_live_preview_and_telemetry(ui, c);
}

fn update_live_preview_and_telemetry(ui: &MainWindow, c: &mut AppContext) {
    let st = c.engine.snapshot();
    let vf = c.media_engine.subscribe_frames().borrow().clone();
    let vf_view = vf.as_ref().map(|v| v.as_view());

    // Always render Main 16:9 preview (used in Col 3 & Bible mini-monitor)
    let main_frame = c
        .preview_compositor
        .compose(&st, OutputRole::Main, 480, 270, vf_view);
    let main_buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        &main_frame.pixels,
        main_frame.width,
        main_frame.height,
    );
    ui.set_live_main_image(Image::from_rgba8(main_buf));

    // Render Stage or Return preview on demand when selected in Col 3's preview role tabs
    match ui.get_preview_role_tab() {
        1 => {
            let stage_frame =
                c.preview_compositor
                    .compose(&st, OutputRole::Stage, 480, 270, None);
            let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                &stage_frame.pixels,
                stage_frame.width,
                stage_frame.height,
            );
            ui.set_live_stage_image(Image::from_rgba8(buf));
        }
        2 => {
            let ret_frame =
                c.preview_compositor
                    .compose(&st, OutputRole::Return, 480, 270, None);
            let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                &ret_frame.pixels,
                ret_frame.width,
                ret_frame.height,
            );
            ui.set_live_return_image(Image::from_rgba8(buf));
        }
        _ => {}
    }

    if ui.get_active_modal() == 3 && ui.get_theme_bg_kind() == 3 {
        render_theme_preview_into_ui(ui, c);
    }

    if let Some(mgr) = &c.output_mgr {
        let frames = mgr.telemetry().frames_presented();
        if frames > 0 {
            ui.set_output_latency_text(
                format!("{:.2} ms ({} q)", mgr.telemetry().last_ms(), frames).into(),
            );
        } else {
            ui.set_output_latency_text("Pronto (60 FPS)".into());
        }
    }
}

fn populate_theme_editor_fields(ui: &MainWindow, t: &Theme) {
    ui.set_theme_edit_id(t.id as i32);
    ui.set_theme_edit_name(t.name.clone().into());
    ui.set_theme_font_family(t.config.font_family.clone().into());
    ui.set_theme_font_size(format!("{}", t.config.font_size as u32).into());
    ui.set_theme_text_color(t.config.text_color.to_hex().into());
    ui.set_theme_shadow_enabled(t.config.shadow.enabled);
    ui.set_theme_is_return(t.is_return_theme);
    ui.set_theme_align_index(match t.config.h_align {
        HorizontalAlign::Left => 0,
        HorizontalAlign::Center => 1,
        HorizontalAlign::Right => 2,
    });
    match &t.config.background {
        BackgroundSpec::SolidColor { color } => {
            ui.set_theme_bg_kind(0);
            ui.set_theme_bg_primary(color.to_hex().into());
        }
        BackgroundSpec::Gradient { top, bottom } => {
            ui.set_theme_bg_kind(1);
            ui.set_theme_bg_primary(top.to_hex().into());
            ui.set_theme_bg_secondary(bottom.to_hex().into());
        }
        BackgroundSpec::Image { path, .. } => {
            ui.set_theme_bg_kind(2);
            ui.set_theme_bg_media(path.clone().into());
        }
        BackgroundSpec::Video { path, .. } => {
            ui.set_theme_bg_kind(3);
            ui.set_theme_bg_media(path.clone().into());
        }
    }
}

fn read_theme_config_from_ui(ui: &MainWindow) -> ThemeConfig {
    let mut cfg = ThemeConfig::default();
    let ff = ui.get_theme_font_family().trim().to_string();
    if !ff.is_empty() {
        cfg.font_family = ff;
    }
    if let Ok(sz) = ui.get_theme_font_size().trim().parse::<f32>() {
        cfg.font_size = sz.clamp(18.0, 160.0);
    }
    if let Some(col) = RgbaColor::from_hex(ui.get_theme_text_color().as_str()) {
        cfg.text_color = col;
    }
    cfg.shadow.enabled = ui.get_theme_shadow_enabled();
    cfg.h_align = match ui.get_theme_align_index() {
        0 => HorizontalAlign::Left,
        2 => HorizontalAlign::Right,
        _ => HorizontalAlign::Center,
    };
    let prim = ui.get_theme_bg_primary().trim().to_string();
    let sec = ui.get_theme_bg_secondary().trim().to_string();
    let med = ui.get_theme_bg_media().trim().to_string();
    cfg.background = match ui.get_theme_bg_kind() {
        0 => BackgroundSpec::SolidColor {
            color: RgbaColor::from_hex(&prim).unwrap_or(RgbaColor::new(10, 15, 30, 255)),
        },
        1 => BackgroundSpec::Gradient {
            top: RgbaColor::from_hex(&prim).unwrap_or(RgbaColor::new(15, 28, 62, 255)),
            bottom: RgbaColor::from_hex(&sec).unwrap_or(RgbaColor::new(3, 6, 16, 255)),
        },
        2 => BackgroundSpec::Image {
            path: med,
            dim_opacity: 0.35,
        },
        _ => BackgroundSpec::Video {
            path: if med.is_empty() {
                "motion://aurora".to_string()
            } else {
                med
            },
            dim_opacity: 0.25,
            loop_playback: true,
        },
    };
    cfg
}

fn render_theme_preview_into_ui(ui: &MainWindow, c: &mut AppContext) {
    let cfg = read_theme_config_from_ui(ui);
    if let BackgroundSpec::Video {
        path,
        loop_playback,
        ..
    } = &cfg.background
    {
        c.media_engine.send(MediaCommand::PlayBackgroundVideo {
            path: path.clone(),
            loop_playback: *loop_playback,
        });
    }
    let vf = c.media_engine.subscribe_frames().borrow().clone();
    let vf_view = vf.as_ref().map(|v| v.as_view());
    let sample = "Santo! Santo! Santo!\nDeus Onipotente!";
    let frame = c
        .preview_compositor
        .compose_theme_preview(&cfg, sample, 480, 270, vf_view);
    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        &frame.pixels,
        frame.width,
        frame.height,
    );
    ui.set_theme_live_preview(Image::from_rgba8(buf));
}

fn compute_countdown_seconds(mins_str: &str, secs_str: &str, mode: i32) -> u64 {
    if mode == 1 {
        // Exact target wall-clock time HH:MM
        let target_h = mins_str.trim().parse::<u32>().unwrap_or(19).min(23);
        let target_m = secs_str.trim().parse::<u32>().unwrap_or(0).min(59);
        let now = chrono::Local::now();
        let now_secs = now.hour() * 3600 + now.minute() * 60 + now.second();
        let target_secs = target_h * 3600 + target_m * 60;
        let diff = if target_secs > now_secs {
            target_secs - now_secs
        } else {
            86_400 - (now_secs - target_secs)
        };
        (diff as u64).max(1)
    } else {
        let mins = mins_str.trim().parse::<u64>().unwrap_or(10);
        let secs = secs_str.trim().parse::<u64>().unwrap_or(0);
        (mins * 60 + secs).max(1)
    }
}

fn build_countdown_label(before: &str, after: &str) -> String {
    let b = before.trim();
    let a = after.trim();
    match (b.is_empty(), a.is_empty()) {
        (true, true) => "Para iniciar o culto".to_string(),
        (false, true) => b.to_string(),
        (true, false) => a.to_string(),
        (false, false) => format!("{b} — {a}"),
    }
}

fn normalize_key_name(raw: &str) -> String {
    match raw {
        "\u{f703}" | "\u{001d}" => "Right".to_string(),
        "\u{f702}" | "\u{001c}" => "Left".to_string(),
        "\u{001b}" => "Escape".to_string(),
        "\u{f729}" => "Home".to_string(),
        "\u{f72b}" => "End".to_string(),
        other => other.trim().to_string(),
    }
}

fn eq_key(actual: &str, configured: &str) -> bool {
    !configured.is_empty() && actual.eq_ignore_ascii_case(configured.trim())
}

fn resolve_app_paths(is_smoke_test: bool) -> Result<(PathBuf, PathBuf)> {
    if is_smoke_test {
        let dir = std::env::temp_dir().join("overture_smoke_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        return Ok((dir.join("overture.db"), dir.join("media")));
    }
    if let Some(proj) = ProjectDirs::from("org", "Overture", "Overture") {
        let data = proj.data_local_dir().to_path_buf();
        Ok((data.join("overture.db"), data.join("media")))
    } else {
        let base = PathBuf::from("./overture_data");
        Ok((base.join("overture.db"), base.join("media")))
    }
}

fn find_bundled_logo() -> Option<String> {
    let mut candidates = vec![PathBuf::from("assets/overture-512.png")];
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("assets").join("overture-512.png"));
        candidates.push(dir.join("overture-512.png"));
    }
    candidates
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().to_string())
}

async fn seed_initial_demo_data_if_empty(storage: &Storage) -> Result<()> {
    let existing = storage.search_songs("", true, None, 1).await?;
    if !existing.is_empty() {
        return Ok(());
    }

    // Strictly Public-Domain classic hymns (pre-1929)
    let hymns = [
        (
            "Alvo Mais Que a Neve",
            "Hinário Clássico (1881)",
            "Bendito seja o Cordeiro\nQue na cruz por nós padeceu!\n\nBendito seja o Seu sangue\nQue por nós ali Ele verteu!\n\nEis nesse sangue lavados,\nCom roupas que tão alvas são,\n\nOs pecadores remidos\nQue perante seu Deus já estão!\n\nAlvo mais que a neve!\nAlvo mais que a neve!\n\nSim, nesse sangue lavado,\nMais alvo que a neve serei!",
        ),
        (
            "Castelo Forte",
            "Martinho Lutero (1529)",
            "Castelo forte é nosso Deus,\nEspada e bom escudo;\n\nCom Seu poder defende os Seus\nEm todo transe agudo.\n\nA força do homem nada faz,\nSozinho está perdido;\n\nMas nosso Deus socorro traz\nEm Seu Filho escolhido.",
        ),
        (
            "Firme nas Promessas",
            "Russell Kelso Carter (1886)",
            "Firme nas promessas do meu Salvador,\nCantarei louvores ao meu Redentor!\n\nFico, pelos séculos do Seu amor,\nFirme nas promessas de Jesus!\n\nFirme, firme!\nFirme nas promessas de Jesus, meu Mestre!\n\nFirme, firme,\nSim, firme nas promessas de Jesus!",
        ),
        (
            "Graça Excelsa (Amazing Grace)",
            "John Newton (1779)",
            "Sublime graça do Senhor\nQue a um infeliz salvou!\n\nEu cego estava, deu-me luz,\nPerdido e me encontrou!\n\nA graça me ensinou a temer\nE o meu temor levou;\n\nQuão preciosa apareceu\nA graça em mim, Senhor!",
        ),
        (
            "Grandioso És Tu",
            "Carl Boberg (1885)",
            "Senhor meu Deus, quando eu maravilhado,\nContemplo a Tua imensa criação,\n\nA terra e o mar e o céu todo estrelado\nMe vêm falar da Tua perfeição.\n\nEntão minh'alma canta a Ti, Senhor:\nGrandioso és Tu! Grandioso és Tu!\n\nEntão minh'alma canta a Ti, Senhor:\nGrandioso és Tu! Grandioso és Tu!",
        ),
        (
            "Mais Perto Quero Estar",
            "Sarah Flower Adams (1841)",
            "Mais perto quero estar,\nMeu Deus, de Ti,\n\nInda que seja a dor\nQue me una a Ti!\n\nSempre hei de suplicar:\nMais perto quero estar,\n\nMais perto quero estar,\nMeu Deus, de Ti!",
        ),
        (
            "Rocha Eterna",
            "Augustus Toplady (1776)",
            "Rocha eterna, foi na cruz\nQue morreste Tu, Jesus!\n\nVem de Ti um sangue tal\nQue me limpa todo o mal;\n\nTraz as bênçãos do perdão:\nGozo, paz e salvação!",
        ),
        (
            "Santo, Santo, Santo",
            "Reginald Heber (1826)",
            "Santo! Santo! Santo!\nDeus Onipotente!\n\nCedo de manhã\nCantaremos Teu louvor!\n\nSanto! Santo! Santo!\nDeus Jeová Triúno!\n\nÉs um só Deus,\nExcelso Criador!",
        ),
        (
            "Tudo Entregarei",
            "Judson Van DeVenter (1896)",
            "Tudo a Cristo eu entrego,\nTudo a Ele darei!\n\nResoluto, mas submisso,\nSempre a Ele seguirei!\n\nTudo entregarei!\nTudo entregarei!\n\nSim, por Ti, Jesus bendito,\nTudo deixarei!",
        ),
        (
            "Vencendo Vem Jesus",
            "Julia Ward Howe (1861)",
            "Já refulge a glória eterna\nDe Jesus, o Rei dos reis;\n\nBreve os reinos deste mundo\nSeguirão as Suas leis!\n\nGlória, glória! Aleluia!\nGlória, glória! Aleluia!\n\nGlória, glória! Aleluia!\nVencendo vem Jesus!",
        ),
    ];

    let mut seeded_ids = Vec::new();
    for (title, artist, raw) in hymns {
        let mut s = Song::new_draft(title, artist, "public_domain_seed");
        s.slides = raw
            .split("\n\n")
            .enumerate()
            .map(|(i, txt)| SongSlide {
                id: 0,
                song_id: 0,
                order: i as i32,
                text: txt.to_string(),
                forced_break: false,
                duration_secs: None,
            })
            .collect();
        if let overture_storage::SongInsertOutcome::Inserted { id, .. } =
            storage.insert_song(&s, true).await?
        {
            seeded_ids.push((id, title, artist));
        }
    }

    for (name, uri) in [
        ("Motion — Aurora Boreal", "motion://aurora"),
        ("Motion — Oceano Profundo", "motion://ocean"),
        ("Motion — Âmbar Dourado", "motion://warm"),
    ] {
        storage
            .upsert_media(&MediaItem {
                id: 0,
                name: name.to_string(),
                path: uri.to_string(),
                kind: MediaKind::Video,
                duration_secs: None,
                archived: false,
            })
            .await?;
    }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let svc = storage.create_service(&today, "Culto Dominical").await?;
    let mut initial_items = Vec::new();
    initial_items.push(ServiceItem {
        id: 0,
        service_id: svc.id,
        order: 0,
        kind: ServiceItemKind::Countdown,
        ref_id: None,
        title: "Para iniciar o culto".to_string(),
        subtitle: "10:00".to_string(),
        params: ServiceItemParams {
            countdown_seconds: Some(600),
            ..Default::default()
        },
    });
    for (idx, (sid, title, artist)) in seeded_ids.iter().take(4).enumerate() {
        initial_items.push(ServiceItem {
            id: 0,
            service_id: svc.id,
            order: (idx + 1) as i32,
            kind: ServiceItemKind::Song,
            ref_id: Some(*sid),
            title: (*title).to_string(),
            subtitle: (*artist).to_string(),
            params: ServiceItemParams::default(),
        });
    }
    initial_items.push(ServiceItem {
        id: 0,
        service_id: svc.id,
        order: (initial_items.len()) as i32,
        kind: ServiceItemKind::Bible,
        ref_id: None,
        title: "Salmos 23:1-6".to_string(),
        subtitle: "TB1917".to_string(),
        params: ServiceItemParams {
            bible_version_id: Some(1),
            ..Default::default()
        },
    });
    storage
        .replace_service_items(svc.id, &initial_items)
        .await?;

    Ok(())
}

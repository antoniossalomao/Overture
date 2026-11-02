#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use directories::ProjectDirs;
use overture_control::{ControlState, start_control_server};
use overture_core::{
    AppSettings, BackgroundSpec, BibleVerse, CoreCommand, DisplayMode, HorizontalAlign,
    LivePresentationItem, MediaItem, MediaKind, OutputRole, PresentationSlide, RgbaColor, Service,
    ServiceItem, ServiceItemKind, ServiceItemParams, Song, SongSlide, StateEngine, Theme,
    ThemeConfig,
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
use overture_storage::{Storage, create_full_backup, restore_full_backup};
use overture_ui::{
    MainWindow, UiBibleRow, UiMediaRow, UiMemberCountRow, UiPlaylistRow, UiReportStatRow,
    UiScheduleItem, UiSlideCard, UiSongRow, UiThemeRow, ui_strings_for_locale,
};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode, VecModel};
use std::cell::RefCell;
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
    engine: StateEngine,
    media_engine: MediaEngine,
    output_mgr: Option<OutputManager>,
    ctrl_state: ControlState,
    i18n: I18n,
    settings: AppSettings,
    persistent_service: Service,
    temp_service: Service,
    using_temp_service: bool,
    cached_bible_results: Vec<BibleVerse>,
    cached_themes: Vec<Theme>,
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

    let storage = rt.block_on(Storage::connect_file(&db_path))?;
    rt.block_on(seed_initial_demo_data_if_empty(&storage))?;

    let settings = rt.block_on(storage.load_settings()).unwrap_or_default();
    let locale = Locale::from_code(&settings.locale);
    let i18n = I18n::new(locale);

    let engine = StateEngine::new();
    let themes = rt.block_on(storage.list_themes()).unwrap_or_default();
    if let Some(main_t) = themes.iter().find(|t| !t.is_return_theme) {
        engine.dispatch(CoreCommand::SetMainTheme {
            theme: Box::new(main_t.config.clone()),
        });
    }
    if let Some(ret_t) = themes.iter().find(|t| t.is_return_theme) {
        engine.dispatch(CoreCommand::SetReturnTheme {
            theme: Box::new(ret_t.config.clone()),
        });
    }

    // Set default logo fallback to bundled asset if available
    let bundled_logo = find_bundled_logo();
    engine.dispatch(CoreCommand::SetLogoPath {
        path: settings.logo_path.clone().or(bundled_logo),
    });

    let services = rt.block_on(storage.list_services(false))?;
    let persistent_service = match services.into_iter().next() {
        Some(s) => s,
        None => {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            rt.block_on(storage.create_service(&today, "Culto Principal"))?
        }
    };

    let media_engine = MediaEngine::spawn(640, 360);
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

    // Spawn execution logger subscriber on tokio runtime
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
    ui.set_language_choice_idx(if locale == Locale::En { 1 } else { 0 });
    ui.set_settings_api_port(settings.api_port.to_string().into());
    ui.set_settings_api_token(settings.api_token.clone().into());
    ui.set_settings_lan_url(lan_url.into());
    ui.set_shortcut_next(settings.shortcuts.next_slide.clone().into());
    ui.set_shortcut_prev(settings.shortcuts.prev_slide.clone().into());
    ui.set_shortcut_black(settings.shortcuts.black_screen.clone().into());
    ui.set_shortcut_logo(settings.shortcuts.logo_screen.clone().into());

    let monitors = enumerate_monitors();
    let mon_labels: Vec<slint::SharedString> = if monitors.is_empty() {
        vec!["Monitor 1 (Padrão)".into()]
    } else {
        monitors
            .iter()
            .map(|m| m.friendly_name.clone().into())
            .collect()
    };
    ui.set_monitor_choices(ModelRc::new(VecModel::from(mon_labels)));

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
        engine,
        media_engine,
        output_mgr,
        ctrl_state,
        i18n,
        settings,
        persistent_service,
        temp_service: Service::new_temporary("Playlist Temporária"),
        using_temp_service: false,
        cached_bible_results: Vec::new(),
        cached_themes: themes,
        preview_compositor: SlideCompositor::new(),
    }));

    refresh_all_ui_models(&ui, &mut ctx.borrow_mut());

    if is_smoke_test {
        println!("OVERTURE_SMOKE_OK");
        ctrl_handle.shutdown();
        ctx.borrow().media_engine.shutdown();
        return Ok(());
    }

    wire_callbacks(&ui, ctx.clone());

    // 250ms UI live preview + telemetry refresh timer
    let preview_timer = Timer::default();
    {
        let ui_weak = ui.as_weak();
        let ctx_t = ctx.clone();
        preview_timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
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

fn wire_callbacks(ui: &MainWindow, ctx: Rc<RefCell<AppContext>>) {
    // Keyboard shortcuts
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
            } else if eq_key(&k, &c.settings.shortcuts.black_screen) {
                c.engine.dispatch(CoreCommand::ToggleBlackScreen);
            } else if eq_key(&k, &c.settings.shortcuts.logo_screen) {
                c.engine.dispatch(CoreCommand::ToggleLogo);
            } else if eq_key(&k, &c.settings.shortcuts.clear_text) {
                c.engine.dispatch(CoreCommand::ClearPresentation);
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

    // Transport buttons
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
            let mut c = ctx.borrow_mut();
            if idx >= 0 {
                c.engine
                    .dispatch(CoreCommand::GotoSlide { index: idx as usize });
                if let Some(w) = ui_weak.upgrade() {
                    sync_output_state_to_ui(&w, &mut c);
                }
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
        ui.on_clear_output(move || {
            let mut c = ctx.borrow_mut();
            c.engine.dispatch(CoreCommand::ClearPresentation);
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ctx = ctx.clone();
        ui.on_toggle_auto_play(move |enabled| {
            let c = ctx.borrow();
            c.engine.dispatch(CoreCommand::SetAutoPlay {
                enabled,
                default_duration_secs: c.settings.auto_advance_default_secs.or(Some(6)),
            });
        });
    }

    // Countdown, Stage Timer, Scoreboard, Stage Alert
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_start_countdown_now(move |mins_str, label| {
            let mins: f64 = mins_str.trim().parse().unwrap_or(5.0);
            let secs = (mins * 60.0).round().max(1.0) as u64;
            let mut c = ctx.borrow_mut();
            c.engine.dispatch(CoreCommand::StartCountdown {
                label: label.to_string(),
                seconds: secs,
                show_on_main: true,
            });
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
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
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_trigger_stage_alert(move |msg| {
            let mut c = ctx.borrow_mut();
            let opt = if msg.trim().is_empty() {
                None
            } else {
                Some(msg.to_string())
            };
            c.engine.dispatch(CoreCommand::SetStageAlert { message: opt });
            if let Some(w) = ui_weak.upgrade() {
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }

    // Playlist & Service management
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_toggle_temp_playlist(move |use_temp| {
            let c = &mut *ctx.borrow_mut();
            c.using_temp_service = use_temp;
            if let Some(w) = ui_weak.upgrade() {
                refresh_playlist_ui(&w, c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_create_new_service(move |prefix| {
            let mut c = ctx.borrow_mut();
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let name = format!("{} {}", prefix, chrono::Local::now().format("%d/%m %H:%M"));
            if let Ok(svc) = c.rt.block_on(c.storage.create_service(&today, &name)) {
                c.persistent_service = svc;
                c.using_temp_service = false;
                if let Some(w) = ui_weak.upgrade() {
                    w.set_is_temp_playlist(false);
                    refresh_playlist_ui(&w, &c);
                    w.set_status_banner(format!("Novo culto criado: {name}").into());
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_add_quick_announcement(move |text| {
            if text.trim().is_empty() {
                return;
            }
            let mut c = ctx.borrow_mut();
            let slides: Vec<String> = split_lyrics_into_slides(text.as_str(), DEFAULT_MAX_LINE_CHARS)
                .into_iter()
                .map(|s| s.text)
                .collect();
            let item = ServiceItem {
                id: 0,
                service_id: c.active_service().id,
                order: c.active_service().items.len() as i32,
                kind: ServiceItemKind::Announcement,
                ref_id: None,
                title: text.lines().next().unwrap_or("Aviso").to_string(),
                subtitle: "Aviso".to_string(),
                params: ServiceItemParams {
                    custom_slides: slides,
                    ..Default::default()
                },
            };
            append_item_to_active_service(&mut c, item);
            if let Some(w) = ui_weak.upgrade() {
                refresh_playlist_ui(&w, &c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_present_playlist_row(move |row_idx| {
            let mut c = ctx.borrow_mut();
            if row_idx < 0 {
                return;
            }
            let Some(item) = c.active_service().items.get(row_idx as usize).cloned() else {
                return;
            };
            let fallback = c.engine.snapshot().main_theme;
            if let Ok(live) = c.rt.block_on(c.storage.resolve_service_item(&item, &fallback)) {
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
                    sync_output_state_to_ui(&w, &mut c);
                }
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_remove_playlist_row(move |row_idx| {
            let mut c = ctx.borrow_mut();
            if row_idx < 0 {
                return;
            }
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

    // Quick search & Library actions
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_quick_search_submit(move |q| {
            let query = q.trim();
            if query.is_empty() {
                return;
            }
            let mut c = ctx.borrow_mut();
            // Try Bible reference first (e.g. "João 3:16")
            if overture_storage::parse_bible_reference(query).is_some()
                && let Ok(verses) = c.rt.block_on(c.storage.search_bible(1, query, 30))
                && !verses.is_empty()
            {
                let slides = verses
                    .iter()
                    .enumerate()
                    .map(|(idx, v)| PresentationSlide {
                        index: idx,
                        text: v.text.clone(),
                        footer: Some(v.reference(Some("TB1917"))),
                        forced_break: false,
                        duration_secs: None,
                    })
                    .collect();
                let _ = c.rt.block_on(c.storage.record_bible_history(&verses[0]));
                c.engine.dispatch(CoreCommand::PresentItem {
                    item: Box::new(LivePresentationItem {
                        service_item_id: None,
                        song_id: None,
                        title: query.to_string(),
                        subtitle: "Escrituras".to_string(),
                        notes: String::new(),
                        kind: ServiceItemKind::Bible,
                        slides,
                        theme: ThemeConfig::scripture_preset(),
                        media_path: None,
                        media_kind: None,
                    }),
                    start_slide: 0,
                });
                if let Some(w) = ui_weak.upgrade() {
                    sync_output_state_to_ui(&w, &mut c);
                }
                return;
            }

            if let Ok(songs) = c.rt.block_on(c.storage.search_songs(query, false, None, 1))
                && let Some(first) = songs.first()
            {
                let theme = c.engine.snapshot().main_theme;
                c.sync_background_video_for_theme(&theme);
                let live = LivePresentationItem::from_song(first, None, theme);
                c.engine.dispatch(CoreCommand::PresentItem {
                    item: Box::new(live),
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
        ui.on_search_library_changed(move |q| {
            let c = ctx.borrow();
            if let Some(w) = ui_weak.upgrade() {
                refresh_songs_ui(&w, &c, q.as_str());
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_import_txt_dir(move |dir_str| {
            let path = Path::new(dir_str.trim());
            let c = ctx.borrow_mut();
            let status = match parse_txt_directory(path, "user_txt", DEFAULT_MAX_LINE_CHARS) {
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
                        "Importação TXT concluída: {} importadas | {} quebras forçadas | {} curtas (<250c) | {} duplicatas exatas ignoradas",
                        db_inserted,
                        report.forced_break_songs,
                        report.short_lyric_songs,
                        report.exact_duplicates_removed + db_skipped
                    )
                }
                Err(e) => format!("Erro ao importar pasta TXT: {e}"),
            };
            if let Some(w) = ui_weak.upgrade() {
                refresh_songs_ui(&w, &c, w.get_library_search_query().as_str());
                w.set_status_banner(status.into());
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_import_file_any(move |path_str| {
            let path = Path::new(path_str.trim());
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let mut c = ctx.borrow_mut();

            let msg = if ext == "pptx" {
                match parse_pptx_file(path) {
                    Ok(pres) => {
                        let count = pres.slide_texts.len();
                        let title = pres.title.clone();
                        let item = pres.into_service_item(
                            c.active_service().id,
                            c.active_service().items.len() as i32,
                        );
                        append_item_to_active_service(&mut c, item);
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
                // XML: try OpenLP -> OpenSong -> Zefania Bible
                let xml_raw = std::fs::read_to_string(path).unwrap_or_default();
                if xml_raw.contains("openlyrics")
                    && let Ok(song) = parse_openlp_file(path, DEFAULT_MAX_LINE_CHARS)
                {
                    let _ = c.rt.block_on(c.storage.insert_song(&song, true));
                    format!("Música OpenLP XML '{}' importada!", song.title)
                } else if xml_raw.contains("XMLBIBLE")
                    && let Ok((ver, verses)) =
                        parse_zefania_bible_xml(&xml_raw, "IMPORT", "pt-BR", "User Provided")
                {
                    let _ = c.rt.block_on(c.storage.import_bible_version(&ver, &verses));
                    format!("Bíblia Zefania '{}' ({} versículos) importada!", ver.name, verses.len())
                } else if let Ok(song) = parse_opensong_file(path, DEFAULT_MAX_LINE_CHARS) {
                    let _ = c.rt.block_on(c.storage.insert_song(&song, true));
                    format!("Música OpenSong '{}' importada!", song.title)
                } else {
                    "Formato de arquivo não reconhecido (use .pptx, .xml ou .json)".to_string()
                }
            };

            if let Some(w) = ui_weak.upgrade() {
                refresh_all_ui_models(&w, &mut c);
                w.set_status_banner(msg.into());
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_remove_duplicates_clicked(move || {
            let c = ctx.borrow();
            let removed = c.rt.block_on(c.storage.remove_exact_duplicates()).unwrap_or(0);
            if let Some(w) = ui_weak.upgrade() {
                refresh_songs_ui(&w, &c, w.get_library_search_query().as_str());
                w.set_status_banner(format!("{removed} duplicatas exatas removidas.").into());
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_song_add_to_service(move |song_id| {
            let mut c = ctx.borrow_mut();
            if let Ok(Some(song)) = c.rt.block_on(c.storage.get_song(song_id.into())) {
                let item = ServiceItem {
                    id: 0,
                    service_id: c.active_service().id,
                    order: c.active_service().items.len() as i32,
                    kind: ServiceItemKind::Song,
                    ref_id: Some(song.id),
                    title: song.title,
                    subtitle: song.artist,
                    params: ServiceItemParams::default(),
                };
                append_item_to_active_service(&mut c, item);
                if let Some(w) = ui_weak.upgrade() {
                    refresh_playlist_ui(&w, &c);
                    w.set_status_banner("Música adicionada ao culto.".into());
                }
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_song_project_now(move |song_id| {
            let mut c = ctx.borrow_mut();
            if let Ok(Some(song)) = c.rt.block_on(c.storage.get_song(song_id.into())) {
                let theme = c.engine.snapshot().main_theme;
                c.sync_background_video_for_theme(&theme);
                let live = LivePresentationItem::from_song(&song, None, theme);
                c.engine.dispatch(CoreCommand::PresentItem {
                    item: Box::new(live),
                    start_slide: 0,
                });
                if let Some(w) = ui_weak.upgrade() {
                    w.set_active_screen(0);
                    sync_output_state_to_ui(&w, &mut c);
                }
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_song_set_archived(move |song_id, archived| {
            let c = ctx.borrow();
            let _ = c.rt.block_on(c.storage.set_song_archived(song_id.into(), archived));
            if let Some(w) = ui_weak.upgrade() {
                refresh_songs_ui(&w, &c, w.get_library_search_query().as_str());
            }
        });
    }

    // Song Editor callbacks
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_song_open_editor(move |song_id| {
            let c = ctx.borrow();
            if let (Some(w), Ok(Some(song))) =
                (ui_weak.upgrade(), c.rt.block_on(c.storage.get_song(song_id.into())))
            {
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
                let cards: Vec<UiSlideCard> = song
                    .slides
                    .iter()
                    .enumerate()
                    .map(|(i, s)| UiSlideCard {
                        slide_idx: i as i32,
                        number_label: format!("{}", i + 1).into(),
                        body: s.text.clone().into(),
                        forced_break: s.forced_break,
                    })
                    .collect();
                w.set_editor_preview_slides(ModelRc::new(VecModel::from(cards)));
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
                w.set_editor_preview_slides(ModelRc::new(VecModel::from(Vec::<UiSlideCard>::new())));
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_editor_preview_split(move || {
            if let Some(w) = ui_weak.upgrade() {
                let slides = split_lyrics_into_slides(
                    w.get_editor_raw_lyrics().as_str(),
                    DEFAULT_MAX_LINE_CHARS,
                );
                let cards: Vec<UiSlideCard> = slides
                    .into_iter()
                    .enumerate()
                    .map(|(i, s)| UiSlideCard {
                        slide_idx: i as i32,
                        number_label: format!("{}", i + 1).into(),
                        body: s.text.into(),
                        forced_break: s.forced_break,
                    })
                    .collect();
                w.set_editor_preview_slides(ModelRc::new(VecModel::from(cards)));
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

            let c = ctx.borrow();
            let saved_msg = if song.id > 0 {
                let _ = c.rt.block_on(c.storage.update_song(&song));
                format!(
                    "Música '{}' atualizada ({} slides, {} quebras forçadas).",
                    song.title,
                    song.slides.len(),
                    forced_count
                )
            } else {
                match c.rt.block_on(c.storage.insert_song(&song, false)) {
                    Ok(overture_storage::SongInsertOutcome::Inserted { id, .. }) => {
                        w.set_editor_song_id(id as i32);
                        format!(
                            "Música '{}' salva ({} slides, {} quebras forçadas).",
                            song.title,
                            song.slides.len(),
                            forced_count
                        )
                    }
                    _ => "Música salva.".to_string(),
                }
            };
            refresh_songs_ui(&w, &c, w.get_library_search_query().as_str());
            w.set_status_banner(saved_msg.into());
        });
    }

    // Bible callbacks
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_version_changed(move |_| {
            let mut c = ctx.borrow_mut();
            if let Some(w) = ui_weak.upgrade() {
                let q = w.get_library_search_query().to_string();
                refresh_bible_ui(&w, &mut c, &q);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_search_submit(move |q| {
            let mut c = ctx.borrow_mut();
            if let Some(w) = ui_weak.upgrade() {
                refresh_bible_ui(&w, &mut c, q.as_str());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_project_verse(move |row_idx| {
            let mut c = ctx.borrow_mut();
            let Some(v) = c.cached_bible_results.get(row_idx as usize).cloned() else {
                return;
            };
            let _ = c.rt.block_on(c.storage.record_bible_history(&v));
            let versions = c.rt.block_on(c.storage.list_bible_versions()).unwrap_or_default();
            let abbr = versions
                .iter()
                .find(|ver| ver.id == v.version_id)
                .map(|ver| ver.abbr.as_str())
                .unwrap_or("");
            let ref_label = v.reference(Some(abbr));
            c.engine.dispatch(CoreCommand::PresentItem {
                item: Box::new(LivePresentationItem {
                    service_item_id: None,
                    song_id: None,
                    title: ref_label.clone(),
                    subtitle: abbr.to_string(),
                    notes: String::new(),
                    kind: ServiceItemKind::Bible,
                    slides: vec![PresentationSlide {
                        index: 0,
                        text: v.text,
                        footer: Some(ref_label),
                        forced_break: false,
                        duration_secs: None,
                    }],
                    theme: ThemeConfig::scripture_preset(),
                    media_path: None,
                    media_kind: None,
                }),
                start_slide: 0,
            });
            if let Some(w) = ui_weak.upgrade() {
                let q = w.get_library_search_query().to_string();
                refresh_bible_ui(&w, &mut c, &q);
                w.set_active_screen(0);
                sync_output_state_to_ui(&w, &mut c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_add_service(move |row_idx| {
            let mut c = ctx.borrow_mut();
            let Some(v) = c.cached_bible_results.get(row_idx as usize).cloned() else {
                return;
            };
            let ref_str = format!("{} {}:{}", v.book, v.chapter, v.verse);
            let item = ServiceItem {
                id: 0,
                service_id: c.active_service().id,
                order: c.active_service().items.len() as i32,
                kind: ServiceItemKind::Bible,
                ref_id: None,
                title: ref_str,
                subtitle: "Bíblia".to_string(),
                params: ServiceItemParams {
                    bible_version_id: Some(v.version_id),
                    custom_slides: vec![v.text],
                    ..Default::default()
                },
            };
            append_item_to_active_service(&mut c, item);
            if let Some(w) = ui_weak.upgrade() {
                refresh_playlist_ui(&w, &c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_bible_favorite_verse(move |row_idx| {
            let mut c = ctx.borrow_mut();
            if let Some(v) = c.cached_bible_results.get(row_idx as usize).cloned() {
                let _ = c.rt.block_on(c.storage.add_bible_favorite(&v));
                if let Some(w) = ui_weak.upgrade() {
                    let q = w.get_library_search_query().to_string();
                    refresh_bible_ui(&w, &mut c, &q);
                }
            }
        });
    }

    // Media callbacks
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_add_media_item(move |path_str| {
            let p = path_str.trim();
            if p.is_empty() {
                return;
            }
            let kind = if p.starts_with("motion://") {
                MediaKind::Video
            } else {
                Path::new(p)
                    .extension()
                    .and_then(|e| MediaKind::from_extension(&e.to_string_lossy()))
                    .unwrap_or(MediaKind::Video)
            };
            let c = ctx.borrow();
            let _ = c.rt.block_on(c.storage.upsert_media(&MediaItem {
                id: 0,
                name: String::new(),
                path: p.to_string(),
                kind,
                duration_secs: None,
                archived: false,
            }));
            if let Some(w) = ui_weak.upgrade() {
                refresh_media_ui(&w, &c);
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
                    title: m.name,
                    subtitle: m.kind.as_str().to_uppercase(),
                    params: ServiceItemParams::default(),
                };
                append_item_to_active_service(&mut c, item);
                if let Some(w) = ui_weak.upgrade() {
                    refresh_playlist_ui(&w, &c);
                }
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
                        subtitle: m.kind.as_str().to_string(),
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
                    w.set_active_screen(0);
                    sync_output_state_to_ui(&w, &mut c);
                }
            }
        });
    }

    // Theme Editor callbacks
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_select_theme_row(move |theme_id| {
            let mut c = ctx.borrow_mut();
            let Some(t) = c
                .cached_themes
                .iter()
                .find(|t| t.id == theme_id as i64)
                .cloned()
            else {
                return;
            };
            if let Some(w) = ui_weak.upgrade() {
                w.set_editor_theme_id(t.id as i32);
                w.set_editor_theme_name(t.name.into());
                w.set_editor_font_family(t.config.font_family.clone().into());
                w.set_editor_font_size(format!("{}", t.config.font_size as u32).into());
                w.set_editor_text_color(t.config.text_color.to_hex().into());
                w.set_editor_shadow_enabled(t.config.shadow.enabled);
                w.set_editor_is_return_theme(t.is_return_theme);
                w.set_editor_align_idx(match t.config.h_align {
                    HorizontalAlign::Left => 0,
                    HorizontalAlign::Center => 1,
                    HorizontalAlign::Right => 2,
                });
                match &t.config.background {
                    BackgroundSpec::SolidColor { color } => {
                        w.set_editor_bg_kind_idx(0);
                        w.set_editor_bg_value(color.to_hex().into());
                    }
                    BackgroundSpec::Gradient { top, .. } => {
                        w.set_editor_bg_kind_idx(1);
                        w.set_editor_bg_value(top.to_hex().into());
                    }
                    BackgroundSpec::Image { path, .. } => {
                        w.set_editor_bg_kind_idx(2);
                        w.set_editor_bg_value(path.clone().into());
                    }
                    BackgroundSpec::Video { path, .. } => {
                        w.set_editor_bg_kind_idx(3);
                        w.set_editor_bg_value(path.clone().into());
                    }
                }
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
        ui.on_apply_theme_live(move || {
            let mut c = ctx.borrow_mut();
            if let Some(w) = ui_weak.upgrade() {
                let cfg = read_theme_config_from_ui(&w);
                if w.get_editor_is_return_theme() {
                    c.engine
                        .dispatch(CoreCommand::SetReturnTheme { theme: Box::new(cfg) });
                } else {
                    c.sync_background_video_for_theme(&cfg);
                    c.engine
                        .dispatch(CoreCommand::SetMainTheme { theme: Box::new(cfg) });
                }
                sync_output_state_to_ui(&w, &mut c);
                w.set_status_banner("Tema aplicado na saída ao vivo!".into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_save_current_theme(move || {
            let mut c = ctx.borrow_mut();
            if let Some(w) = ui_weak.upgrade() {
                let cfg = read_theme_config_from_ui(&w);
                let theme = Theme {
                    id: w.get_editor_theme_id() as i64,
                    name: w.get_editor_theme_name().trim().to_string(),
                    is_return_theme: w.get_editor_is_return_theme(),
                    config: cfg,
                };
                if let Ok(id) = c.rt.block_on(c.storage.save_theme(&theme)) {
                    w.set_editor_theme_id(id as i32);
                    refresh_themes_ui(&w, &mut c);
                    w.set_status_banner("Tema salvo no banco SQLite.".into());
                }
            }
        });
    }

    // Monitor outputs
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_apply_monitor_outputs(move || {
            let c = ctx.borrow_mut();
            let Some(w) = ui_weak.upgrade() else {
                return;
            };
            // Update Return Theme selection first
            let ret_themes: Vec<Theme> = c
                .cached_themes
                .iter()
                .filter(|t| t.is_return_theme)
                .cloned()
                .collect();
            if let Some(rt) = ret_themes.get(w.get_return_theme_choice_idx() as usize) {
                c.engine.dispatch(CoreCommand::SetReturnTheme {
                    theme: Box::new(rt.config.clone()),
                });
            }

            if let Some(mgr) = &c.output_mgr {
                if w.get_main_monitor_enabled() {
                    let _ = mgr.open_window(RoleWindowTarget {
                        role: OutputRole::Main,
                        monitor_index: w.get_main_monitor_idx().max(0) as usize,
                        fullscreen: true,
                        topmost: true,
                    });
                } else {
                    let _ = mgr.close_window(OutputRole::Main);
                }

                if w.get_stage_monitor_enabled() {
                    let _ = mgr.open_window(RoleWindowTarget {
                        role: OutputRole::Stage,
                        monitor_index: w.get_stage_monitor_idx().max(0) as usize,
                        fullscreen: true,
                        topmost: false,
                    });
                } else {
                    let _ = mgr.close_window(OutputRole::Stage);
                }

                if w.get_return_monitor_enabled() {
                    let _ = mgr.open_window(RoleWindowTarget {
                        role: OutputRole::Return,
                        monitor_index: w.get_return_monitor_idx().max(0) as usize,
                        fullscreen: true,
                        topmost: false,
                    });
                } else {
                    let _ = mgr.close_window(OutputRole::Return);
                }
                w.set_status_banner("Saídas de monitor atualizadas (Esc na janela fecha individualmente).".into());
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
                w.set_main_monitor_enabled(false);
                w.set_stage_monitor_enabled(false);
                w.set_return_monitor_enabled(false);
                w.set_status_banner("Todas as janelas de saída foram fechadas.".into());
            }
        });
    }

    // Schedule & Reports callbacks
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_add_schedule_member(move || {
            let c = ctx.borrow();
            let Some(w) = ui_weak.upgrade() else {
                return;
            };
            let member_name = w.get_schedule_member_input().trim().to_string();
            let role = w.get_schedule_role_input().trim().to_string();
            if member_name.is_empty() || role.is_empty() {
                return;
            }
            if let Ok(m) = c.rt.block_on(c.storage.upsert_team_member(&member_name)) {
                let _ = c.rt.block_on(c.storage.add_schedule_entry(
                    c.persistent_service.id,
                    m.id,
                    &role,
                ));
                w.set_schedule_member_input("".into());
                refresh_schedule_and_reports_ui(&w, &c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_remove_schedule_member(move |entry_id| {
            let c = ctx.borrow();
            let _ = c
                .rt
                .block_on(c.storage.remove_schedule_entry(entry_id as i64));
            if let Some(w) = ui_weak.upgrade() {
                refresh_schedule_and_reports_ui(&w, &c);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_refresh_reports(move || {
            let c = ctx.borrow();
            if let Some(w) = ui_weak.upgrade() {
                refresh_schedule_and_reports_ui(&w, &c);
            }
        });
    }

    // Settings, Real-Time Language Switch, Backup & Export
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
                w.set_tr(ui_strings_for_locale(next_loc));
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
                w.set_status_banner("Configurações e token da API atualizados com sucesso.".into());
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_run_full_backup(move |dest_str| {
            let c = ctx.borrow();
            let dest = if dest_str.trim().is_empty() {
                c.db_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("overture-backup.zip")
            } else {
                let p = PathBuf::from(dest_str.trim());
                if p.extension().is_none() {
                    p.join("overture-backup.zip")
                } else {
                    p
                }
            };
            let msg = match c
                .rt
                .block_on(create_full_backup(c.storage.pool(), &c.media_dir, &dest))
            {
                Ok(m) => format!(
                    "Backup criado em {} ({} mídias empacotadas).",
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
        ui.on_run_restore_backup(move |src_str| {
            let mut c = ctx.borrow_mut();
            let src = PathBuf::from(src_str.trim());
            if !src.exists() {
                if let Some(w) = ui_weak.upgrade() {
                    w.set_status_banner("Arquivo de backup (.zip) não encontrado.".into());
                }
                return;
            }
            c.rt.block_on(c.storage.close());
            let msg = match restore_full_backup(&src, &c.db_path, &c.media_dir) {
                Ok(_) => match c.rt.block_on(Storage::connect_file(&c.db_path)) {
                    Ok(reopened) => {
                        c.storage = reopened.clone();
                        c.ctrl_state.storage = reopened;
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
    {
        let ui_weak = ui.as_weak();
        let ctx = ctx.clone();
        ui.on_run_export_json(move |dest_str| {
            let c = ctx.borrow();
            let dest = if dest_str.trim().is_empty() {
                c.db_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("overture-library.json")
            } else {
                let p = PathBuf::from(dest_str.trim());
                if p.is_dir() {
                    p.join("overture-library.json")
                } else {
                    p
                }
            };
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
                Ok(()) => format!("Biblioteca exportada para JSON em {}", dest.display()),
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
        ui.on_run_export_txt(move |dir_str| {
            let c = ctx.borrow();
            let out_dir = if dir_str.trim().is_empty() {
                c.db_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("export_txt")
            } else {
                PathBuf::from(dir_str.trim())
            };
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

fn refresh_all_ui_models(ui: &MainWindow, c: &mut AppContext) {
    refresh_playlist_ui(ui, c);
    refresh_songs_ui(ui, c, "");
    refresh_bible_ui(ui, c, "");
    refresh_media_ui(ui, c);
    refresh_themes_ui(ui, c);
    refresh_schedule_and_reports_ui(ui, c);
    sync_output_state_to_ui(ui, c);
    render_theme_preview_into_ui(ui, c);
}

fn refresh_playlist_ui(ui: &MainWindow, c: &AppContext) {
    let svc = c.active_service();
    ui.set_active_service_title(svc.name.clone().into());
    let rows: Vec<UiPlaylistRow> = svc
        .items
        .iter()
        .enumerate()
        .map(|(idx, it)| UiPlaylistRow {
            item_id: it.id as i32,
            row_idx: idx as i32,
            kind_badge: match it.kind {
                ServiceItemKind::Song => "MÚS".into(),
                ServiceItemKind::Bible => "BÍB".into(),
                ServiceItemKind::Media => "MÍD".into(),
                ServiceItemKind::Announcement => "AVI".into(),
                ServiceItemKind::Countdown => "REG".into(),
                ServiceItemKind::Presentation => "PPT".into(),
            },
            title: it.title.clone().into(),
            subtitle: it.subtitle.clone().into(),
        })
        .collect();
    ui.set_playlist_rows(ModelRc::new(VecModel::from(rows)));
}

fn refresh_songs_ui(ui: &MainWindow, c: &AppContext, query: &str) {
    if let Ok(active_songs) = c.rt.block_on(c.storage.search_songs(query, false, None, 300)) {
        let rows: Vec<UiSongRow> = active_songs
            .into_iter()
            .map(|s| {
                let fb = s.slides.iter().any(|sl| sl.forced_break);
                UiSongRow {
                    song_id: s.id as i32,
                    title: s.title.into(),
                    artist: if s.artist.is_empty() {
                        "—".into()
                    } else {
                        s.artist.into()
                    },
                    meta: format!("{} slides", s.slides.len()).into(),
                    has_forced_break: fb,
                    duplicate_marked: s.duplicate_marked,
                    archived: s.archived,
                }
            })
            .collect();
        ui.set_song_rows(ModelRc::new(VecModel::from(rows)));
    }

    if let Ok(all_songs) = c.rt.block_on(c.storage.search_songs("", true, None, 500)) {
        let arch: Vec<UiSongRow> = all_songs
            .into_iter()
            .filter(|s| s.archived)
            .map(|s| UiSongRow {
                song_id: s.id as i32,
                title: s.title.into(),
                artist: s.artist.into(),
                meta: format!("{} slides", s.slides.len()).into(),
                has_forced_break: false,
                duplicate_marked: false,
                archived: true,
            })
            .collect();
        ui.set_archived_song_rows(ModelRc::new(VecModel::from(arch)));
    }
}

fn refresh_bible_ui(ui: &MainWindow, c: &mut AppContext, query: &str) {
    let versions = c.rt.block_on(c.storage.list_bible_versions()).unwrap_or_default();
    let ver_labels: Vec<slint::SharedString> = versions
        .iter()
        .map(|v| format!("{} — {} [{}]", v.abbr, v.name, v.license).into())
        .collect();
    if !ver_labels.is_empty() {
        ui.set_bible_version_names(ModelRc::new(VecModel::from(ver_labels)));
    }
    let sel_idx = (ui.get_selected_bible_version_idx().max(0) as usize).min(versions.len().saturating_sub(1));
    let ver_id = versions.get(sel_idx).map(|v| v.id).unwrap_or(1);
    let ver_abbr = versions
        .get(sel_idx)
        .map(|v| v.abbr.clone())
        .unwrap_or_else(|| "TB1917".to_string());

    let verses = c
        .rt
        .block_on(c.storage.search_bible(ver_id, query, 80))
        .unwrap_or_default();
    let rows: Vec<UiBibleRow> = verses
        .iter()
        .map(|v| UiBibleRow {
            version_id: v.version_id as i32,
            book: v.book.clone().into(),
            chapter: v.chapter,
            verse: v.verse,
            reference: v.reference(Some(&ver_abbr)).into(),
            body: v.text.clone().into(),
        })
        .collect();
    c.cached_bible_results = verses;
    ui.set_bible_rows(ModelRc::new(VecModel::from(rows)));

    let favs = c.rt.block_on(c.storage.list_bible_favorites()).unwrap_or_default();
    let fav_rows: Vec<UiBibleRow> = favs
        .into_iter()
        .map(|f| UiBibleRow {
            version_id: f.version_id as i32,
            reference: format!("{} {}:{}", f.book, f.chapter, f.verse).into(),
            book: f.book.into(),
            chapter: f.chapter,
            verse: f.verse,
            body: f.text.into(),
        })
        .collect();
    ui.set_bible_fav_rows(ModelRc::new(VecModel::from(fav_rows)));

    let hist = c.rt.block_on(c.storage.list_bible_history(20)).unwrap_or_default();
    let hist_rows: Vec<UiBibleRow> = hist
        .into_iter()
        .map(|h| UiBibleRow {
            version_id: h.version_id as i32,
            reference: h.reference(None).into(),
            book: h.book.into(),
            chapter: h.chapter,
            verse: h.verse,
            body: h.text.into(),
        })
        .collect();
    ui.set_bible_history_rows(ModelRc::new(VecModel::from(hist_rows)));
}

fn refresh_media_ui(ui: &MainWindow, c: &AppContext) {
    if let Ok(items) = c.rt.block_on(c.storage.list_media(false)) {
        let rows: Vec<UiMediaRow> = items
            .into_iter()
            .map(|m| UiMediaRow {
                media_id: m.id as i32,
                name: m.name.into(),
                kind_label: m.kind.as_str().to_uppercase().into(),
                path: m.path.into(),
            })
            .collect();
        ui.set_media_rows(ModelRc::new(VecModel::from(rows)));
    }
}

fn refresh_themes_ui(ui: &MainWindow, c: &mut AppContext) {
    if let Ok(themes) = c.rt.block_on(c.storage.list_themes()) {
        let rows: Vec<UiThemeRow> = themes
            .iter()
            .map(|t| UiThemeRow {
                theme_id: t.id as i32,
                name: t.name.clone().into(),
                summary: format!(
                    "{} {}pt {}",
                    t.config.font_family,
                    t.config.font_size as u32,
                    if t.is_return_theme { "[Retorno]" } else { "" }
                )
                .into(),
                is_return: t.is_return_theme,
            })
            .collect();
        let ret_choices: Vec<slint::SharedString> = themes
            .iter()
            .filter(|t| t.is_return_theme)
            .map(|t| t.name.clone().into())
            .collect();
        if !ret_choices.is_empty() {
            ui.set_return_theme_choices(ModelRc::new(VecModel::from(ret_choices)));
        }
        c.cached_themes = themes;
        ui.set_theme_rows(ModelRc::new(VecModel::from(rows)));
    }
}

fn refresh_schedule_and_reports_ui(ui: &MainWindow, c: &AppContext) {
    if let Ok(sched) = c
        .rt
        .block_on(c.storage.list_service_schedule(c.persistent_service.id))
    {
        let rows: Vec<UiScheduleItem> = sched
            .into_iter()
            .map(|s| UiScheduleItem {
                entry_id: s.id as i32,
                member_name: s.member_name.into(),
                role_label: s.role.into(),
            })
            .collect();
        ui.set_current_schedule_rows(ModelRc::new(VecModel::from(rows)));
    }

    if let Ok(counts) = c.rt.block_on(c.storage.report_member_schedule_counts()) {
        let rows: Vec<UiMemberCountRow> = counts
            .into_iter()
            .map(|m| UiMemberCountRow {
                member_name: m.member_name.into(),
                count_label: format!("{}x", m.times_scheduled).into(),
            })
            .collect();
        ui.set_member_count_rows(ModelRc::new(VecModel::from(rows)));
    }

    let from_d = ui.get_report_from_date().trim().to_string();
    let to_d = ui.get_report_to_date().trim().to_string();
    let from_opt = (!from_d.is_empty()).then_some(from_d.as_str());
    let to_iso = if to_d.is_empty() {
        None
    } else {
        Some(format!("{to_d}T23:59:59Z"))
    };
    let wk_idx = ui.get_report_weekday_idx();
    let weekday = if wk_idx > 0 {
        Some((wk_idx - 1) as u32)
    } else {
        None
    };

    if let Ok(stats) = c.rt.block_on(c.storage.report_most_played_songs(
        from_opt,
        to_iso.as_deref(),
        weekday,
        50,
    )) {
        let rows: Vec<UiReportStatRow> = stats
            .into_iter()
            .map(|st| UiReportStatRow {
                title: st.title.into(),
                artist: st.artist.into(),
                count_label: format!("{} execuções", st.play_count).into(),
            })
            .collect();
        ui.set_most_played_rows(ModelRc::new(VecModel::from(rows)));
    }
}

fn sync_output_state_to_ui(ui: &MainWindow, c: &mut AppContext) {
    let st = c.engine.snapshot();
    ui.set_active_slide_idx(st.current_slide_index as i32);
    ui.set_black_active(st.display_mode == DisplayMode::BlackScreen);
    ui.set_logo_active(st.display_mode == DisplayMode::Logo);
    ui.set_stage_timer_text(st.formatted_stage_timer().into());
    ui.set_countdown_display(st.countdown.formatted_mm_ss().into());
    ui.set_team_a_score(st.scoreboard.team_a_score);
    ui.set_team_b_score(st.scoreboard.team_b_score);

    match &st.current_item {
        Some(item) => {
            ui.set_live_item_header(
                format!(
                    "{} — {} ({}/{})",
                    item.title,
                    item.subtitle,
                    (st.current_slide_index + 1).min(item.slides.len().max(1)),
                    item.slides.len()
                )
                .into(),
            );
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
            ui.set_live_slides(ModelRc::new(VecModel::from(cards)));
            let nxt = st
                .next_slide()
                .map(|s| s.text.as_str())
                .unwrap_or("[Fim do item]");
            ui.set_next_slide_text(nxt.into());
        }
        None => {
            ui.set_live_item_header("Sem item ativo".into());
            ui.set_live_slides(ModelRc::new(VecModel::from(Vec::<UiSlideCard>::new())));
            ui.set_next_slide_text("—".into());
        }
    }

    update_live_preview_and_telemetry(ui, c);
}

fn update_live_preview_and_telemetry(ui: &MainWindow, c: &mut AppContext) {
    let st = c.engine.snapshot();
    let vf = c.media_engine.subscribe_frames().borrow().clone();
    let vf_view = vf.as_ref().map(|v| v.as_view());

    let frame = c
        .preview_compositor
        .compose(&st, OutputRole::Main, 384, 216, vf_view);
    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        &frame.pixels,
        frame.width,
        frame.height,
    );
    ui.set_live_main_preview(Image::from_rgba8(buf));

    if ui.get_active_screen() == 3 && ui.get_editor_bg_kind_idx() == 3 {
        render_theme_preview_into_ui(ui, c);
    }

    if let Some(mgr) = &c.output_mgr {
        ui.set_latency_readout(
            format!(
                "{:.2} ms ({} frames)",
                mgr.telemetry().last_ms(),
                mgr.telemetry().frames_presented()
            )
            .into(),
        );
    }
}

fn read_theme_config_from_ui(ui: &MainWindow) -> ThemeConfig {
    let mut cfg = ThemeConfig::default();
    let ff = ui.get_editor_font_family().trim().to_string();
    if !ff.is_empty() {
        cfg.font_family = ff;
    }
    if let Ok(sz) = ui.get_editor_font_size().trim().parse::<f32>() {
        cfg.font_size = sz.clamp(18.0, 160.0);
    }
    if let Some(col) = RgbaColor::from_hex(ui.get_editor_text_color().as_str()) {
        cfg.text_color = col;
    }
    cfg.shadow.enabled = ui.get_editor_shadow_enabled();
    cfg.h_align = match ui.get_editor_align_idx() {
        0 => HorizontalAlign::Left,
        2 => HorizontalAlign::Right,
        _ => HorizontalAlign::Center,
    };
    let bg_val = ui.get_editor_bg_value().trim().to_string();
    cfg.background = match ui.get_editor_bg_kind_idx() {
        0 => BackgroundSpec::SolidColor {
            color: RgbaColor::from_hex(&bg_val).unwrap_or(RgbaColor::new(10, 15, 30, 255)),
        },
        1 => BackgroundSpec::Gradient {
            top: RgbaColor::from_hex(&bg_val).unwrap_or(RgbaColor::new(15, 28, 62, 255)),
            bottom: RgbaColor::new(3, 6, 16, 255),
        },
        2 => BackgroundSpec::Image {
            path: bg_val,
            dim_opacity: 0.35,
        },
        _ => BackgroundSpec::Video {
            path: if bg_val.is_empty() {
                "motion://aurora".to_string()
            } else {
                bg_val
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
    ui.set_theme_editor_preview(Image::from_rgba8(buf));
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

    let hymns = [
        (
            "Castelo Forte",
            "Martinho Lutero",
            "Castelo forte é nosso Deus,\nEspada e bom escudo;\n\nCom seu poder defende os seus\nEm todo transe agudo.",
        ),
        (
            "Santo, Santo, Santo",
            "Reginald Heber",
            "Santo! Santo! Santo!\nDeus Onipotente!\n\nCedo de manhã\nCantaremos Teu louvor!",
        ),
        (
            "Grandioso És Tu",
            "Carl Boberg",
            "Senhor meu Deus, quando eu maravilhado,\nContemplo a Tua imensa criação,\n\nEntão minh'alma canta a Ti, Senhor:\nGrandioso és Tu! Grandioso és Tu!",
        ),
    ];

    let mut first_song_id = None;
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
            && first_song_id.is_none()
        {
            first_song_id = Some(id);
        }
    }

    for (name, uri) in [
        ("Motion — Aurora Boreal", "motion://aurora"),
        ("Motion — Oceano Profundo", "motion://ocean"),
        ("Motion — Luz Dourada", "motion://warm"),
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
    let svc = storage.create_service(&today, "Culto de Domingo").await?;
    if let Some(sid) = first_song_id {
        storage
            .replace_service_items(
                svc.id,
                &[
                    ServiceItem {
                        id: 0,
                        service_id: svc.id,
                        order: 0,
                        kind: ServiceItemKind::Song,
                        ref_id: Some(sid),
                        title: "Castelo Forte".to_string(),
                        subtitle: "Martinho Lutero".to_string(),
                        params: ServiceItemParams::default(),
                    },
                    ServiceItem {
                        id: 0,
                        service_id: svc.id,
                        order: 1,
                        kind: ServiceItemKind::Bible,
                        ref_id: None,
                        title: "Salmos 23:1-3".to_string(),
                        subtitle: "TB1917".to_string(),
                        params: ServiceItemParams {
                            bible_version_id: Some(1),
                            ..Default::default()
                        },
                    },
                ],
            )
            .await?;
    }

    Ok(())
}

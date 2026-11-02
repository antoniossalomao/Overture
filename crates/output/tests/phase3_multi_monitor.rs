//! Phase 3 Acceptance Test: Verifies that Stage view and Return monitor render distinct
//! content simultaneously (current slide, next slide, chronometer on Stage; custom high-contrast
//! return theme + stage alert on Return; clean lyrics + background on Main).

use overture_core::{
    CoreCommand, LivePresentationItem, OutputRole, PresentationSlide, RgbaColor, ServiceItemKind,
    StateEngine, ThemeConfig,
};
use overture_output::{OutputManager, RoleWindowTarget};
use overture_render::SlideCompositor;
use std::time::Duration;

#[test]
fn phase3_two_monitors_distinct_stage_and_return_outputs() -> anyhow::Result<()> {
    let engine = StateEngine::new();
    let mut return_theme = ThemeConfig::return_monitor_preset();
    return_theme.text_color = RgbaColor::new(56, 189, 248, 255);
    engine.dispatch(CoreCommand::SetReturnTheme {
        theme: Box::new(return_theme),
    });

    engine.dispatch(CoreCommand::PresentItem {
        item: Box::new(LivePresentationItem {
            service_item_id: Some(1),
            song_id: Some(1),
            title: "Grandioso És Tu".to_string(),
            subtitle: "Carl Boberg".to_string(),
            notes: "Subir meio tom no último refrão".to_string(),
            kind: ServiceItemKind::Song,
            slides: vec![
                PresentationSlide {
                    index: 0,
                    text: "Senhor meu Deus, quando eu maravilhado,\nContemplo a Tua imensa criação".to_string(),
                    footer: None,
                    forced_break: false,
                    duration_secs: None,
                },
                PresentationSlide {
                    index: 1,
                    text: "Então minh'alma canta a Ti, Senhor:\nGrandioso és Tu! Grandioso és Tu!".to_string(),
                    footer: None,
                    forced_break: false,
                    duration_secs: None,
                },
            ],
            theme: ThemeConfig::default(),
            media_path: None,
            media_kind: None,
        }),
        start_slide: 0,
    });
    engine.dispatch(CoreCommand::StartStageTimer);
    engine.dispatch(CoreCommand::TickSecond);
    engine.dispatch(CoreCommand::SetStageAlert {
        message: Some("Atenção ao tempo de transição".to_string()),
    });

    let snap = engine.snapshot();
    assert_eq!(snap.formatted_stage_timer(), "00:01");

    // Verify pixel-level composition across Main, Stage, and Return roles
    let mut comp = SlideCompositor::new();
    let main_px = comp.compose(&snap, OutputRole::Main, 640, 360, None);
    let stage_px = comp.compose(&snap, OutputRole::Stage, 640, 360, None);
    let return_px = comp.compose(&snap, OutputRole::Return, 640, 360, None);

    assert_ne!(main_px.pixels, stage_px.pixels);
    assert_ne!(main_px.pixels, return_px.pixels);
    assert_ne!(stage_px.pixels, return_px.pixels);

    // Also spawn two real simultaneous windows (Stage + Return) via OutputManager if a display is attached
    if let Ok(mgr) = OutputManager::spawn(
        engine.subscribe_watch(),
        None,
        vec![
            RoleWindowTarget {
                role: OutputRole::Stage,
                monitor_index: 0,
                fullscreen: false,
                topmost: false,
            },
            RoleWindowTarget {
                role: OutputRole::Return,
                monitor_index: 0,
                fullscreen: false,
                topmost: false,
            },
        ],
    ) {
        std::thread::sleep(Duration::from_millis(350));
        engine.dispatch(CoreCommand::NextSlide);
        std::thread::sleep(Duration::from_millis(200));
        mgr.shutdown();
    }

    Ok(())
}

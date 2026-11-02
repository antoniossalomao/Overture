//! Fase 0 Spike: Abre janela fullscreen num monitor escolhido com texto renderizado via `cosmic-text` + `wgpu`,
//! mede a latência entre o comando e a apresentação na GPU e encerra limpa e automaticamente se `--auto-close-ms` for passado.

use overture_core::{
    CoreCommand, LivePresentationItem, OutputRole, PresentationSlide, ServiceItemKind,
    StateEngine, ThemeConfig,
};
use overture_output::{
    OVERTURE_APP_USER_MODEL_ID, OutputManager, RoleWindowTarget, enumerate_monitors,
    init_windows_process_identity,
};
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    init_windows_process_identity(OVERTURE_APP_USER_MODEL_ID);

    let args: Vec<String> = std::env::args().collect();
    let mut monitor_index: usize = 0;
    let mut auto_close_ms: Option<u64> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--monitor" if i + 1 < args.len() => {
                monitor_index = args[i + 1].parse().unwrap_or(0);
                i += 2;
            }
            "--auto-close-ms" if i + 1 < args.len() => {
                auto_close_ms = args[i + 1].parse().ok();
                i += 2;
            }
            _ => i += 1,
        }
    }

    let monitors = enumerate_monitors();
    println!("Monitores detectados ({}):", monitors.len());
    for m in &monitors {
        println!(
            "  [{}] {} — {} ({}x{} @ {},{})",
            m.index, m.friendly_name, m.system_name, m.width, m.height, m.x, m.y
        );
    }

    let engine = StateEngine::new();
    engine.dispatch(CoreCommand::PresentItem {
        item: Box::new(LivePresentationItem {
            service_item_id: None,
            song_id: None,
            title: "Fase 0 — Spike Overture".to_string(),
            subtitle: "Saída Fullscreen wgpu + cosmic-text".to_string(),
            notes: "Teste de latência e nitidez".to_string(),
            kind: ServiceItemKind::Song,
            slides: vec![
                PresentationSlide {
                    index: 0,
                    text: "Santo, Santo, Santo!\nDeus Onipotente!".to_string(),
                    footer: None,
                    forced_break: false,
                    duration_secs: None,
                },
                PresentationSlide {
                    index: 1,
                    text: "Toda a terra está cheia\nDa Sua glória!".to_string(),
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

    let t_open = Instant::now();
    let manager = OutputManager::spawn(
        engine.subscribe_watch(),
        None,
        vec![RoleWindowTarget {
            role: OutputRole::Main,
            monitor_index,
            fullscreen: true,
            topmost: true,
        }],
    )?;

    let wait_ms = auto_close_ms.unwrap_or(3000);
    std::thread::sleep(Duration::from_millis(wait_ms / 2));

    // Trigger slide transition and measure command -> GPU present latency
    engine.dispatch(CoreCommand::NextSlide);
    std::thread::sleep(Duration::from_millis(wait_ms / 2));

    println!(
        "Spike concluído em {:?} | Frames apresentados: {} | Última latência comando->GPU: {:.2} ms (máx {:.2} ms)",
        t_open.elapsed(),
        manager.telemetry().frames_presented(),
        manager.telemetry().last_ms(),
        manager.telemetry().max_ms(),
    );

    manager.shutdown();
    Ok(())
}

//! Primeira execução: baixa o pacote de letras da release do Overture e importa em lotes.

use overture_import::read_letras_pack;
use overture_storage::Storage;
use slint::{ComponentHandle, Weak};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Release fixa (`letras-latest`) atualizada pelo workflow `letras-pack.yml`.
pub const LETRAS_PACK_URL: &str =
    "https://github.com/antoniossalomao/Overture/releases/download/letras-latest/overture-letras.zip";

fn mark_done(rt: &tokio::runtime::Runtime, storage: &Storage) {
    if let Ok(mut s) = rt.block_on(storage.load_settings()) {
        s.onboarding_done = true;
        let _ = rt.block_on(storage.save_settings(&s));
    }
}

/// Mostra a tela de boas-vindas (se ainda não concluída) e liga os callbacks.
pub fn wire(
    ui: &overture_ui::MainWindow,
    storage: Storage,
    rt: Arc<tokio::runtime::Runtime>,
    work_dir: PathBuf,
    already_done: bool,
    headless: bool,
) {
    if !already_done && !headless {
        ui.set_onboarding_step(0);
    }
    let running = Arc::new(AtomicBool::new(false));
    let hidden = Arc::new(AtomicBool::new(false));

    {
        let (storage, rt) = (storage.clone(), rt.clone());
        let (running, hidden) = (running.clone(), hidden.clone());
        let weak = ui.as_weak();
        ui.on_onboarding_start(move || {
            if running.swap(true, Ordering::SeqCst) {
                return;
            }
            hidden.store(false, Ordering::SeqCst);
            let Some(w) = weak.upgrade() else { return };
            w.set_onboarding_step(1);
            w.set_onboarding_progress(0.0);
            w.set_onboarding_message("Baixando a coletânea de letras...".into());
            w.set_onboarding_detail("".into());
            let (storage, rt) = (storage.clone(), rt.clone());
            let (running, hidden) = (running.clone(), hidden.clone());
            let weak = weak.clone();
            let zip_path = work_dir.join("overture-letras.zip");
            std::thread::spawn(move || {
                let result = run_import(&weak, &hidden, &storage, &rt, &zip_path);
                let _ = std::fs::remove_file(&zip_path);
                running.store(false, Ordering::SeqCst);
                let weak2 = weak.clone();
                let hidden2 = hidden.clone();
                match result {
                    Ok(n) => {
                        mark_done(&rt, &storage);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = weak2.upgrade() {
                                let msg = format!("{n} letras importadas para a sua biblioteca.");
                                if hidden2.load(Ordering::SeqCst) {
                                    w.set_status_banner(msg.into());
                                    let q = w.get_library_search_query().to_string();
                                    w.invoke_search_songs(q.into());
                                } else {
                                    w.set_onboarding_progress(1.0);
                                    w.set_onboarding_message(msg.into());
                                    w.set_onboarding_step(2);
                                }
                            }
                        });
                    }
                    Err(e) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = weak2.upgrade() {
                                let msg = format!("Falha ao baixar/importar as letras: {e}");
                                if hidden2.load(Ordering::SeqCst) {
                                    w.set_status_banner(msg.into());
                                } else {
                                    w.set_onboarding_message(msg.into());
                                    w.set_onboarding_step(3);
                                }
                            }
                        });
                    }
                }
            });
        });
    }
    {
        let (storage, rt, hidden) = (storage.clone(), rt.clone(), hidden.clone());
        let weak = ui.as_weak();
        ui.on_onboarding_skip(move || {
            hidden.store(true, Ordering::SeqCst);
            mark_done(&rt, &storage);
            if let Some(w) = weak.upgrade() {
                w.set_onboarding_step(-1);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_onboarding_finish(move || {
            if let Some(w) = weak.upgrade() {
                w.set_onboarding_step(-1);
                let q = w.get_library_search_query().to_string();
                w.invoke_search_songs(q.into());
            }
        });
    }
}

fn set_progress(weak: &Weak<overture_ui::MainWindow>, hidden: &AtomicBool, p: f32, detail: String) {
    if hidden.load(Ordering::SeqCst) {
        return;
    }
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = weak.upgrade() {
            w.set_onboarding_progress(p);
            w.set_onboarding_detail(detail.into());
        }
    });
}

fn run_import(
    weak: &Weak<overture_ui::MainWindow>,
    hidden: &AtomicBool,
    storage: &Storage,
    rt: &tokio::runtime::Runtime,
    zip_path: &std::path::Path,
) -> anyhow::Result<usize> {
    use std::io::Write;
    // 1. download (0–60 % da barra)
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .user_agent("overture-onboarding")
            .build()?;
        let mut resp = client.get(LETRAS_PACK_URL).send().await?.error_for_status()?;
        let total = resp.content_length();
        let mut file = std::fs::File::create(zip_path)?;
        let mut got: u64 = 0;
        while let Some(chunk) = resp.chunk().await? {
            file.write_all(&chunk)?;
            got += chunk.len() as u64;
            let frac = total.map(|t| got as f32 / t as f32).unwrap_or(0.0);
            set_progress(
                weak,
                hidden,
                0.6 * frac,
                format!("Baixando… {:.1} MB", got as f64 / 1_048_576.0),
            );
        }
        file.flush()?;
        anyhow::Ok(())
    })?;

    // 2. importação (60–100 %)
    if !hidden.load(Ordering::SeqCst) {
        let w2 = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = w2.upgrade() {
                w.set_onboarding_message("Importando as letras para a biblioteca...".into());
            }
        });
    }
    let mut inserted = 0usize;
    read_letras_pack(zip_path, "letras", |batch, done, total| {
        let r = rt.block_on(storage.import_songs_bulk(&batch, true))?;
        inserted += r.inserted;
        set_progress(
            weak,
            hidden,
            0.6 + 0.4 * (done as f32 / total.max(1) as f32),
            format!("{inserted} músicas importadas"),
        );
        Ok(())
    })?;
    Ok(inserted)
}

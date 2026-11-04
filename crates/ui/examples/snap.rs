//! Render headless da tela principal para PPM: `cargo run -p overture-ui --example snap -- W H out.ppm`.
use std::{io::Write, rc::Rc};

use overture_ui::{MainWindow, UiPlaylistRow, UiSlideCard, UiSongRow, UiThemeRow};
use slint::{Image, ModelRc, VecModel};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, PhysicalSize, PlatformError};

struct P(Rc<MinimalSoftwareWindow>);
impl Platform for P {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (w, h): (u32, u32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let win = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(P(win.clone()))).unwrap();
    let ui = MainWindow::new().unwrap();
    if std::env::var("SNAP_DEMO").is_ok() {
        demo(&ui);
    }
    if let Ok(step) = std::env::var("SNAP_ONBOARDING") {
        let step: i32 = step.parse().unwrap();
        ui.set_onboarding_step(step);
        ui.set_onboarding_progress(0.42);
        ui.set_onboarding_message(match step {
            1 => "Importando as letras para a biblioteca...".into(),
            2 => "152.304 letras importadas para a sua biblioteca.".into(),
            3 => "Falha ao baixar/importar as letras: sem conexão com a internet".into(),
            _ => "".into(),
        });
        ui.set_onboarding_detail("48.120 músicas importadas".into());
    }
    win.set_size(PhysicalSize::new(w, h));
    ui.show().unwrap();
    let mut buf = vec![slint::Rgb8Pixel::default(); (w * h) as usize];
    win.draw_if_needed(|r| {
        r.render(&mut buf, w as usize);
    });
    let _ = Rgb565Pixel::default();
    let mut f = std::fs::File::create(&a[3]).unwrap();
    write!(f, "P6\n{w} {h}\n255\n").unwrap();
    for p in &buf {
        f.write_all(&[p.r, p.g, p.b]).unwrap();
    }
}

fn demo(ui: &MainWindow) {
    let song = |id, t: &str, a: &str| UiSongRow {
        song_id: id,
        title: t.into(),
        artist: a.into(),
        meta: "".into(),
        has_forced_break: false,
        duplicate_marked: false,
        archived: false,
    };
    let songs = vec![
        song(1, "Consagração (IBAB)", "Aline Barros"),
        song(2, "Emaús", "Gabriela Rocha"),
        song(3, "Grande é o Senhor", "Adhemar de Campos"),
        song(4, "Ressuscita-me", "Aline Barros"),
        song(5, "Tua Graça me Basta", "Ministério Zoe"),
    ];
    ui.set_songs_list(ModelRc::new(VecModel::from(songs.clone())));
    ui.set_song_history_rows(ModelRc::new(VecModel::from(songs[1..3].to_vec())));
    ui.set_selected_song_id(2);
    ui.set_active_header_title("Emaús".into());
    ui.set_active_header_subtitle("Gabriela Rocha".into());
    ui.set_active_key("Fm".into());
    ui.set_active_bpm("87".into());
    let bodies = [
        "Eu quero caminhar contigo\nSem saber pra onde ir",
        "Ouvir tua voz no caminho\nE a ti me render",
        "Arde o meu coração\nQuando falas comigo",
        "Fica conosco, Senhor\nPois a noite já vem",
        "Emaús, Emaús\nO caminho da fé",
    ];
    let cards: Vec<UiSlideCard> = bodies
        .iter()
        .enumerate()
        .map(|(i, b)| UiSlideCard {
            slide_idx: i as i32,
            number_label: format!("{}", i + 1).into(),
            body: (*b).into(),
            forced_break: i == 3,
        })
        .collect();
    ui.set_active_slides(ModelRc::new(VecModel::from(cards)));
    ui.set_current_slide_index(1);
    let pl = |i, n: &str, a: &str| UiPlaylistRow {
        item_id: i,
        row_idx: i - 1,
        kind_badge: "".into(),
        title: n.into(),
        subtitle: a.into(),
        is_media: false,
    };
    ui.set_playlist_song_rows(ModelRc::new(VecModel::from(vec![
        pl(1, "Consagração (IBAB)", "Aline Barros"),
        pl(2, "Emaús", "Gabriela Rocha"),
        pl(3, "Grande é o Senhor", "Adhemar de Campos"),
    ])));
    let th = |i, n: &str| UiThemeRow {
        theme_id: i,
        name: n.into(),
        is_return: false,
        summary: "".into(),
        thumbnail: Image::default(),
    };
    ui.set_themes_list(ModelRc::new(VecModel::from(vec![th(1, "Grafite"), th(2, "Azul"), th(3, "Claro")])));
}

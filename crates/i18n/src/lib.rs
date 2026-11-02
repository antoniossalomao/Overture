//! Real-time PT-BR and EN localization dictionary and runtime switcher for Overture.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Locale {
    #[default]
    #[serde(rename = "pt-BR")]
    PtBr,
    #[serde(rename = "en")]
    En,
}

impl Locale {
    pub fn from_code(code: &str) -> Self {
        let lower = code.trim().to_ascii_lowercase();
        if lower.starts_with("en") {
            Self::En
        } else {
            Self::PtBr
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::PtBr => "pt-BR",
            Self::En => "en",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::PtBr => "Português (Brasil)",
            Self::En => "English",
        }
    }
}

/// Complete strongly-typed string bundle used by Slint desktop UI and mobile web controller.
#[derive(Debug, Clone, Serialize)]
pub struct UiStrings {
    pub app_title: &'static str,
    // Navigation tabs (7 screens)
    pub nav_control: &'static str,
    pub nav_library: &'static str,
    pub nav_song_editor: &'static str,
    pub nav_themes: &'static str,
    pub nav_monitors: &'static str,
    pub nav_schedule_reports: &'static str,
    pub nav_settings: &'static str,

    // Screen 1: Control
    pub service_playlist: &'static str,
    pub temporary_playlist: &'static str,
    pub new_service: &'static str,
    pub add_announcement: &'static str,
    pub add_countdown: &'static str,
    pub quick_library: &'static str,
    pub search_placeholder: &'static str,
    pub live_output: &'static str,
    pub next_slide_preview: &'static str,
    pub btn_prev: &'static str,
    pub btn_next: &'static str,
    pub btn_black: &'static str,
    pub btn_logo: &'static str,
    pub btn_clear: &'static str,
    pub btn_auto_play: &'static str,
    pub countdown_title: &'static str,
    pub scoreboard_title: &'static str,
    pub stage_alert_placeholder: &'static str,
    pub send_alert: &'static str,
    pub clear_alert: &'static str,

    // Screen 2: Library
    pub tab_songs: &'static str,
    pub tab_bible: &'static str,
    pub tab_media: &'static str,
    pub tab_archived: &'static str,
    pub import_txt_folder: &'static str,
    pub import_xml_or_pptx: &'static str,
    pub add_to_service: &'static str,
    pub project_now: &'static str,
    pub archive_item: &'static str,
    pub restore_item: &'static str,
    pub bible_favorites: &'static str,
    pub bible_history: &'static str,

    // Screen 3: Song Editor
    pub song_title: &'static str,
    pub song_artist: &'static str,
    pub song_author: &'static str,
    pub song_notes: &'static str,
    pub raw_lyrics: &'static str,
    pub auto_split_slides: &'static str,
    pub save_song: &'static str,
    pub forced_break_badge: &'static str,

    // Screen 4: Themes
    pub theme_name: &'static str,
    pub font_family: &'static str,
    pub font_size: &'static str,
    pub text_color: &'static str,
    pub background_kind: &'static str,
    pub shadow_toggle: &'static str,
    pub alignment: &'static str,
    pub save_theme: &'static str,

    // Screen 5: Monitors
    pub main_output_monitor: &'static str,
    pub stage_monitor: &'static str,
    pub return_monitor: &'static str,
    pub return_theme_label: &'static str,
    pub apply_monitors: &'static str,
    pub close_outputs: &'static str,
    pub latency_label: &'static str,

    // Screen 6: Schedule & Reports
    pub team_schedule: &'static str,
    pub member_name: &'static str,
    pub role_name: &'static str,
    pub add_to_schedule: &'static str,
    pub times_scheduled: &'static str,
    pub most_played_songs: &'static str,
    pub filter_weekday: &'static str,

    // Screen 7: Settings
    pub language_label: &'static str,
    pub api_token_label: &'static str,
    pub api_port_label: &'static str,
    pub shortcuts_label: &'static str,
    pub create_backup: &'static str,
    pub restore_backup: &'static str,
    pub export_json: &'static str,
    pub export_txt: &'static str,
    pub save_settings: &'static str,
}

pub const PT_BR: UiStrings = UiStrings {
    app_title: "Overture — Projeção para Cultos",
    nav_control: "Controle",
    nav_library: "Biblioteca",
    nav_song_editor: "Editor de Música",
    nav_themes: "Temas",
    nav_monitors: "Monitores",
    nav_schedule_reports: "Escala e Relatórios",
    nav_settings: "Configurações",

    service_playlist: "Ordem do Culto",
    temporary_playlist: "Playlist Temporária",
    new_service: "Novo Culto",
    add_announcement: "+ Aviso",
    add_countdown: "+ Contagem",
    quick_library: "Acesso Rápido e Slides",
    search_placeholder: "Buscar música, trecho ou referência bíblica (ex: João 3:16)...",
    live_output: "Saída ao Vivo",
    next_slide_preview: "Próximo Slide",
    btn_prev: "◀ Anterior",
    btn_next: "Próximo ▶",
    btn_black: "Tela Preta (B)",
    btn_logo: "Logo (L)",
    btn_clear: "Limpar (Esc)",
    btn_auto_play: "Auto-Avanço",
    countdown_title: "Contagem Regressiva",
    scoreboard_title: "Placar Manual",
    stage_alert_placeholder: "Mensagem de alerta para palco/retorno...",
    send_alert: "Enviar Alerta",
    clear_alert: "Limpar Alerta",

    tab_songs: "Músicas",
    tab_bible: "Bíblia",
    tab_media: "Mídias",
    tab_archived: "Arquivados",
    import_txt_folder: "Importar Pasta TXT",
    import_xml_or_pptx: "Importar XML / PPTX",
    add_to_service: "+ Culto",
    project_now: "Projetar",
    archive_item: "Arquivar",
    restore_item: "Restaurar",
    bible_favorites: "Favoritos",
    bible_history: "Histórico",

    song_title: "Título",
    song_artist: "Artista",
    song_author: "Autor / Compositor",
    song_notes: "Notas de Palco (Tom, BPM, Entrada)",
    raw_lyrics: "Letra Completa (estofes separadas por linha em branco)",
    auto_split_slides: "Dividir Slides Automaticamente",
    save_song: "Salvar Música",
    forced_break_badge: "Revisar quebra forçada",

    theme_name: "Nome do Tema",
    font_family: "Fonte",
    font_size: "Tamanho (pt)",
    text_color: "Cor do Texto (#HEX)",
    background_kind: "Fundo (Cor, Gradiente, Imagem ou Vídeo)",
    shadow_toggle: "Sombra Suave",
    alignment: "Alinhamento",
    save_theme: "Salvar Tema",

    main_output_monitor: "Saída Principal (Projetor)",
    stage_monitor: "Tela de Palco (Slide Atual + Próximo + Cronômetro)",
    return_monitor: "Monitor de Retorno (Tema Próprio)",
    return_theme_label: "Tema do Retorno",
    apply_monitors: "Ativar / Atualizar Saídas",
    close_outputs: "Fechar Janelas de Saída",
    latency_label: "Latência Comando → GPU",

    team_schedule: "Escala de Integrantes",
    member_name: "Integrante",
    role_name: "Função (ex: Vocal, Teclado, Projeção)",
    add_to_schedule: "Escalar no Culto Atual",
    times_scheduled: "Vezes Escalado",
    most_played_songs: "Músicas Mais Tocadas (Período / Dia da Semana)",
    filter_weekday: "Dia da Semana",

    language_label: "Idioma da Interface",
    api_token_label: "Token Fixo da API Local",
    api_port_label: "Porta HTTP/WebSocket",
    shortcuts_label: "Atalhos de Teclado",
    create_backup: "Gerar Backup Completo (.zip)",
    restore_backup: "Restaurar Backup",
    export_json: "Exportar Biblioteca JSON",
    export_txt: "Exportar Músicas TXT",
    save_settings: "Salvar Configurações",
};

pub const EN_US: UiStrings = UiStrings {
    app_title: "Overture — Worship Projection",
    nav_control: "Live Control",
    nav_library: "Library",
    nav_song_editor: "Song Editor",
    nav_themes: "Themes",
    nav_monitors: "Monitors",
    nav_schedule_reports: "Schedule & Reports",
    nav_settings: "Settings",

    service_playlist: "Service Order",
    temporary_playlist: "Temporary Playlist",
    new_service: "New Service",
    add_announcement: "+ Notice",
    add_countdown: "+ Countdown",
    quick_library: "Quick Lookup & Slides",
    search_placeholder: "Search songs, lyrics, or Scripture (e.g. John 3:16)...",
    live_output: "Live Output",
    next_slide_preview: "Next Slide",
    btn_prev: "◀ Previous",
    btn_next: "Next ▶",
    btn_black: "Blackout (B)",
    btn_logo: "Logo (L)",
    btn_clear: "Clear (Esc)",
    btn_auto_play: "Auto-Play",
    countdown_title: "Countdown Timer",
    scoreboard_title: "Manual Scoreboard",
    stage_alert_placeholder: "Stage / return monitor alert message...",
    send_alert: "Send Alert",
    clear_alert: "Clear Alert",

    tab_songs: "Songs",
    tab_bible: "Bible",
    tab_media: "Media",
    tab_archived: "Archived",
    import_txt_folder: "Import TXT Folder",
    import_xml_or_pptx: "Import XML / PPTX",
    add_to_service: "+ Service",
    project_now: "Go Live",
    archive_item: "Archive",
    restore_item: "Restore",
    bible_favorites: "Favorites",
    bible_history: "History",

    song_title: "Title",
    song_artist: "Artist",
    song_author: "Author / Composer",
    song_notes: "Stage Notes (Key, BPM, Cues)",
    raw_lyrics: "Full Lyrics (blank line separates stanzas)",
    auto_split_slides: "Auto-Split Slides",
    save_song: "Save Song",
    forced_break_badge: "Review forced break",

    theme_name: "Theme Name",
    font_family: "Font Family",
    font_size: "Font Size (pt)",
    text_color: "Text Color (#HEX)",
    background_kind: "Background (Color, Gradient, Image, or Video)",
    shadow_toggle: "Drop Shadow",
    alignment: "Alignment",
    save_theme: "Save Theme",

    main_output_monitor: "Main Output (Projector)",
    stage_monitor: "Stage View (Current + Next + Timer)",
    return_monitor: "Return Monitor (Custom Theme)",
    return_theme_label: "Return Theme",
    apply_monitors: "Activate / Update Outputs",
    close_outputs: "Close Output Windows",
    latency_label: "Command → GPU Latency",

    team_schedule: "Team Schedule",
    member_name: "Member Name",
    role_name: "Role (e.g. Vocals, Keys, Tech)",
    add_to_schedule: "Assign to Current Service",
    times_scheduled: "Times Scheduled",
    most_played_songs: "Most Played Songs (Period / Day of Week)",
    filter_weekday: "Day of Week",

    language_label: "Interface Language",
    api_token_label: "Local API Fixed Token",
    api_port_label: "HTTP/WebSocket Port",
    shortcuts_label: "Keyboard Shortcuts",
    create_backup: "Create Full Backup (.zip)",
    restore_backup: "Restore Backup",
    export_json: "Export JSON Library",
    export_txt: "Export TXT Songs",
    save_settings: "Save Settings",
};

pub fn strings_for(locale: Locale) -> &'static UiStrings {
    match locale {
        Locale::PtBr => &PT_BR,
        Locale::En => &EN_US,
    }
}

#[derive(Clone, Default)]
pub struct I18n {
    locale: Arc<RwLock<Locale>>,
}

impl I18n {
    pub fn new(initial: Locale) -> Self {
        Self {
            locale: Arc::new(RwLock::new(initial)),
        }
    }

    pub fn locale(&self) -> Locale {
        *self.locale.read().expect("i18n lock")
    }

    pub fn set_locale(&self, next: Locale) {
        *self.locale.write().expect("i18n lock") = next;
    }

    pub fn strings(&self) -> &'static UiStrings {
        strings_for(self.locale())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_between_pt_br_and_en_in_real_time() {
        let i18n = I18n::new(Locale::PtBr);
        assert_eq!(i18n.strings().nav_control, "Controle");
        i18n.set_locale(Locale::En);
        assert_eq!(i18n.strings().nav_control, "Live Control");
        assert_eq!(Locale::from_code("pt-BR"), Locale::PtBr);
        assert_eq!(Locale::from_code("en-US"), Locale::En);
    }
}

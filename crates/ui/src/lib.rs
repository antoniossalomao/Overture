//! Native Slint desktop UI components (Holyrics Studio + shadcn/ui design system) for Overture.

slint::include_modules!();

use overture_i18n::Locale;

pub fn ui_strings_for_locale(locale: Locale) -> UiStringsData {
    match locale {
        Locale::PtBr => UiStringsData {
            app_title: "Overture".into(),
            go_to_bible: "Ir para Bíblia".into(),
            search_placeholder: "Pesquisar... (Ctrl+F)".into(),
            temporary_label: "Temporário".into(),
            themes_label: "Temas".into(),
            display_settings: "Configurações de exibição".into(),
            save_theme_for_song: "Salvar Tema para Música".into(),
        },
        Locale::En => UiStringsData {
            app_title: "Overture".into(),
            go_to_bible: "Go to Bible".into(),
            search_placeholder: "Search... (Ctrl+F)".into(),
            temporary_label: "Temporary".into(),
            themes_label: "Themes".into(),
            display_settings: "Display Settings".into(),
            save_theme_for_song: "Save Theme for Song".into(),
        },
    }
}

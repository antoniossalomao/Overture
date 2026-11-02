use anyhow::{Context, Result, anyhow};
use overture_core::{Group, Service, Song, Theme};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Native Overture JSON library bundle for structured export/import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OvertureJsonBundle {
    pub format: String,
    pub exported_at: String,
    pub songs: Vec<Song>,
    pub groups: Vec<Group>,
    pub themes: Vec<Theme>,
    pub services: Vec<Service>,
}

impl OvertureJsonBundle {
    pub const FORMAT_ID: &'static str = "overture-json-v1";

    pub fn new(
        songs: Vec<Song>,
        groups: Vec<Group>,
        themes: Vec<Theme>,
        services: Vec<Service>,
    ) -> Self {
        Self {
            format: Self::FORMAT_ID.to_string(),
            exported_at: chrono::Utc::now().to_rfc3339(),
            songs,
            groups,
            themes,
            services,
        }
    }
}

pub fn export_song_to_txt_string(song: &Song) -> String {
    let mut out = String::new();
    out.push_str(song.title.trim());
    out.push('\n');
    let credit = if song.source.trim().is_empty() {
        "Fonte: Overture".to_string()
    } else {
        format!("Fonte: {}", song.source.trim())
    };
    out.push_str(&credit);
    out.push_str("\n\n");

    let slide_blocks: Vec<&str> = song.slides.iter().map(|s| s.text.as_str()).collect();
    out.push_str(&slide_blocks.join("\n\n"));
    out.push('\n');
    out
}

pub fn export_songs_to_txt_directory(songs: &[Song], out_dir: &Path) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("Failed to create TXT export dir {}", out_dir.display()))?;
    let mut written = Vec::with_capacity(songs.len());

    for song in songs {
        let base_stem = if song.artist.trim().is_empty() {
            sanitize_filename(&song.title)
        } else {
            format!(
                "{} - {}",
                sanitize_filename(&song.artist),
                sanitize_filename(&song.title)
            )
        };
        let path = out_dir.join(format!("{base_stem}.txt"));
        fs::write(&path, export_song_to_txt_string(song))?;
        written.push(path);
    }
    Ok(written)
}

pub fn export_json_bundle_to_file(bundle: &OvertureJsonBundle, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(bundle)?;
    fs::write(path, json).with_context(|| format!("Failed to write JSON {}", path.display()))?;
    Ok(())
}

pub fn import_json_bundle_from_file(path: &Path) -> Result<OvertureJsonBundle> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read JSON {}", path.display()))?;
    let bundle: OvertureJsonBundle = serde_json::from_str(&raw)?;
    if bundle.format != OvertureJsonBundle::FORMAT_ID {
        return Err(anyhow!("Unsupported Overture JSON format: {}", bundle.format));
    }
    Ok(bundle)
}

fn sanitize_filename(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect();
    s = s.trim().to_string();
    if s.is_empty() {
        "Sem_Titulo".to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::txt::{DEFAULT_MAX_LINE_CHARS, parse_txt_file};
    use overture_core::SongSlide;

    #[test]
    fn txt_and_json_export_roundtrip() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut song = Song::new_draft("Castelo Forte", "Martinho Lutero", "public_domain");
        song.slides = vec![
            SongSlide {
                id: 1,
                song_id: 1,
                order: 0,
                text: "Castelo forte é nosso Deus,\nEspada e bom escudo;".to_string(),
                forced_break: false,
                duration_secs: None,
            },
            SongSlide {
                id: 2,
                song_id: 1,
                order: 1,
                text: "Com seu poder defende os seus\nEm todo transe agudo.".to_string(),
                forced_break: false,
                duration_secs: None,
            },
        ];

        // TXT export -> re-import
        let paths = export_songs_to_txt_directory(std::slice::from_ref(&song), temp.path())?;
        assert_eq!(paths.len(), 1);
        let reparsed = parse_txt_file(&paths[0], "reimported", DEFAULT_MAX_LINE_CHARS)?;
        assert_eq!(reparsed.song.title, "Castelo Forte");
        assert_eq!(reparsed.song.artist, "Martinho Lutero");
        assert_eq!(reparsed.song.slides.len(), 2);

        // JSON export -> re-import
        let json_path = temp.path().join("library.overture.json");
        let bundle = OvertureJsonBundle::new(vec![song], vec![], vec![], vec![]);
        export_json_bundle_to_file(&bundle, &json_path)?;
        let loaded = import_json_bundle_from_file(&json_path)?;
        assert_eq!(loaded.songs.len(), 1);
        assert_eq!(loaded.songs[0].title, "Castelo Forte");

        Ok(())
    }
}

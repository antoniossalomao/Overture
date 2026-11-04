//! Organiza o corpus `letras-txt.zip` (github.com/damarals/letras) no pacote do Overture.
//!
//! Uso: `cargo run --release -p overture-import --example build_letras_pack -- <letras-txt.zip> <saida.zip>`
//!
//! - remove duplicatas exatas (mesma letra) e de título+artista (mantém a letra mais longa);
//! - agrupa em pastas por inicial do artista (`A/`, `B/`, … `0-9/`, `#/`);
//! - grava `manifest.json` com contagens e a origem.

use overture_import::txt::{parse_txt_bytes, DEFAULT_MAX_LINE_CHARS};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

fn bucket(artist: &str) -> String {
    match artist.chars().find(|c| c.is_alphanumeric()) {
        Some(c) if c.is_ascii_digit() => "0-9".into(),
        Some(c) => {
            let base = c.to_lowercase().next().unwrap_or(c);
            let base = match base {
                'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
                'é' | 'è' | 'ê' => 'e',
                'í' | 'ì' | 'î' => 'i',
                'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'ç' => 'c',
                x => x,
            };
            if base.is_ascii_alphabetic() { base.to_ascii_uppercase().to_string() } else { "#".into() }
        }
        None => "#".into(),
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(args.len() == 3, "uso: build_letras_pack <letras-txt.zip> <saida.zip>");
    let mut input = zip::ZipArchive::new(std::fs::File::open(&args[1])?)?;
    let total = input.len();

    // chave título+artista -> (nome do arquivo, bytes, tamanho da letra)
    let mut best: HashMap<String, (String, Vec<u8>, usize)> = HashMap::new();
    let mut by_lyrics: HashMap<String, String> = HashMap::new(); // fingerprint -> chave
    let (mut dup_exact, mut dup_ta, mut skipped) = (0usize, 0usize, 0usize);

    for i in 0..total {
        let mut f = input.by_index(i)?;
        let name = Path::new(f.name()).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if f.is_dir() || !name.to_lowercase().ends_with(".txt") {
            skipped += 1;
            continue;
        }
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes)?;
        let p = Path::new(&name);
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let (artist, title) = match stem.split_once(" - ") {
            Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
            None => (String::new(), stem.trim().to_string()),
        };
        let parsed = parse_txt_bytes(&bytes, &name, &artist, &title, "letras", DEFAULT_MAX_LINE_CHARS);
        if parsed.song.slides.is_empty() {
            skipped += 1;
            continue;
        }
        let key = format!("{}|{}", norm(&parsed.song.artist), norm(&parsed.song.title));
        if !parsed.exact_fingerprint.is_empty() {
            match by_lyrics.get(&parsed.exact_fingerprint) {
                Some(k) if *k != key || best.contains_key(&key) => {
                    dup_exact += 1;
                    continue;
                }
                _ => {
                    by_lyrics.insert(parsed.exact_fingerprint.clone(), key.clone());
                }
            }
        }
        let size = parsed.validation.lyric_char_count;
        match best.get(&key) {
            Some((_, _, s)) if *s >= size => dup_ta += 1,
            Some(_) => {
                dup_ta += 1;
                best.insert(key, (name, bytes, size));
            }
            None => {
                best.insert(key, (name, bytes, size));
            }
        }
    }

    let mut entries: Vec<(String, Vec<u8>)> = best
        .into_values()
        .map(|(name, bytes, _)| {
            let stem = Path::new(&name).file_stem().unwrap().to_string_lossy().to_string();
            let artist = stem.split_once(" - ").map(|(a, _)| a.to_string()).unwrap_or_default();
            (format!("{}/{}", bucket(&artist), name.replace('/', "-")), bytes)
        })
        .collect();
    entries.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    let mut out = zip::ZipWriter::new(std::fs::File::create(&args[2])?);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(9));
    for (name, bytes) in &entries {
        out.start_file(name, opts)?;
        out.write_all(bytes)?;
    }
    let manifest = serde_json::json!({
        "pack": "overture-letras",
        "source": "https://github.com/damarals/letras",
        "license": "MIT (coletânea); direitos das letras pertencem aos autores",
        "songs": entries.len(),
        "scanned": total,
        "duplicates_exact": dup_exact,
        "duplicates_title_artist": dup_ta,
        "skipped": skipped,
    });
    out.start_file("manifest.json", opts)?;
    out.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    out.finish()?;
    println!("{manifest:#}");
    Ok(())
}

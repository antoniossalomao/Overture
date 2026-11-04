//! Leitura do pacote de letras (`overture-letras.zip`, gerado por `build_letras_pack`).

use crate::txt::{DEFAULT_MAX_LINE_CHARS, parse_txt_bytes};
use anyhow::{Context, Result};
use overture_core::Song;
use std::io::Read;
use std::path::Path;

/// Quantas músicas vão em cada lote entregue a `on_batch`.
pub const PACK_BATCH_SIZE: usize = 2_000;

/// Lê o zip em lotes de [`PACK_BATCH_SIZE`] músicas. `on_batch(lote, lidos, total)` pode
/// gravar o lote no banco; um erro devolvido por ele interrompe a leitura.
/// Retorna o total de músicas lidas.
pub fn read_letras_pack(
    path: &Path,
    source_tag: &str,
    mut on_batch: impl FnMut(Vec<Song>, usize, usize) -> Result<()>,
) -> Result<usize> {
    let file = std::fs::File::open(path).with_context(|| format!("abrir {}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("pacote de letras inválido (zip)")?;
    let total = zip.len();
    let mut batch: Vec<Song> = Vec::with_capacity(PACK_BATCH_SIZE);
    let mut read = 0usize;

    for i in 0..total {
        let mut entry = zip.by_index(i)?;
        let name = Path::new(entry.name())
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if entry.is_dir() || !name.to_lowercase().ends_with(".txt") {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        let stem = Path::new(&name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let (artist, title) = match stem.split_once(" - ") {
            Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
            None => (String::new(), stem.trim().to_string()),
        };
        let parsed = parse_txt_bytes(&bytes, &name, &artist, &title, source_tag, DEFAULT_MAX_LINE_CHARS);
        if parsed.song.slides.is_empty() {
            continue;
        }
        batch.push(parsed.song);
        read += 1;
        if batch.len() >= PACK_BATCH_SIZE {
            on_batch(std::mem::take(&mut batch), i + 1, total)?;
        }
    }
    if !batch.is_empty() {
        on_batch(batch, total, total)?;
    }
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn le_pacote_em_lotes_e_ignora_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            w.start_file("A/Aline Barros - Consagração.txt", o).unwrap();
            w.write_all("Consagração\n\nTudo o que sou\nEu entrego a Ti\n".as_bytes()).unwrap();
            w.start_file("manifest.json", o).unwrap();
            w.write_all(b"{}").unwrap();
            w.finish().unwrap();
        }
        let mut songs = Vec::new();
        let n = read_letras_pack(&path, "letras", |b, _, _| {
            songs.extend(b);
            Ok(())
        })
        .unwrap();
        assert_eq!(n, 1);
        assert_eq!(songs[0].artist, "Aline Barros");
        assert_eq!(songs[0].title, "Consagração");
        assert!(!songs[0].slides.is_empty());
    }
}

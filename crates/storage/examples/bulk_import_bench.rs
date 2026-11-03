//! Mede a importação em lote com uma biblioteca sintética grande (padrão: 170 mil músicas).
//!
//! Uso: cargo run --release -p overture-storage --example bulk_import_bench -- [quantidade]

use overture_core::{Song, SongSlide};
use overture_storage::Storage;
use std::time::Instant;

fn song(i: usize, total: usize) -> Song {
    // ~5% das músicas repetem a letra de outra (duplicata exata); artistas em ~1/30 do total.
    let letra_id = if i % 20 == 0 && i > 0 { i - 1 } else { i };
    let artist = format!("Artista {:05}", (i * 7919) % (total / 30).max(1));
    let title = format!("Música {:06}", i);
    let slides = (0..3)
        .map(|k| SongSlide {
            id: 0,
            song_id: 0,
            order: k,
            text: format!("Linha {} da letra {}\nSegunda linha {}", k, letra_id, letra_id % 997),
            forced_break: false,
            duration_secs: None,
        })
        .collect();
    Song {
        id: 0,
        title,
        artist,
        author: String::new(),
        group_id: None,
        notes: String::new(),
        source: "bench".into(),
        archived: false,
        duplicate_marked: false,
        created_at: String::new(),
        slides,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let total: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(170_000);
    let db = std::env::temp_dir().join(format!("overture-bench-{total}.db"));
    let _ = std::fs::remove_file(&db);
    let storage = Storage::connect_file(&db).await?;

    let songs: Vec<Song> = (0..total).map(|i| song(i, total)).collect();
    let t0 = Instant::now();
    let report = storage.import_songs_bulk(&songs, true).await?;
    let elapsed = t0.elapsed();
    println!("importadas: {} | duplicatas ignoradas: {} | marcadas título+artista: {} | grupos: {}",
        report.inserted, report.skipped_duplicates, report.marked_title_artist, report.groups_total);
    println!("tempo: {:.1} s ({:.0} músicas/s)", elapsed.as_secs_f64(), total as f64 / elapsed.as_secs_f64());

    let t1 = Instant::now();
    let hits = storage.search_songs("Linha 1", false, None, 200).await?;
    println!("busca 'Linha 1': {} resultados em {} ms", hits.len(), t1.elapsed().as_millis());
    Ok(())
}

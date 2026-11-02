use anyhow::{Context, Result};
use encoding_rs::WINDOWS_1252;
use overture_core::{Song, SongSlide};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Default maximum character width per projected line before splitting into 2 lines.
pub const DEFAULT_MAX_LINE_CHARS: usize = 52;
/// Maximum lines per slide mandated by Section 4.
pub const MAX_LINES_PER_SLIDE: usize = 2;
/// Minimum lyric character threshold below which `short_lyrics` warning is flagged.
pub const MIN_STANDARD_LYRIC_CHARS: usize = 250;

/// Words (articles, prepositions, contractions, clitics, short conjunctions/possessives in PT-BR & EN)
/// that MUST NEVER be stranded at the end of a line separated from the word/noun they accompany.
const ATTACHED_TO_NEXT_WORDS: &[&str] = &[
    // Portuguese articles & contractions
    "o", "a", "os", "as", "um", "uma", "uns", "umas",
    "de", "da", "do", "das", "dos", "dum", "duma", "duns", "dumas",
    "em", "na", "no", "nas", "nos", "num", "numa", "nuns", "numas",
    "por", "pelo", "pela", "pelos", "pelas",
    "para", "pra", "pro", "pras", "pros",
    "ao", "aos", "à", "às",
    // Portuguese prepositions
    "com", "sem", "sob", "sobre", "ante", "ate", "até", "apos", "após",
    "contra", "entre", "perante", "desde", "rumo",
    // Clitics, negation, conjunctions & determiners that bind forward
    "que", "se", "me", "te", "lhe", "lhes", "vos", "nao", "não", "nem",
    "e", "ou", "mas", "meu", "minha", "meus", "minhas", "teu", "tua", "teus", "tuas",
    "seu", "sua", "seus", "suas", "nosso", "nossa", "nossos", "nossas",
    "este", "esta", "esse", "essa", "aquele", "aquela", "todo", "toda", "todos", "todas",
    // English articles & prepositions
    "the", "an", "of", "in", "on", "at", "to", "for", "with", "by", "from", "into", "unto",
    "my", "your", "his", "her", "our", "their", "and", "or",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SongValidationReport {
    pub file_name: String,
    pub title: String,
    pub artist: String,
    pub encoding_detected: String,
    pub total_slides: usize,
    pub forced_break_slides: usize,
    pub lines_over_limit: usize,
    pub lyric_char_count: usize,
    pub short_lyrics: bool,
    pub exact_duplicate_removed: bool,
    pub duplicate_title_artist_marked: bool,
}

#[derive(Debug, Clone)]
pub struct ParsedTxtSong {
    pub song: Song,
    pub validation: SongValidationReport,
    pub exact_fingerprint: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchTxtImportReport {
    pub total_files_scanned: usize,
    pub imported_count: usize,
    pub exact_duplicates_removed: usize,
    pub title_artist_duplicates_marked: usize,
    pub forced_break_songs: usize,
    pub short_lyric_songs: usize,
    pub song_reports: Vec<SongValidationReport>,
}

/// Decodes raw bytes as UTF-8 (stripping UTF-8 BOM if present) or falls back to ISO-8859-1 / Windows-1252.
pub fn decode_txt_bytes(bytes: &[u8]) -> (String, &'static str) {
    let stripped = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if let Ok(utf8) = std::str::from_utf8(stripped) {
        (utf8.to_string(), "UTF-8")
    } else {
        let (cow, _) = WINDOWS_1252.decode_without_bom_handling(bytes);
        (cow.into_owned(), "ISO-8859-1")
    }
}

/// Extracts `(artist, title)` from a file stem following `Artista - Título.txt` (or `Título.txt`).
pub fn parse_filename_artist_title(path: &Path) -> (String, String) {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    if let Some((left, right)) = stem.split_once(" - ") {
        (left.trim().to_string(), right.trim().to_string())
    } else {
        (String::new(), stem.trim().to_string())
    }
}

/// Parses a single `.txt` file from disk applying Section 4 header stripping, encoding detection,
/// stanza preservation, smart 2-line slide division, and validation metrics.
pub fn parse_txt_file(
    path: &Path,
    source_tag: &str,
    max_line_chars: usize,
) -> Result<ParsedTxtSong> {
    let bytes =
        fs::read(path).with_context(|| format!("Failed to read TXT file {}", path.display()))?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    let (fn_artist, fn_title) = parse_filename_artist_title(path);
    Ok(parse_txt_bytes(
        &bytes,
        &file_name,
        &fn_artist,
        &fn_title,
        source_tag,
        max_line_chars,
    ))
}

/// Parses raw TXT bytes with explicit filename metadata.
pub fn parse_txt_bytes(
    bytes: &[u8],
    file_name: &str,
    filename_artist: &str,
    filename_title: &str,
    source_tag: &str,
    max_line_chars: usize,
) -> ParsedTxtSong {
    let (decoded, encoding_detected) = decode_txt_bytes(bytes);
    let normalized = decoded.replace("\r\n", "\n").replace('\r', "\n");

    let (title, lyrics_body) = extract_header_and_lyrics(&normalized, filename_title);
    let slides = split_lyrics_into_slides(&lyrics_body, max_line_chars);

    let mut song = Song::new_draft(title.clone(), filename_artist.trim(), source_tag);
    song.slides = slides;

    let forced_break_slides = song.slides.iter().filter(|s| s.forced_break).count();
    let lines_over_limit = song
        .slides
        .iter()
        .flat_map(|s| s.text.lines())
        .filter(|line| line.chars().count() > max_line_chars)
        .count();
    let lyric_char_count: usize = song
        .slides
        .iter()
        .map(|s| s.text.chars().filter(|c| !c.is_whitespace()).count()
            + s.text.split_whitespace().count().saturating_sub(1))
        .sum();
    let short_lyrics = lyric_char_count < MIN_STANDARD_LYRIC_CHARS;

    let exact_fingerprint = compute_lyrics_fingerprint(&song.slides);

    let validation = SongValidationReport {
        file_name: file_name.to_string(),
        title,
        artist: song.artist.clone(),
        encoding_detected: encoding_detected.to_string(),
        total_slides: song.slides.len(),
        forced_break_slides,
        lines_over_limit,
        lyric_char_count,
        short_lyrics,
        exact_duplicate_removed: false,
        duplicate_title_artist_marked: false,
    };

    ParsedTxtSong {
        song,
        validation,
        exact_fingerprint,
    }
}

/// Imports all `.txt` files in a directory, removing exact duplicates and marking `title + artist` duplicates.
pub fn parse_txt_directory(
    dir: &Path,
    source_tag: &str,
    max_line_chars: usize,
) -> Result<(Vec<Song>, BatchTxtImportReport)> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory {}", dir.display()))?
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .map(|ext| ext.to_string_lossy().eq_ignore_ascii_case("txt"))
                    .unwrap_or(false)
        })
        .collect();
    paths.sort();

    let mut seen_fingerprints: HashSet<String> = HashSet::new();
    let mut seen_title_artist: HashSet<(String, String)> = HashSet::new();
    let mut accepted_songs: Vec<Song> = Vec::new();
    let mut report = BatchTxtImportReport {
        total_files_scanned: paths.len(),
        ..Default::default()
    };

    for path in paths {
        let mut parsed = parse_txt_file(&path, source_tag, max_line_chars)?;

        if !parsed.exact_fingerprint.is_empty()
            && !seen_fingerprints.insert(parsed.exact_fingerprint.clone())
        {
            parsed.validation.exact_duplicate_removed = true;
            report.exact_duplicates_removed += 1;
            report.song_reports.push(parsed.validation);
            continue;
        }

        let ta_key = (
            parsed.song.title.to_lowercase(),
            parsed.song.artist.to_lowercase(),
        );
        if !seen_title_artist.insert(ta_key) {
            parsed.song.duplicate_marked = true;
            parsed.validation.duplicate_title_artist_marked = true;
            report.title_artist_duplicates_marked += 1;
        }

        if parsed.validation.forced_break_slides > 0 {
            report.forced_break_songs += 1;
        }
        if parsed.validation.short_lyrics {
            report.short_lyric_songs += 1;
        }

        report.imported_count += 1;
        report.song_reports.push(parsed.validation);
        accepted_songs.push(parsed.song);
    }

    Ok((accepted_songs, report))
}

/// Removes the standard 2-line site header (`line 1 = title`, `line 2 = site credit`, `line 3 = blank`)
/// and falls back to `filename_title` whenever line 1 is missing or non-standardized.
fn extract_header_and_lyrics(normalized_text: &str, filename_title: &str) -> (String, String) {
    let trimmed = normalized_text.trim_matches('\n');
    let lines: Vec<&str> = trimmed.lines().collect();

    // Check for leading header block followed by a blank line:
    // Standard format: line 0 = Title, line 1 = Site credit, line 2 = empty, line 3.. = lyrics
    if lines.len() >= 4 && lines[2].trim().is_empty() && !lines[0].trim().is_empty() {
        let l1 = lines[0].trim();
        let l2 = lines[1].trim();
        if looks_like_site_credit(l2) || titles_roughly_match(l1, filename_title) {
            let final_title = if is_standardized_title(l1, filename_title) {
                l1.to_string()
            } else if !filename_title.trim().is_empty() {
                filename_title.trim().to_string()
            } else {
                l1.to_string()
            };
            let rest = lines[3..].join("\n");
            return (final_title, rest.trim().to_string());
        }
    }

    // Also handle 1-line title header followed by a blank line when line 1 matches filename_title
    if lines.len() >= 3
        && lines[1].trim().is_empty()
        && !lines[0].trim().is_empty()
        && titles_roughly_match(lines[0].trim(), filename_title)
    {
        let l1 = lines[0].trim();
        let final_title = if is_standardized_title(l1, filename_title) {
            l1.to_string()
        } else {
            filename_title.trim().to_string()
        };
        let rest = lines[2..].join("\n");
        return (final_title, rest.trim().to_string());
    }

    let fallback_title = if !filename_title.trim().is_empty() {
        filename_title.trim().to_string()
    } else {
        lines
            .first()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Sem Título".to_string())
    };
    (fallback_title, trimmed.trim().to_string())
}

fn looks_like_site_credit(line: &str) -> bool {
    let lower = line.to_lowercase();
    let markers = [
        "http://",
        "https://",
        "www.",
        ".com",
        ".br",
        ".org",
        ".net",
        "letras",
        "vagalume",
        "cifraclub",
        "supercanticos",
        "harpa crist",
        "cantor crist",
        "hinário",
        "hinario",
        "fonte:",
        "crédito:",
        "credito:",
        "extraído de",
        "extraido de",
        "enviado por",
        "domínio público",
        "dominio publico",
        "public domain",
    ];
    markers.iter().any(|m| lower.contains(m))
}

fn is_standardized_title(line1: &str, filename_title: &str) -> bool {
    if line1.is_empty() || line1.chars().count() > 75 {
        return false;
    }
    let lower = line1.to_lowercase();
    if lower.contains("http")
        || lower.contains("www.")
        || lower.contains(".com")
        || line1.starts_with('[')
        || line1.contains(" - ")
        || line1.ends_with(',')
        || line1.ends_with(';')
    {
        return false;
    }
    // Leading track number like "01. Castelo Forte" or "01 - " is unstandardized if filename_title is clean
    if line1
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
        && !filename_title.trim().is_empty()
        && !filename_title
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
    {
        return false;
    }
    // ALL CAPS line1 when filename_title has proper mixed case is unstandardized
    let has_alpha = line1.chars().any(|c| c.is_alphabetic());
    let is_all_caps = has_alpha
        && line1
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(|c| c.is_uppercase());
    let fn_has_lower = filename_title.chars().any(|c| c.is_lowercase());
    if is_all_caps && fn_has_lower {
        return false;
    }
    true
}

fn titles_roughly_match(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let fa: String = a
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect();
    let fb: String = b
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect();
    !fa.is_empty() && (fa == fb || fa.contains(&fb) || fb.contains(&fa))
}

#[derive(Debug, Clone)]
struct ShapedLine {
    text: String,
    forced_break: bool,
}

/// Splits raw lyrics into slides according to Section 4 rules:
/// - Maximum 2 lines per slide.
/// - Stanzas separated by blank lines are NEVER mixed.
/// - Preserves semantic meaning: prefers punctuation breaks (`,` `;` `.` `!` `?`).
/// - NEVER separates an article or preposition (`de`, `da`, `no`, `pra`, etc.) from the noun it accompanies.
/// - When no natural break exists on an overlong line, splits at the valid word space closest to the midpoint
///   and marks the slide `forced_break = true` (`quebra_forcada`).
pub fn split_lyrics_into_slides(lyrics_body: &str, max_line_chars: usize) -> Vec<SongSlide> {
    let max_chars = max_line_chars.max(20);
    let normalized = lyrics_body.replace("\r\n", "\n").replace('\r', "\n");

    // Group non-empty lines into stanzas separated by >= 1 blank line
    let mut stanzas: Vec<Vec<String>> = Vec::new();
    let mut current_stanza: Vec<String> = Vec::new();

    for raw_line in normalized.lines() {
        let cleaned = collapse_inline_spaces(raw_line.trim());
        // Skip section tags like [Refrão], [Coro], [Verso 1] if standalone
        if is_standalone_section_marker(&cleaned) {
            if !current_stanza.is_empty() {
                stanzas.push(std::mem::take(&mut current_stanza));
            }
            continue;
        }
        if cleaned.is_empty() {
            if !current_stanza.is_empty() {
                stanzas.push(std::mem::take(&mut current_stanza));
            }
        } else {
            current_stanza.push(cleaned);
        }
    }
    if !current_stanza.is_empty() {
        stanzas.push(current_stanza);
    }

    let mut slides: Vec<SongSlide> = Vec::new();
    for stanza in stanzas {
        let mut shaped_lines: Vec<ShapedLine> = Vec::new();
        for line in stanza {
            split_single_line_semantically(&line, max_chars, &mut shaped_lines);
        }

        group_stanza_lines_into_slides(&shaped_lines, &mut slides);
    }

    for (idx, s) in slides.iter_mut().enumerate() {
        s.order = idx as i32;
    }
    slides
}

fn is_standalone_section_marker(line: &str) -> bool {
    if line.starts_with('[') && line.ends_with(']') && line.len() <= 32 {
        return true;
    }
    false
}

fn collapse_inline_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn split_single_line_semantically(line: &str, max_chars: usize, out: &mut Vec<ShapedLine>) {
    let char_len = line.chars().count();
    if char_len <= max_chars {
        out.push(ShapedLine {
            text: line.to_string(),
            forced_break: false,
        });
        return;
    }

    // Try finding a natural punctuation break (`,` `;` `.` `!` `?` `:` `—`) that keeps the left side
    // within `max_chars` (or balanced near the middle if still recursive) and leaves a non-trivial right side.
    if let Some((left, right)) = find_best_punctuation_split(line, max_chars) {
        split_single_line_semantically(left, max_chars, out);
        split_single_line_semantically(right, max_chars, out);
        return;
    }

    // No natural punctuation break: split at the valid space closest to the character midpoint,
    // strictly avoiding stranding articles/prepositions at the end of `left`.
    if let Some((left, right)) = find_midpoint_safe_word_split(line, max_chars) {
        let left_len = left.chars().count();
        let right_len = right.chars().count();
        if left_len <= max_chars && right_len <= max_chars {
            out.push(ShapedLine {
                text: left.to_string(),
                forced_break: true,
            });
            out.push(ShapedLine {
                text: right.to_string(),
                forced_break: true,
            });
        } else {
            let start_idx = out.len();
            split_single_line_semantically(left, max_chars, out);
            split_single_line_semantically(right, max_chars, out);
            for item in &mut out[start_idx..] {
                item.forced_break = true;
            }
        }
    } else {
        // Unsplittable single giant token
        out.push(ShapedLine {
            text: line.to_string(),
            forced_break: true,
        });
    }
}

fn find_best_punctuation_split(line: &str, max_chars: usize) -> Option<(&str, &str)> {
    let total_chars = line.chars().count();
    let mid = total_chars / 2;
    let mut best: Option<(usize, usize)> = None; // (byte_pos_after_punct, distance_from_mid)

    for (byte_idx, ch) in line.char_indices() {
        if matches!(ch, ',' | ';' | '.' | '!' | '?' | ':' | '—' | '–') {
            let after_punct = byte_idx + ch.len_utf8();
            let left = line[..after_punct].trim();
            let right = line[after_punct..].trim();
            let left_chars = left.chars().count();
            let right_chars = right.chars().count();

            if left_chars < 6 || right_chars < 6 {
                continue;
            }
            if left_chars > max_chars {
                continue;
            }
            let dist = left_chars.abs_diff(mid);
            if best.map(|(_, b_dist)| dist < b_dist).unwrap_or(true) {
                best = Some((after_punct, dist));
            }
        }
    }

    let (split_byte, _) = best?;
    Some((line[..split_byte].trim(), line[split_byte..].trim()))
}

fn find_midpoint_safe_word_split(line: &str, max_chars: usize) -> Option<(&str, &str)> {
    let total_chars = line.chars().count();
    let mid = total_chars / 2;

    // Collect all whitespace boundaries `(byte_idx, left_char_count, is_safe_preposition_boundary)`
    let mut candidates: Vec<(usize, usize, bool)> = Vec::new();
    let mut char_pos = 0usize;

    for (byte_idx, ch) in line.char_indices() {
        if ch == ' ' {
            let left = line[..byte_idx].trim();
            let right = line[byte_idx + 1..].trim();
            if !left.is_empty() && !right.is_empty() {
                let last_word = left.split_whitespace().next_back().unwrap_or("");
                let safe = !is_attached_to_next_word(last_word);
                candidates.push((byte_idx, char_pos, safe));
            }
        }
        char_pos += 1;
    }

    if candidates.is_empty() {
        return None;
    }

    // 1. Prefer safe boundaries where BOTH halves fit <= max_chars, closest to `mid`
    if let Some(&(b_idx, _, _)) = candidates
        .iter()
        .filter(|&&(b_idx, l_chars, safe)| {
            safe && l_chars <= max_chars && line[b_idx + 1..].trim().chars().count() <= max_chars
        })
        .min_by_key(|&&(_, l_chars, _)| l_chars.abs_diff(mid))
    {
        return Some((line[..b_idx].trim(), line[b_idx + 1..].trim()));
    }

    // 2. Otherwise any safe boundary where left fits <= max_chars, closest to `mid`
    if let Some(&(b_idx, _, _)) = candidates
        .iter()
        .filter(|&&(_, l_chars, safe)| safe && l_chars <= max_chars)
        .min_by_key(|&&(_, l_chars, _)| l_chars.abs_diff(mid))
    {
        return Some((line[..b_idx].trim(), line[b_idx + 1..].trim()));
    }

    // 3. Otherwise any safe boundary at all, closest to `mid`
    if let Some(&(b_idx, _, _)) = candidates
        .iter()
        .filter(|&&(_, _, safe)| safe)
        .min_by_key(|&&(_, l_chars, _)| l_chars.abs_diff(mid))
    {
        return Some((line[..b_idx].trim(), line[b_idx + 1..].trim()));
    }

    // 4. Extreme fallback if literally every word is a preposition/article
    let &(b_idx, _, _) = candidates
        .iter()
        .min_by_key(|&&(_, l_chars, _)| l_chars.abs_diff(mid))?;
    Some((line[..b_idx].trim(), line[b_idx + 1..].trim()))
}

pub fn is_attached_to_next_word(word: &str) -> bool {
    let cleaned = word
        .trim_matches(|c: char| !c.is_alphabetic() && c != '\'' && c != '-')
        .to_lowercase();
    if cleaned.is_empty() {
        return false;
    }
    ATTACHED_TO_NEXT_WORDS.contains(&cleaned.as_str())
}

fn ends_with_terminal_sentence_punct(line: &str) -> bool {
    line.trim_end()
        .chars()
        .next_back()
        .map(|c| matches!(c, '.' | '!' | '?' | ';'))
        .unwrap_or(false)
}

fn group_stanza_lines_into_slides(lines: &[ShapedLine], out: &mut Vec<SongSlide>) {
    if lines.is_empty() {
        return;
    }
    // Special semantic case for 3-line stanza:
    // If line 0 ends with `.`/`!`/`?`/`;` and line 1 does NOT end with terminal punctuation,
    // keep line 0 on its own slide and pair `[line 1, line 2]` together so the second sentence stays intact!
    if lines.len() == 3
        && ends_with_terminal_sentence_punct(&lines[0].text)
        && !ends_with_terminal_sentence_punct(&lines[1].text)
    {
        out.push(SongSlide {
            id: 0,
            song_id: 0,
            order: 0,
            text: lines[0].text.clone(),
            forced_break: lines[0].forced_break,
            duration_secs: None,
        });
        out.push(SongSlide {
            id: 0,
            song_id: 0,
            order: 0,
            text: format!("{}\n{}", lines[1].text, lines[2].text),
            forced_break: lines[1].forced_break || lines[2].forced_break,
            duration_secs: None,
        });
        return;
    }

    let mut i = 0;
    while i < lines.len() {
        if i + 1 < lines.len() {
            out.push(SongSlide {
                id: 0,
                song_id: 0,
                order: 0,
                text: format!("{}\n{}", lines[i].text, lines[i + 1].text),
                forced_break: lines[i].forced_break || lines[i + 1].forced_break,
                duration_secs: None,
            });
            i += MAX_LINES_PER_SLIDE;
        } else {
            out.push(SongSlide {
                id: 0,
                song_id: 0,
                order: 0,
                text: lines[i].text.clone(),
                forced_break: lines[i].forced_break,
                duration_secs: None,
            });
            i += 1;
        }
    }
}

fn compute_lyrics_fingerprint(slides: &[SongSlide]) -> String {
    let mut canonical = String::new();
    for s in slides {
        for ch in s.text.chars() {
            if ch.is_alphanumeric() {
                for lc in ch.to_lowercase() {
                    canonical.push(lc);
                }
            }
        }
    }
    if canonical.is_empty() {
        return String::new();
    }
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_separates_portuguese_prepositions_or_articles() {
        // No punctuation, > 52 chars, midpoint sits right around "pra" / "no" / "da"
        let line = "Cantarei louvores eternos de todo coração pra sempre no templo da glória";
        let slides = split_lyrics_into_slides(line, 44);
        assert!(!slides.is_empty());
        for s in &slides {
            for l in s.text.lines() {
                let last_word = l.split_whitespace().next_back().unwrap_or("");
                assert!(
                    !is_attached_to_next_word(last_word),
                    "line ended with attached preposition/article `{last_word}` in `{l}`"
                );
            }
            assert!(s.forced_break, "expected forced_break marker on unpunctuated split");
        }
    }

    #[test]
    fn decodes_iso_8859_1_and_strips_site_header_with_fallback_title() {
        // Line 1 is ALL CAPS unstandardized, Line 2 is site credit, Line 3 blank, then ISO-8859-1 lyrics
        let text = "CASTELO FORTE\nhttps://www.letras.mus.br/lutero/123/\n\nCastelo forte é nosso Deus,\nEspada e bom escudo;\n\nCom seu poder defende os seus\nEm todo transe agudo.";
        let (encoded, _, _) = WINDOWS_1252.encode(text);

        let parsed = parse_txt_bytes(
            &encoded,
            "Martinho Lutero - Castelo Forte.txt",
            "Martinho Lutero",
            "Castelo Forte",
            "user_txt",
            52,
        );

        assert_eq!(parsed.validation.encoding_detected, "ISO-8859-1");
        assert_eq!(parsed.song.artist, "Martinho Lutero");
        assert_eq!(parsed.song.title, "Castelo Forte"); // Used standardized filename title!
        assert_eq!(parsed.song.slides.len(), 2);
        assert_eq!(
            parsed.song.slides[0].text,
            "Castelo forte é nosso Deus,\nEspada e bom escudo;"
        );
        assert!(parsed.validation.short_lyrics); // < 250 chars flagged accurately
    }

    #[test]
    fn never_mixes_stanzas_separated_by_blank_line() {
        let lyrics = "Glória a Deus nas alturas!\n\nE paz na terra aos homens\nA quem Ele quer bem.";
        let slides = split_lyrics_into_slides(lyrics, 52);
        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0].text, "Glória a Deus nas alturas!");
        assert_eq!(
            slides[1].text,
            "E paz na terra aos homens\nA quem Ele quer bem."
        );
    }
}

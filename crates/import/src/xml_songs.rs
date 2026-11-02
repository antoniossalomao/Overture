use crate::txt::{DEFAULT_MAX_LINE_CHARS, split_lyrics_into_slides};
use anyhow::{Context, Result, anyhow};
use overture_core::Song;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::fs;
use std::path::Path;

/// Parses an OpenSong XML file (supports `[V1]`/`[C]` sections, `.` chord lines, and `' '` lyric lines).
pub fn parse_opensong_file(path: &Path, max_line_chars: usize) -> Result<Song> {
    let xml = fs::read_to_string(path)
        .with_context(|| format!("Failed to read OpenSong file {}", path.display()))?;
    parse_opensong_xml(&xml, "opensong_xml", max_line_chars)
}

pub fn parse_opensong_xml(xml: &str, source_tag: &str, max_line_chars: usize) -> Result<Song> {
    let mut reader = Reader::from_str(xml);
    let mut current_tag = String::new();
    let mut title = String::new();
    let mut author = String::new();
    let mut raw_lyrics = String::new();
    let mut notes = String::new();

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                current_tag = String::from_utf8_lossy(e.local_name().as_ref()).to_lowercase();
            }
            Ok(Event::End(_)) => {
                current_tag.clear();
            }
            Ok(Event::Text(t)) => {
                let text = t.unescape().unwrap_or_default();
                match current_tag.as_str() {
                    "title" => title.push_str(&text),
                    "author" => author.push_str(&text),
                    "lyrics" => raw_lyrics.push_str(&text),
                    "aka" | "user1" => notes.push_str(&text),
                    _ => {}
                }
            }
            Ok(Event::CData(c)) => {
                let text = String::from_utf8_lossy(&c);
                if current_tag == "lyrics" {
                    raw_lyrics.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("Invalid OpenSong XML: {e}")),
            _ => {}
        }
        buf.clear();
    }

    if title.trim().is_empty() && raw_lyrics.trim().is_empty() {
        return Err(anyhow!("OpenSong XML contained neither <title> nor <lyrics>"));
    }

    // Clean OpenSong chord lines (`.`) and leading lyric space (`' '`), underscores (`_`)
    let mut cleaned_stanzas = String::new();
    for raw_line in raw_lyrics.replace("\r\n", "\n").lines() {
        if raw_line.starts_with('.') {
            // Chord line — skip
            continue;
        }
        if raw_line.starts_with(';') {
            // Comment line — skip
            continue;
        }
        let trimmed_start = raw_line.trim_start();
        if trimmed_start.starts_with('[') && trimmed_start.ends_with(']') {
            cleaned_stanzas.push('\n');
            continue;
        }
        let lyric_line = raw_line
            .strip_prefix(' ')
            .unwrap_or(raw_line)
            .replace('_', "")
            .replace('|', "\n");
        cleaned_stanzas.push_str(lyric_line.trim());
        cleaned_stanzas.push('\n');
    }

    let final_title = if title.trim().is_empty() {
        "Sem Título".to_string()
    } else {
        title.trim().to_string()
    };

    let mut song = Song::new_draft(final_title, author.trim(), source_tag);
    song.author = author.trim().to_string();
    song.notes = notes.trim().to_string();
    song.slides = split_lyrics_into_slides(
        &cleaned_stanzas,
        if max_line_chars == 0 {
            DEFAULT_MAX_LINE_CHARS
        } else {
            max_line_chars
        },
    );
    Ok(song)
}

/// Parses an OpenLP / OpenLyrics XML song (`http://openlyrics.info/namespace/2009/song`).
pub fn parse_openlp_file(path: &Path, max_line_chars: usize) -> Result<Song> {
    let xml = fs::read_to_string(path)
        .with_context(|| format!("Failed to read OpenLP XML file {}", path.display()))?;
    parse_openlp_xml(&xml, "openlp_xml", max_line_chars)
}

pub fn parse_openlp_xml(xml: &str, source_tag: &str, max_line_chars: usize) -> Result<Song> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<String> = Vec::new();

    let mut titles: Vec<String> = Vec::new();
    let mut authors: Vec<String> = Vec::new();
    let mut comments: Vec<String> = Vec::new();
    let mut verses: Vec<String> = Vec::new();

    let mut cur_text = String::new();
    let mut cur_verse_buf = String::new();
    let mut in_verse = false;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_lowercase();
                if tag == "verse" {
                    in_verse = true;
                    cur_verse_buf.clear();
                } else if tag == "br" && in_verse {
                    cur_verse_buf.push('\n');
                } else if matches!(tag.as_str(), "title" | "author" | "comment") {
                    cur_text.clear();
                }
                stack.push(tag);
            }
            Ok(Event::Empty(e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_lowercase();
                if tag == "br" && in_verse {
                    cur_verse_buf.push('\n');
                }
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_lowercase();
                match tag.as_str() {
                    "title" => {
                        if !cur_text.trim().is_empty() {
                            titles.push(cur_text.trim().to_string());
                        }
                        cur_text.clear();
                    }
                    "author" => {
                        if !cur_text.trim().is_empty() {
                            authors.push(cur_text.trim().to_string());
                        }
                        cur_text.clear();
                    }
                    "comment" => {
                        if !cur_text.trim().is_empty() {
                            comments.push(cur_text.trim().to_string());
                        }
                        cur_text.clear();
                    }
                    "lines" => {
                        if in_verse && !cur_verse_buf.ends_with('\n') {
                            cur_verse_buf.push('\n');
                        }
                    }
                    "verse" => {
                        in_verse = false;
                        if !cur_verse_buf.trim().is_empty() {
                            verses.push(cur_verse_buf.trim().to_string());
                        }
                        cur_verse_buf.clear();
                    }
                    _ => {}
                }
                stack.pop();
            }
            Ok(Event::Text(t)) => {
                let txt = t.unescape().unwrap_or_default();
                if in_verse {
                    cur_verse_buf.push_str(&txt);
                } else if let Some(top) = stack.last()
                    && matches!(top.as_str(), "title" | "author" | "comment")
                {
                    cur_text.push_str(&txt);
                }
            }
            Ok(Event::CData(c)) => {
                let txt = String::from_utf8_lossy(&c);
                if in_verse {
                    cur_verse_buf.push_str(&txt);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("Invalid OpenLP/OpenLyrics XML: {e}")),
            _ => {}
        }
        buf.clear();
    }

    if titles.is_empty() && verses.is_empty() {
        return Err(anyhow!("OpenLyrics XML contained no titles or verses"));
    }

    let final_title = titles
        .into_iter()
        .next()
        .unwrap_or_else(|| "Sem Título".to_string());
    let joined_authors = authors.join(", ");
    let joined_lyrics = verses.join("\n\n");

    let mut song = Song::new_draft(final_title, joined_authors.clone(), source_tag);
    song.author = joined_authors;
    song.notes = comments.join("\n");
    song.slides = split_lyrics_into_slides(
        &joined_lyrics,
        if max_line_chars == 0 {
            DEFAULT_MAX_LINE_CHARS
        } else {
            max_line_chars
        },
    );
    Ok(song)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_opensong_xml_stripping_chords() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<song>
  <title>Castelo Forte</title>
  <author>Martinho Lutero</author>
  <lyrics>[V1]
.C        G      Am
 Castelo_ forte é nosso Deus,
.F         G       C
 Espada e bom escudo;
[C]
.Am       F
 Com seu poder defende os seus
.G               C
 Em todo transe agudo.</lyrics>
</song>"#;

        let song = parse_opensong_xml(xml, "opensong", 52).expect("valid OpenSong XML");
        assert_eq!(song.title, "Castelo Forte");
        assert_eq!(song.artist, "Martinho Lutero");
        assert_eq!(song.slides.len(), 2);
        assert_eq!(
            song.slides[0].text,
            "Castelo forte é nosso Deus,\nEspada e bom escudo;"
        );
    }

    #[test]
    fn parses_openlp_openlyrics_xml() {
        let xml = r#"<?xml version='1.0' encoding='UTF-8'?>
<song xmlns="http://openlyrics.info/namespace/2009/song" version="0.8">
  <properties>
    <titles><title>Amazing Grace</title></titles>
    <authors><author>John Newton</author></authors>
    <comments><comment>Public Domain Hymn</comment></comments>
  </properties>
  <lyrics>
    <verse name="v1">
      <lines>Amazing grace! How sweet the sound<br/>That saved a wretch like me!</lines>
    </verse>
    <verse name="v2">
      <lines>I once was lost, but now am found;<br/>Was blind, but now I see.</lines>
    </verse>
  </lyrics>
</song>"#;

        let song = parse_openlp_xml(xml, "openlp", 52).expect("valid OpenLyrics XML");
        assert_eq!(song.title, "Amazing Grace");
        assert_eq!(song.author, "John Newton");
        assert_eq!(song.notes, "Public Domain Hymn");
        assert_eq!(song.slides.len(), 2);
        assert_eq!(
            song.slides[0].text,
            "Amazing grace! How sweet the sound\nThat saved a wretch like me!"
        );
    }
}

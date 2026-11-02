use anyhow::{Context, Result, anyhow};
use overture_core::{ServiceItem, ServiceItemKind, ServiceItemParams, Song, SongSlide};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;
use zip::ZipArchive;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPptxPresentation {
    pub title: String,
    pub slide_texts: Vec<String>,
}

impl ParsedPptxPresentation {
    pub fn into_service_item(self, service_id: i64, order: i32) -> ServiceItem {
        let count = self.slide_texts.len();
        ServiceItem {
            id: 0,
            service_id,
            order,
            kind: ServiceItemKind::Presentation,
            ref_id: None,
            title: self.title,
            subtitle: format!("PowerPoint ({count} slides)"),
            params: ServiceItemParams {
                custom_slides: self.slide_texts,
                ..Default::default()
            },
        }
    }

    pub fn into_song(self, artist: &str) -> Song {
        let mut song = Song::new_draft(self.title, artist, "pptx_import");
        song.slides = self
            .slide_texts
            .into_iter()
            .filter(|t| !t.trim().is_empty())
            .enumerate()
            .map(|(idx, text)| SongSlide {
                id: 0,
                song_id: 0,
                order: idx as i32,
                text,
                forced_break: false,
                duration_secs: None,
            })
            .collect();
        song
    }
}

pub fn parse_pptx_file(path: &Path) -> Result<ParsedPptxPresentation> {
    let default_title = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Apresentação PowerPoint".to_string());

    let mut f =
        File::open(path).with_context(|| format!("Failed to open PPTX {}", path.display()))?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes)?;
    parse_pptx_bytes(&bytes, &default_title)
}

pub fn parse_pptx_bytes(bytes: &[u8], default_title: &str) -> Result<ParsedPptxPresentation> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).context("Invalid .pptx ZIP container")?;

    // Try reading explicit slide order from ppt/presentation.xml + ppt/_rels/presentation.xml.rels
    let ordered_paths = resolve_ordered_slide_paths(&mut archive);

    let slide_entry_names: Vec<String> = if !ordered_paths.is_empty() {
        ordered_paths
    } else {
        let mut fallback: Vec<(u32, String)> = Vec::new();
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i) {
                let name = entry.name().replace('\\', "/");
                if let Some(rest) = name.strip_prefix("ppt/slides/slide")
                    && let Some(num_str) = rest.strip_suffix(".xml")
                    && let Ok(num) = num_str.parse::<u32>()
                {
                    fallback.push((num, name));
                }
            }
        }
        fallback.sort_by_key(|(num, _)| *num);
        fallback.into_iter().map(|(_, name)| name).collect()
    };

    if slide_entry_names.is_empty() {
        return Err(anyhow!("No slides found inside .pptx archive"));
    }

    let mut slide_texts = Vec::with_capacity(slide_entry_names.len());
    for entry_name in slide_entry_names {
        if let Ok(mut entry) = archive.by_name(&entry_name) {
            let mut xml = String::new();
            entry.read_to_string(&mut xml)?;
            let extracted = extract_drawingml_slide_text(&xml)?;
            if !extracted.trim().is_empty() {
                slide_texts.push(extracted);
            } else {
                slide_texts.push(String::new());
            }
        }
    }

    Ok(ParsedPptxPresentation {
        title: default_title.trim().to_string(),
        slide_texts,
    })
}

fn resolve_ordered_slide_paths(archive: &mut ZipArchive<Cursor<&[u8]>>) -> Vec<String> {
    let rels_xml = match archive.by_name("ppt/_rels/presentation.xml.rels") {
        Ok(mut f) => {
            let mut s = String::new();
            let _ = f.read_to_string(&mut s);
            s
        }
        Err(_) => return Vec::new(),
    };
    let pres_xml = match archive.by_name("ppt/presentation.xml") {
        Ok(mut f) => {
            let mut s = String::new();
            let _ = f.read_to_string(&mut s);
            s
        }
        Err(_) => return Vec::new(),
    };

    let mut rel_map: HashMap<String, String> = HashMap::new();
    let mut reader = Reader::from_str(&rels_xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                if e.local_name().as_ref().eq_ignore_ascii_case(b"Relationship") {
                    let mut id = String::new();
                    let mut target = String::new();
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref().eq_ignore_ascii_case(b"Id") {
                            id = attr.unescape_value().unwrap_or_default().to_string();
                        } else if attr.key.as_ref().eq_ignore_ascii_case(b"Target") {
                            target = attr.unescape_value().unwrap_or_default().to_string();
                        }
                    }
                    if !id.is_empty() && target.contains("slides/slide") {
                        let normalized = if target.starts_with("ppt/") {
                            target
                        } else {
                            format!("ppt/{}", target.trim_start_matches('/'))
                        };
                        rel_map.insert(id, normalized);
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    let mut ordered = Vec::new();
    let mut p_reader = Reader::from_str(&pres_xml);
    buf.clear();
    loop {
        match p_reader.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                if e.local_name().as_ref().eq_ignore_ascii_case(b"sldId") {
                    for attr in e.attributes().flatten() {
                        let k = attr.key.as_ref();
                        if k.ends_with(b":id") || k.eq_ignore_ascii_case(b"r:id") {
                            let rid = attr.unescape_value().unwrap_or_default().to_string();
                            if let Some(target) = rel_map.get(&rid) {
                                ordered.push(target.clone());
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    ordered
}

fn extract_drawingml_slide_text(xml: &str) -> Result<String> {
    let mut reader = Reader::from_str(xml);
    let mut paragraphs: Vec<String> = Vec::new();
    let mut cur_para = String::new();
    let mut in_text_run = false;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = e.local_name();
                if local.as_ref() == b"p" {
                    cur_para.clear();
                } else if local.as_ref() == b"t" {
                    in_text_run = true;
                } else if local.as_ref() == b"br" {
                    cur_para.push('\n');
                }
            }
            Ok(Event::Empty(e)) => {
                if e.local_name().as_ref() == b"br" {
                    cur_para.push('\n');
                }
            }
            Ok(Event::End(e)) => {
                let local = e.local_name();
                if local.as_ref() == b"t" {
                    in_text_run = false;
                } else if local.as_ref() == b"p" {
                    let trimmed = cur_para.trim();
                    if !trimmed.is_empty() {
                        paragraphs.push(trimmed.to_string());
                    }
                    cur_para.clear();
                }
            }
            Ok(Event::Text(t)) if in_text_run => {
                cur_para.push_str(&t.unescape().unwrap_or_default());
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("Invalid PPTX slide XML: {e}")),
            _ => {}
        }
        buf.clear();
    }

    Ok(paragraphs.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::CompressionMethod;
    use zip::write::{SimpleFileOptions, ZipWriter};

    #[test]
    fn imports_pptx_ooxml_slides_in_presentation_order() -> Result<()> {
        let mut raw_zip = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut raw_zip));
            let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

            zw.start_file("ppt/_rels/presentation.xml.rels", opts)?;
            zw.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
                <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
                  <Relationship Id="rId2" Target="slides/slide2.xml"/>
                  <Relationship Id="rId1" Target="slides/slide1.xml"/>
                </Relationships>"#,
            )?;

            // Deliberately order slide2 BEFORE slide1 in presentation.xml to verify relationship ordering!
            zw.start_file("ppt/presentation.xml", opts)?;
            zw.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
                <p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                                xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
                  <p:sldIdLst>
                    <p:sldId id="256" r:id="rId2"/>
                    <p:sldId id="257" r:id="rId1"/>
                  </p:sldIdLst>
                </p:presentation>"#,
            )?;

            zw.start_file("ppt/slides/slide1.xml", opts)?;
            zw.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
                <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                       xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                  <a:p><a:r><a:t>Segundo Slide na Ordem</a:t></a:r></a:p>
                </p:sld>"#,
            )?;

            zw.start_file("ppt/slides/slide2.xml", opts)?;
            zw.write_all(
                r#"<?xml version="1.0" encoding="UTF-8"?>
                <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                       xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                  <a:p><a:r><a:t>Primeiro Slide</a:t></a:r></a:p>
                  <a:p><a:r><a:t>Subtítulo de Abertura</a:t></a:r></a:p>
                </p:sld>"#
                    .as_bytes(),
            )?;

            zw.finish()?;
        }

        let parsed = parse_pptx_bytes(&raw_zip, "Mensagem de Domingo")?;
        assert_eq!(parsed.title, "Mensagem de Domingo");
        assert_eq!(parsed.slide_texts.len(), 2);
        assert_eq!(parsed.slide_texts[0], "Primeiro Slide\nSubtítulo de Abertura");
        assert_eq!(parsed.slide_texts[1], "Segundo Slide na Ordem");

        let svc_item = parsed.into_service_item(1, 0);
        assert_eq!(svc_item.kind, ServiceItemKind::Presentation);
        assert_eq!(svc_item.params.custom_slides.len(), 2);
        Ok(())
    }
}

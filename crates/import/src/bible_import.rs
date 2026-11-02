use anyhow::{Context, Result, anyhow};
use overture_core::{BibleVerse, BibleVersion};
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonBibleDocument {
    pub abbr: String,
    pub name: String,
    pub lang: String,
    pub license: String,
    pub verses: Vec<BibleVerse>,
}

/// Parses a Zefania XML Bible (`<XMLBIBLE biblename="...">...`) requiring an explicit `license` label.
pub fn parse_zefania_bible_xml(
    xml: &str,
    abbr: &str,
    lang: &str,
    license: &str,
) -> Result<(BibleVersion, Vec<BibleVerse>)> {
    if license.trim().is_empty() {
        return Err(anyhow!(
            "A license declaration is required when importing a Bible version"
        ));
    }

    let mut reader = Reader::from_str(xml);
    let mut bible_name = abbr.trim().to_string();
    let mut cur_book = String::new();
    let mut cur_book_num: i32 = 1;
    let mut cur_chapter: i32 = 1;
    let mut cur_verse: i32 = 1;
    let mut in_vers = false;
    let mut vers_buf = String::new();
    let mut verses: Vec<BibleVerse> = Vec::new();

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_uppercase();
                match tag.as_str() {
                    "XMLBIBLE" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref().eq_ignore_ascii_case(b"biblename") {
                                let v = attr.unescape_value().unwrap_or_default();
                                if !v.trim().is_empty() {
                                    bible_name = v.trim().to_string();
                                }
                            }
                        }
                    }
                    "BIBLEBOOK" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref().eq_ignore_ascii_case(b"bname") {
                                cur_book = attr.unescape_value().unwrap_or_default().to_string();
                            } else if attr.key.as_ref().eq_ignore_ascii_case(b"bnumber") {
                                cur_book_num = attr
                                    .unescape_value()
                                    .unwrap_or_default()
                                    .parse()
                                    .unwrap_or(cur_book_num);
                            }
                        }
                    }
                    "CHAPTER" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref().eq_ignore_ascii_case(b"cnumber") {
                                cur_chapter = attr
                                    .unescape_value()
                                    .unwrap_or_default()
                                    .parse()
                                    .unwrap_or(1);
                            }
                        }
                    }
                    "VERS" => {
                        in_vers = true;
                        vers_buf.clear();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref().eq_ignore_ascii_case(b"vnumber") {
                                cur_verse = attr
                                    .unescape_value()
                                    .unwrap_or_default()
                                    .parse()
                                    .unwrap_or(1);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.local_name().as_ref()).to_uppercase();
                if tag == "VERS" {
                    in_vers = false;
                    let text = vers_buf.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !text.is_empty() && !cur_book.is_empty() {
                        verses.push(BibleVerse {
                            version_id: 0,
                            book: cur_book.clone(),
                            book_order: cur_book_num,
                            chapter: cur_chapter,
                            verse: cur_verse,
                            text,
                        });
                    }
                    vers_buf.clear();
                }
            }
            Ok(Event::Text(t)) if in_vers => {
                vers_buf.push_str(&t.unescape().unwrap_or_default());
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("Invalid Zefania Bible XML: {e}")),
            _ => {}
        }
        buf.clear();
    }

    if verses.is_empty() {
        return Err(anyhow!("Zefania XML contained no <VERS> entries"));
    }

    let version = BibleVersion {
        id: 0,
        abbr: abbr.trim().to_uppercase(),
        name: bible_name,
        lang: lang.trim().to_string(),
        license: license.trim().to_string(),
    };
    Ok((version, verses))
}

pub fn parse_json_bible_file(path: &Path) -> Result<(BibleVersion, Vec<BibleVerse>)> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read JSON Bible {}", path.display()))?;
    let doc: JsonBibleDocument = serde_json::from_str(&raw)?;
    if doc.license.trim().is_empty() {
        return Err(anyhow!("JSON Bible must specify a non-empty license"));
    }
    let ver = BibleVersion {
        id: 0,
        abbr: doc.abbr,
        name: doc.name,
        lang: doc.lang,
        license: doc.license,
    };
    Ok((ver, doc.verses))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_zefania_bible_and_enforces_license() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<XMLBIBLE biblename="Bíblia Livre">
  <BIBLEBOOK bnumber="43" bname="João">
    <CHAPTER cnumber="1">
      <VERS vnumber="1">No princípio era o Verbo, e o Verbo estava com Deus.</VERS>
      <VERS vnumber="14">E o Verbo se fez carne e habitou entre nós.</VERS>
    </CHAPTER>
  </BIBLEBOOK>
</XMLBIBLE>"#;

        assert!(parse_zefania_bible_xml(xml, "BLIVRE", "pt-BR", "").is_err());

        let (ver, verses) =
            parse_zefania_bible_xml(xml, "BLIVRE", "pt-BR", "CC-BY-SA-4.0").expect("valid zefania");
        assert_eq!(ver.abbr, "BLIVRE");
        assert_eq!(ver.name, "Bíblia Livre");
        assert_eq!(ver.license, "CC-BY-SA-4.0");
        assert_eq!(verses.len(), 2);
        assert_eq!(verses[1].verse, 14);
    }
}

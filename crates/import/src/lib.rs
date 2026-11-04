//! Lyrics and presentation importers/exporters for Overture.

pub mod bible_import;
pub mod export;
pub mod pack;
pub mod pptx;
pub mod txt;
pub mod xml_songs;

pub use bible_import::{JsonBibleDocument, parse_json_bible_file, parse_zefania_bible_xml};
pub use export::{
    OvertureJsonBundle, export_json_bundle_to_file, export_song_to_txt_string,
    export_songs_to_txt_directory, import_json_bundle_from_file,
};
pub use pack::{PACK_BATCH_SIZE, read_letras_pack};
pub use pptx::{ParsedPptxPresentation, parse_pptx_bytes, parse_pptx_file};

pub use txt::{
    BatchTxtImportReport, DEFAULT_MAX_LINE_CHARS, MAX_LINES_PER_SLIDE, MIN_STANDARD_LYRIC_CHARS,
    ParsedTxtSong, SongValidationReport, decode_txt_bytes, is_attached_to_next_word,
    parse_filename_artist_title, parse_txt_bytes, parse_txt_directory, parse_txt_file,
    split_lyrics_into_slides,
};
pub use xml_songs::{
    parse_openlp_file, parse_openlp_xml, parse_opensong_file, parse_opensong_xml,
};

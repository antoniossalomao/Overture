use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use overture_core::{BackgroundSpec, BibleVerse, BibleVersion, RgbaColor, Theme, ThemeConfig};
use serde::Deserialize;
use std::io::Read;

const TB1917_GZ: &[u8] = include_bytes!("../assets/tb1917.json.gz");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalBookInfo {
    pub order: i32,
    pub abbr_pt: &'static str,
    pub name_pt: &'static str,
    pub abbr_en: &'static str,
    pub name_en: &'static str,
    /// Holyrics canonical color group:
    /// 0 = Pentateuch, 1 = Historical, 2 = Poetic, 3 = Prophets,
    /// 4 = Gospels, 5 = Acts, 6 = Epistles, 7 = Revelation
    pub category: u8,
    pub chapters: i32,
}

pub const CANONICAL_BIBLE_BOOKS: [CanonicalBookInfo; 66] = [
    // Row 1 (1..11)
    CanonicalBookInfo { order: 1, abbr_pt: "Gn", name_pt: "Gênesis", abbr_en: "Gen", name_en: "Genesis", category: 0, chapters: 50 },
    CanonicalBookInfo { order: 2, abbr_pt: "Ex", name_pt: "Êxodo", abbr_en: "Exo", name_en: "Exodus", category: 0, chapters: 40 },
    CanonicalBookInfo { order: 3, abbr_pt: "Lv", name_pt: "Levítico", abbr_en: "Lev", name_en: "Leviticus", category: 0, chapters: 27 },
    CanonicalBookInfo { order: 4, abbr_pt: "Nm", name_pt: "Números", abbr_en: "Num", name_en: "Numbers", category: 0, chapters: 36 },
    CanonicalBookInfo { order: 5, abbr_pt: "Dt", name_pt: "Deuteronômio", abbr_en: "Deu", name_en: "Deuteronomy", category: 0, chapters: 34 },
    CanonicalBookInfo { order: 6, abbr_pt: "Js", name_pt: "Josué", abbr_en: "Jos", name_en: "Joshua", category: 1, chapters: 24 },
    CanonicalBookInfo { order: 7, abbr_pt: "Jz", name_pt: "Juízes", abbr_en: "Jdg", name_en: "Judges", category: 1, chapters: 21 },
    CanonicalBookInfo { order: 8, abbr_pt: "Rt", name_pt: "Rute", abbr_en: "Rut", name_en: "Ruth", category: 1, chapters: 4 },
    CanonicalBookInfo { order: 9, abbr_pt: "1Sm", name_pt: "1 Samuel", abbr_en: "1Sa", name_en: "1 Samuel", category: 1, chapters: 31 },
    CanonicalBookInfo { order: 10, abbr_pt: "2Sm", name_pt: "2 Samuel", abbr_en: "2Sa", name_en: "2 Samuel", category: 1, chapters: 24 },
    CanonicalBookInfo { order: 11, abbr_pt: "1Rs", name_pt: "1 Reis", abbr_en: "1Ki", name_en: "1 Kings", category: 1, chapters: 22 },
    // Row 2 (12..22)
    CanonicalBookInfo { order: 12, abbr_pt: "2Rs", name_pt: "2 Reis", abbr_en: "2Ki", name_en: "2 Kings", category: 1, chapters: 25 },
    CanonicalBookInfo { order: 13, abbr_pt: "1Cr", name_pt: "1 Crônicas", abbr_en: "1Ch", name_en: "1 Chronicles", category: 1, chapters: 29 },
    CanonicalBookInfo { order: 14, abbr_pt: "2Cr", name_pt: "2 Crônicas", abbr_en: "2Ch", name_en: "2 Chronicles", category: 1, chapters: 36 },
    CanonicalBookInfo { order: 15, abbr_pt: "Ed", name_pt: "Esdras", abbr_en: "Ezr", name_en: "Ezra", category: 1, chapters: 10 },
    CanonicalBookInfo { order: 16, abbr_pt: "Ne", name_pt: "Neemias", abbr_en: "Neh", name_en: "Nehemiah", category: 1, chapters: 13 },
    CanonicalBookInfo { order: 17, abbr_pt: "Et", name_pt: "Ester", abbr_en: "Est", name_en: "Esther", category: 1, chapters: 10 },
    CanonicalBookInfo { order: 18, abbr_pt: "Jó", name_pt: "Jó", abbr_en: "Job", name_en: "Job", category: 2, chapters: 42 },
    CanonicalBookInfo { order: 19, abbr_pt: "Sl", name_pt: "Salmos", abbr_en: "Psa", name_en: "Psalms", category: 2, chapters: 150 },
    CanonicalBookInfo { order: 20, abbr_pt: "Pv", name_pt: "Provérbios", abbr_en: "Pro", name_en: "Proverbs", category: 2, chapters: 31 },
    CanonicalBookInfo { order: 21, abbr_pt: "Ec", name_pt: "Eclesiastes", abbr_en: "Ecc", name_en: "Ecclesiastes", category: 2, chapters: 12 },
    CanonicalBookInfo { order: 22, abbr_pt: "Ct", name_pt: "Cantares", abbr_en: "Sng", name_en: "Song of Solomon", category: 2, chapters: 8 },
    // Row 3 (23..33)
    CanonicalBookInfo { order: 23, abbr_pt: "Is", name_pt: "Isaías", abbr_en: "Isa", name_en: "Isaiah", category: 3, chapters: 66 },
    CanonicalBookInfo { order: 24, abbr_pt: "Jr", name_pt: "Jeremias", abbr_en: "Jer", name_en: "Jeremiah", category: 3, chapters: 52 },
    CanonicalBookInfo { order: 25, abbr_pt: "Lm", name_pt: "Lamentações", abbr_en: "Lam", name_en: "Lamentations", category: 3, chapters: 5 },
    CanonicalBookInfo { order: 26, abbr_pt: "Ez", name_pt: "Ezequiel", abbr_en: "Ezk", name_en: "Ezekiel", category: 3, chapters: 48 },
    CanonicalBookInfo { order: 27, abbr_pt: "Dn", name_pt: "Daniel", abbr_en: "Dan", name_en: "Daniel", category: 3, chapters: 12 },
    CanonicalBookInfo { order: 28, abbr_pt: "Os", name_pt: "Oseias", abbr_en: "Hos", name_en: "Hosea", category: 3, chapters: 14 },
    CanonicalBookInfo { order: 29, abbr_pt: "Jl", name_pt: "Joel", abbr_en: "Jol", name_en: "Joel", category: 3, chapters: 3 },
    CanonicalBookInfo { order: 30, abbr_pt: "Am", name_pt: "Amós", abbr_en: "Amo", name_en: "Amos", category: 3, chapters: 9 },
    CanonicalBookInfo { order: 31, abbr_pt: "Ob", name_pt: "Obadias", abbr_en: "Oba", name_en: "Obadiah", category: 3, chapters: 1 },
    CanonicalBookInfo { order: 32, abbr_pt: "Jn", name_pt: "Jonas", abbr_en: "Jon", name_en: "Jonah", category: 3, chapters: 4 },
    CanonicalBookInfo { order: 33, abbr_pt: "Mq", name_pt: "Miqueias", abbr_en: "Mic", name_en: "Micah", category: 3, chapters: 7 },
    // Row 4 (34..44)
    CanonicalBookInfo { order: 34, abbr_pt: "Na", name_pt: "Naum", abbr_en: "Nam", name_en: "Nahum", category: 3, chapters: 3 },
    CanonicalBookInfo { order: 35, abbr_pt: "Hc", name_pt: "Habacuque", abbr_en: "Hab", name_en: "Habakkuk", category: 3, chapters: 3 },
    CanonicalBookInfo { order: 36, abbr_pt: "Sf", name_pt: "Sofonias", abbr_en: "Zep", name_en: "Zephaniah", category: 3, chapters: 3 },
    CanonicalBookInfo { order: 37, abbr_pt: "Ag", name_pt: "Ageu", abbr_en: "Hag", name_en: "Haggai", category: 3, chapters: 2 },
    CanonicalBookInfo { order: 38, abbr_pt: "Zc", name_pt: "Zacarias", abbr_en: "Zec", name_en: "Zechariah", category: 3, chapters: 14 },
    CanonicalBookInfo { order: 39, abbr_pt: "Ml", name_pt: "Malaquias", abbr_en: "Mal", name_en: "Malachi", category: 3, chapters: 4 },
    CanonicalBookInfo { order: 40, abbr_pt: "Mt", name_pt: "Mateus", abbr_en: "Mat", name_en: "Matthew", category: 4, chapters: 28 },
    CanonicalBookInfo { order: 41, abbr_pt: "Mc", name_pt: "Marcos", abbr_en: "Mrk", name_en: "Mark", category: 4, chapters: 16 },
    CanonicalBookInfo { order: 42, abbr_pt: "Lc", name_pt: "Lucas", abbr_en: "Luk", name_en: "Luke", category: 4, chapters: 24 },
    CanonicalBookInfo { order: 43, abbr_pt: "Jo", name_pt: "João", abbr_en: "Jhn", name_en: "John", category: 4, chapters: 21 },
    CanonicalBookInfo { order: 44, abbr_pt: "At", name_pt: "Atos", abbr_en: "Act", name_en: "Acts", category: 5, chapters: 28 },
    // Row 5 (45..55)
    CanonicalBookInfo { order: 45, abbr_pt: "Rm", name_pt: "Romanos", abbr_en: "Rom", name_en: "Romans", category: 6, chapters: 16 },
    CanonicalBookInfo { order: 46, abbr_pt: "1Co", name_pt: "1 Coríntios", abbr_en: "1Co", name_en: "1 Corinthians", category: 6, chapters: 16 },
    CanonicalBookInfo { order: 47, abbr_pt: "2Co", name_pt: "2 Coríntios", abbr_en: "2Co", name_en: "2 Corinthians", category: 6, chapters: 13 },
    CanonicalBookInfo { order: 48, abbr_pt: "Gl", name_pt: "Gálatas", abbr_en: "Gal", name_en: "Galatians", category: 6, chapters: 6 },
    CanonicalBookInfo { order: 49, abbr_pt: "Ef", name_pt: "Efésios", abbr_en: "Eph", name_en: "Ephesians", category: 6, chapters: 6 },
    CanonicalBookInfo { order: 50, abbr_pt: "Fp", name_pt: "Filipenses", abbr_en: "Php", name_en: "Philippians", category: 6, chapters: 4 },
    CanonicalBookInfo { order: 51, abbr_pt: "Cl", name_pt: "Colossenses", abbr_en: "Col", name_en: "Colossians", category: 6, chapters: 4 },
    CanonicalBookInfo { order: 52, abbr_pt: "1Ts", name_pt: "1 Tessalonicenses", abbr_en: "1Th", name_en: "1 Thessalonians", category: 6, chapters: 5 },
    CanonicalBookInfo { order: 53, abbr_pt: "2Ts", name_pt: "2 Tessalonicenses", abbr_en: "2Th", name_en: "2 Thessalonians", category: 6, chapters: 3 },
    CanonicalBookInfo { order: 54, abbr_pt: "1Tm", name_pt: "1 Timóteo", abbr_en: "1Ti", name_en: "1 Timothy", category: 6, chapters: 6 },
    CanonicalBookInfo { order: 55, abbr_pt: "2Tm", name_pt: "2 Timóteo", abbr_en: "2Ti", name_en: "2 Timothy", category: 6, chapters: 4 },
    // Row 6 (56..66)
    CanonicalBookInfo { order: 56, abbr_pt: "Tt", name_pt: "Tito", abbr_en: "Tit", name_en: "Titus", category: 6, chapters: 3 },
    CanonicalBookInfo { order: 57, abbr_pt: "Fm", name_pt: "Filemom", abbr_en: "Phm", name_en: "Philemon", category: 6, chapters: 1 },
    CanonicalBookInfo { order: 58, abbr_pt: "Hb", name_pt: "Hebreus", abbr_en: "Heb", name_en: "Hebrews", category: 6, chapters: 13 },
    CanonicalBookInfo { order: 59, abbr_pt: "Tg", name_pt: "Tiago", abbr_en: "Jas", name_en: "James", category: 6, chapters: 5 },
    CanonicalBookInfo { order: 60, abbr_pt: "1Pe", name_pt: "1 Pedro", abbr_en: "1Pe", name_en: "1 Peter", category: 6, chapters: 5 },
    CanonicalBookInfo { order: 61, abbr_pt: "2Pe", name_pt: "2 Pedro", abbr_en: "2Pe", name_en: "2 Peter", category: 6, chapters: 3 },
    CanonicalBookInfo { order: 62, abbr_pt: "1Jo", name_pt: "1 João", abbr_en: "1Jn", name_en: "1 John", category: 6, chapters: 5 },
    CanonicalBookInfo { order: 63, abbr_pt: "2Jo", name_pt: "2 João", abbr_en: "2Jn", name_en: "2 John", category: 6, chapters: 1 },
    CanonicalBookInfo { order: 64, abbr_pt: "3Jo", name_pt: "3 João", abbr_en: "3Jn", name_en: "3 John", category: 6, chapters: 1 },
    CanonicalBookInfo { order: 65, abbr_pt: "Jd", name_pt: "Judas", abbr_en: "Jud", name_en: "Jude", category: 6, chapters: 1 },
    CanonicalBookInfo { order: 66, abbr_pt: "Ap", name_pt: "Apocalipse", abbr_en: "Rev", name_en: "Revelation", category: 7, chapters: 22 },
];

pub fn canonical_book_by_order(order: i32) -> Option<&'static CanonicalBookInfo> {
    CANONICAL_BIBLE_BOOKS
        .iter()
        .find(|b| b.order == order)
}

#[derive(Deserialize)]
struct RawJsonBook {
    name: String,
    chapters: Vec<Vec<String>>,
}

/// Decompresses the embedded complete 66-book Public Domain Portuguese Bible (`TB1917`).
pub fn load_full_tb1917_verses(version_id: i64) -> Result<Vec<BibleVerse>> {
    let mut decoder = GzDecoder::new(TB1917_GZ);
    let mut json_str = String::with_capacity(4_100_000);
    decoder
        .read_to_string(&mut json_str)
        .context("Failed to decompress embedded tb1917.json.gz")?;

    let raw_books: Vec<RawJsonBook> =
        serde_json::from_str(&json_str).context("Failed to parse embedded tb1917.json")?;

    let mut verses = Vec::with_capacity(31_150);
    for (book_idx, raw_book) in raw_books.into_iter().enumerate() {
        let book_order = (book_idx + 1) as i32;
        let canonical_name = canonical_book_by_order(book_order)
            .map(|c| c.name_pt.to_string())
            .unwrap_or(raw_book.name);

        for (chap_idx, raw_chap) in raw_book.chapters.into_iter().enumerate() {
            let chapter = (chap_idx + 1) as i32;
            for (verse_idx, verse_text) in raw_chap.into_iter().enumerate() {
                let verse = (verse_idx + 1) as i32;
                verses.push(BibleVerse {
                    version_id,
                    book: canonical_name.clone(),
                    book_order,
                    chapter,
                    verse,
                    text: verse_text,
                });
            }
        }
    }
    Ok(verses)
}

/// Rich studio preset themes for the Holyrics-style visual theme strip.
pub fn default_studio_themes() -> Vec<Theme> {
    let mut noire = ThemeConfig::default();
    noire.background = BackgroundSpec::Gradient {
        top: RgbaColor::new(14, 14, 18, 255),
        bottom: RgbaColor::new(5, 5, 8, 255),
    };

    let mut aurora = ThemeConfig::default();
    aurora.background = BackgroundSpec::Video {
        path: "motion://aurora".to_string(),
        loop_playback: true,
        dim_opacity: 0.25,
    };

    let mut ocean = ThemeConfig::default();
    ocean.background = BackgroundSpec::Video {
        path: "motion://ocean".to_string(),
        loop_playback: true,
        dim_opacity: 0.25,
    };

    let mut warm = ThemeConfig::default();
    warm.background = BackgroundSpec::Video {
        path: "motion://warm".to_string(),
        loop_playback: true,
        dim_opacity: 0.25,
    };

    let mut purple = ThemeConfig::default();
    purple.background = BackgroundSpec::Gradient {
        top: RgbaColor::new(38, 16, 68, 255),
        bottom: RgbaColor::new(10, 5, 22, 255),
    };

    let mut emerald = ThemeConfig::default();
    emerald.background = BackgroundSpec::Gradient {
        top: RgbaColor::new(8, 48, 38, 255),
        bottom: RgbaColor::new(4, 16, 14, 255),
    };

    let mut crimson = ThemeConfig::default();
    crimson.background = BackgroundSpec::Gradient {
        top: RgbaColor::new(58, 14, 24, 255),
        bottom: RgbaColor::new(16, 4, 8, 255),
    };

    vec![
        Theme {
            id: 0,
            name: "Noite Estrelada".to_string(),
            is_return_theme: false,
            config: noire,
        },
        Theme::default_main(),
        Theme {
            id: 0,
            name: "Aurora Boreal (Motion)".to_string(),
            is_return_theme: false,
            config: aurora,
        },
        Theme {
            id: 0,
            name: "Oceano Profundo (Motion)".to_string(),
            is_return_theme: false,
            config: ocean,
        },
        Theme {
            id: 0,
            name: "Âmbar Adoração (Motion)".to_string(),
            is_return_theme: false,
            config: warm,
        },
        Theme {
            id: 0,
            name: "Roxo Majestade".to_string(),
            is_return_theme: false,
            config: purple,
        },
        Theme {
            id: 0,
            name: "Esmeralda Suave".to_string(),
            is_return_theme: false,
            config: emerald,
        },
        Theme {
            id: 0,
            name: "Vinho Aliança".to_string(),
            is_return_theme: false,
            config: crimson,
        },
        Theme::default_scripture(),
        Theme::default_return_monitor(),
    ]
}

/// Fast lightweight seed used in in-memory unit tests, plus initial metadata for file DBs.
pub fn builtin_public_domain_bibles() -> Vec<(BibleVersion, Vec<BibleVerse>)> {
    let pt_version = BibleVersion {
        id: 1,
        abbr: "TB1917".to_string(),
        name: "Tradução Brasileira (1917 — Domínio Público)".to_string(),
        lang: "pt-BR".to_string(),
        license: "Public Domain".to_string(),
    };

    let pt_verses = vec![
        ("Gênesis", 1, 1, 1, "No princípio criou Deus os céus e a terra."),
        ("Gênesis", 1, 1, 3, "Disse Deus: Haja luz; e houve luz."),
        ("Salmos", 19, 23, 1, "Jeová é o meu pastor; nada me faltará."),
        ("Salmos", 19, 23, 2, "Deitar-me faz em pastos verdes; guia-me mansamente a águas tranquilas."),
        ("Salmos", 19, 23, 3, "Refrigera a minha alma; guia-me nas veredas da justiça por amor do seu nome."),
        ("Salmos", 19, 23, 4, "Ainda que eu ande pelo vale da sombra da morte, não temerei mal algum, porque tu estás comigo."),
        ("Salmos", 19, 23, 5, "Preparas uma mesa perante mim na presença dos meus inimigos; unges com óleo a minha cabeça, o meu cálice transborda."),
        ("Salmos", 19, 23, 6, "Certamente que a bondade e a misericórdia me seguirão todos os dias da minha vida, e habitarei na casa de Jeová por longos dias."),
        ("Salmos", 19, 46, 1, "Deus é o nosso refúgio e fortaleza, socorro bem presente na angústia."),
        ("Salmos", 19, 91, 1, "Aquele que habita no esconderijo do Altíssimo, à sombra do Todo-Poderoso descansará."),
        ("Salmos", 19, 91, 2, "Direi de Jeová: Ele é o meu refúgio e a minha fortaleza; o meu Deus, em quem confio."),
        ("Salmos", 19, 119, 105, "Lâmpada para os meus pés é a tua palavra, e luz para o meu caminho."),
        ("Salmos", 19, 121, 1, "Elevo os meus olhos para os montes: de onde me virá o socorro?"),
        ("Salmos", 19, 121, 2, "O meu socorro vem de Jeová, que fez os céus e a terra."),
        ("Salmos", 19, 150, 6, "Tudo quanto tem fôlego louve a Jeová. Louvai a Jeová!"),
        ("Isaías", 23, 40, 31, "Mas os que esperam em Jeová renovarão as suas forças; subirão com asas como águias; correrão e não se cansarão; andarão e não se fatigarão."),
        ("Isaías", 23, 53, 5, "Mas ele foi ferido por causa das nossas transgressões, e moído por causa das nossas iniquidades; o castigo que nos traz a paz estava sobre ele, e pelas suas pisaduras fomos sarados."),
        ("Mateus", 40, 5, 14, "Vós sois a luz do mundo; não se pode esconder uma cidade edificada sobre um monte."),
        ("Mateus", 40, 11, 28, "Vinde a mim, todos os que estais cansados e oprimidos, e eu vos aliviarei."),
        ("Mateus", 40, 28, 19, "Ide, portanto, fazei discípulos de todas as nações, batizando-os em nome do Pai, e do Filho, e do Espírito Santo."),
        ("Mateus", 40, 28, 20, "Ensinando-os a observar todas as coisas que vos tenho mandado; e eis que eu estou convosco todos os dias até a consumação do século."),
        ("João", 43, 1, 1, "No princípio era o Verbo, e o Verbo estava com Deus, e o Verbo era Deus."),
        ("João", 43, 3, 16, "Porque Deus amou o mundo de tal maneira que deu o seu Filho unigênito, para que todo aquele que nele crê não pereça, mas tenha a vida eterna."),
        ("João", 43, 8, 32, "E conhecereis a verdade, e a verdade vos libertará."),
        ("João", 43, 14, 6, "Respondeu-lhe Jesus: Eu sou o caminho, e a verdade, e a vida; ninguém vem ao Pai senão por mim."),
        ("Romanos", 45, 8, 28, "Sabemos que todas as coisas concorrem para o bem daqueles que amam a Deus, daqueles que são chamados segundo o seu propósito."),
        ("Romanos", 45, 8, 38, "Porque estou certo de que nem a morte, nem a vida, nem os anjos, nem os principados, nem as coisas presentes, nem as futuras, nem os poderes,"),
        ("Romanos", 45, 8, 39, "Nem a altura, nem a profundidade, nem qualquer outra criatura nos poderá separar do amor de Deus, que está em Cristo Jesus nosso Senhor."),
        ("Filipenses", 50, 4, 13, "Tudo posso naquele que me fortalece."),
        ("Hebreus", 58, 11, 1, "Ora, a fé é a substância das coisas esperadas, a prova das coisas que não se veem."),
        ("Apocalipse", 66, 21, 4, "Ele enxugará de seus olhos toda lágrima; e não haverá mais morte, nem haverá mais pranto, nem clamor, nem dor; porque já as primeiras coisas são passadas."),
    ]
    .into_iter()
    .map(|(book, order, ch, v, txt)| BibleVerse {
        version_id: 1,
        book: book.to_string(),
        book_order: order,
        chapter: ch,
        verse: v,
        text: txt.to_string(),
    })
    .collect();

    let en_version = BibleVersion {
        id: 2,
        abbr: "WEB".to_string(),
        name: "World English Bible (Public Domain)".to_string(),
        lang: "en".to_string(),
        license: "Public Domain".to_string(),
    };

    let en_verses = vec![
        ("Genesis", 1, 1, 1, "In the beginning, God created the heavens and the earth."),
        ("Psalms", 19, 23, 1, "Yahweh is my shepherd; I shall lack nothing."),
        ("Psalms", 19, 23, 2, "He makes me lie down in green pastures. He leads me beside still waters."),
        ("Psalms", 19, 23, 3, "He restores my soul. He guides me in the paths of righteousness for his name's sake."),
        ("Psalms", 19, 23, 4, "Even though I walk through the valley of the shadow of death, I will fear no evil, for you are with me."),
        ("Psalms", 19, 46, 1, "God is our refuge and strength, a very present help in trouble."),
        ("Psalms", 19, 119, 105, "Your word is a lamp to my feet, and a light for my path."),
        ("Isaiah", 23, 40, 31, "But those who wait for Yahweh will renew their strength. They will mount up with wings like eagles."),
        ("Matthew", 40, 11, 28, "Come to me, all you who labor and are heavily burdened, and I will give you rest."),
        ("John", 43, 1, 1, "In the beginning was the Word, and the Word was with God, and the Word was God."),
        ("John", 43, 3, 16, "For God so loved the world, that he gave his one and only Son, that whoever believes in him should not perish, but have eternal life."),
        ("John", 43, 14, 6, "Jesus said to him, \"I am the way, the truth, and the life. No one comes to the Father, except through me.\""),
        ("Romans", 45, 8, 28, "We know that all things work together for good for those who love God, for those who are called according to his purpose."),
        ("Philippians", 50, 4, 13, "I can do all things through Christ, who strengthens me."),
        ("Hebrews", 58, 11, 1, "Now faith is assurance of things hoped for, proof of things not seen."),
        ("Revelation", 66, 21, 4, "He will wipe away from them every tear from their eyes. Death will be no more; neither will there be mourning, nor crying, nor pain, any more."),
    ]
    .into_iter()
    .map(|(book, order, ch, v, txt)| BibleVerse {
        version_id: 2,
        book: book.to_string(),
        book_order: order,
        chapter: ch,
        verse: v,
        text: txt.to_string(),
    })
    .collect();

    vec![(pt_version, pt_verses), (en_version, en_verses)]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBibleRef {
    pub book_order: Option<i32>,
    pub book_query: String,
    pub chapter: Option<i32>,
    pub verse_start: Option<i32>,
    pub verse_end: Option<i32>,
}

/// Parses flexible Holyrics-style Bible references in Portuguese and English:
/// - Space, colon, dot, or comma separated: `"jo 3 16"`, `"João 3:16"`, `"gn 1.1"`, `"rm 8,28"`
/// - Glued references: `"gn1:1"`, `"jo3.16"`, `"1sm 17 45"`, `"1sm17:45"`, `"1 co 13 4-7"`, `"ii co 5 17"`
/// - Book + chapter only: `"sl 23"`, `"jo 3"`, `"jo 3:"`
/// - Book name or abbreviation only: `"gn"`, `"salmos"`, `"1co"`, `"jó"`, `"jo"`
pub fn parse_bible_reference(query: &str) -> Option<ParsedBibleRef> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (raw_book, numbers_tail) = split_book_and_numbers(trimmed)?;
    let book_info = resolve_canonical_book(&raw_book)?;

    let (chapter, verse_start, verse_end) = parse_chapter_and_verses(numbers_tail)?;

    Some(ParsedBibleRef {
        book_order: Some(book_info.order),
        book_query: crate::accent::fold_search_text(book_info.name_pt),
        chapter,
        verse_start,
        verse_end,
    })
}

fn split_book_and_numbers(s: &str) -> Option<(String, &str)> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut prefix_digit: Option<char> = None;
    let mut after_prefix = trimmed;

    // 1. Check Roman numeral prefix ("I ", "II ", "III ") before a book name
    let lower = trimmed.to_lowercase();
    for (roman, digit) in [("iii ", '3'), ("ii ", '2'), ("i ", '1')] {
        if lower.starts_with(roman) {
            let candidate = trimmed[roman.len()..].trim_start();
            if candidate.chars().next().is_some_and(|c| c.is_alphabetic()) {
                prefix_digit = Some(digit);
                after_prefix = candidate;
                break;
            }
        }
    }

    // 2. Check numeric ordinal prefix ("1 ", "1Sm", "1º Sm", "2ª Co", "3Jo")
    if prefix_digit.is_none() {
        let mut chars = trimmed.char_indices();
        if let Some((_, first @ ('1' | '2' | '3'))) = chars.next() {
            let mut idx_after = first.len_utf8();
            let tail = &trimmed[idx_after..];
            let mut tchars = tail.char_indices();
            if let Some((_, c)) = tchars.next()
                && matches!(c, 'º' | 'ª' | '.')
            {
                idx_after += c.len_utf8();
            }
            let candidate = trimmed[idx_after..].trim_start();
            if candidate.chars().next().is_some_and(|c| c.is_alphabetic()) {
                prefix_digit = Some(first);
                after_prefix = candidate;
            }
        }
    }

    // 3. Consume alphabetic characters, spaces, dots, and hyphens until the first ASCII digit (chapter)
    let mut split_byte = after_prefix.len();
    for (byte_idx, ch) in after_prefix.char_indices() {
        if ch.is_ascii_digit() {
            split_byte = byte_idx;
            break;
        }
    }

    let book_letters = after_prefix[..split_byte]
        .trim()
        .trim_end_matches(['.', ':', ',', ';', '-'])
        .trim();
    if book_letters.is_empty() {
        return None;
    }

    let raw_book = match prefix_digit {
        Some(d) => format!("{d} {book_letters}"),
        None => book_letters.to_string(),
    };
    let numbers_tail = after_prefix[split_byte..].trim();
    Some((raw_book, numbers_tail))
}

fn parse_chapter_and_verses(tail: &str) -> Option<(Option<i32>, Option<i32>, Option<i32>)> {
    let t = tail.trim();
    if t.is_empty() {
        return Some((None, None, None));
    }

    // Normalize " a " range (e.g. "jo 3 16 a 18") into "-"
    let normalized = t
        .replace(" a ", "-")
        .replace(" A ", "-")
        .replace(" - ", "-")
        .replace(" -", "-")
        .replace("- ", "-");

    // Split chapter and optional verse part by ':', '.', ',', or whitespace
    let mut chap_end = normalized.len();
    let mut sep_len = 0usize;
    for (idx, ch) in normalized.char_indices() {
        if matches!(ch, ':' | '.' | ',' | ';') || ch.is_whitespace() {
            chap_end = idx;
            sep_len = ch.len_utf8();
            break;
        }
    }

    let chap_str = normalized[..chap_end].trim();
    let chapter: i32 = chap_str.parse().ok()?;
    if chapter <= 0 {
        return None;
    }

    if sep_len == 0 {
        return Some((Some(chapter), None, None));
    }

    let verse_raw = normalized[chap_end + sep_len..]
        .trim()
        .trim_start_matches([':', '.', ',', ';'])
        .trim();

    // Operator may still be mid-typing ("jo 3:" or "jo 3 ")
    if verse_raw.is_empty() {
        return Some((Some(chapter), None, None));
    }

    if let Some((start_s, end_s)) = verse_raw.split_once('-') {
        let vs: i32 = start_s.trim().parse().ok()?;
        if vs <= 0 {
            return None;
        }
        let end_trim = end_s.trim();
        if end_trim.is_empty() {
            // Operator mid-typing "jo 3:16-"
            return Some((Some(chapter), Some(vs), Some(vs)));
        }
        let ve: i32 = end_trim.parse().ok()?;
        if ve <= 0 {
            return None;
        }
        Some((Some(chapter), Some(vs), Some(ve.max(vs))))
    } else {
        let vs: i32 = verse_raw.parse().ok()?;
        if vs <= 0 {
            return None;
        }
        Some((Some(chapter), Some(vs), Some(vs)))
    }
}

/// Resolves a Portuguese or English book name/abbreviation to its canonical 1..=66 entry.
pub fn resolve_canonical_book(alias: &str) -> Option<&'static CanonicalBookInfo> {
    let clean = alias.trim().trim_end_matches('.').trim();
    if clean.is_empty() {
        return None;
    }

    // 1. Disambiguate accented "Jó" / "Job" (Book 18) vs unaccented "Jo" / "João" (Book 43)
    let raw_lower = clean.to_lowercase();
    if raw_lower == "jó" || raw_lower == "livro de jó" || raw_lower == "job" {
        return canonical_book_by_order(18);
    }

    let folded = crate::accent::fold_search_text(clean);
    let compact = folded.replace(' ', "");
    if compact.is_empty() {
        return None;
    }

    if matches!(
        compact.as_str(),
        "jo" | "joao" | "saojoao" | "evangelhodejoao" | "jhn" | "john"
    ) {
        return canonical_book_by_order(43);
    }

    // 2. Exact match on canonical PT/EN abbreviations or full names
    for b in &CANONICAL_BIBLE_BOOKS {
        if b.order == 18 && compact == "jo" {
            continue;
        }
        if crate::accent::fold_search_text(b.abbr_pt).replace(' ', "") == compact
            || crate::accent::fold_search_text(b.abbr_en).replace(' ', "") == compact
            || crate::accent::fold_search_text(b.name_pt).replace(' ', "") == compact
            || crate::accent::fold_search_text(b.name_en).replace(' ', "") == compact
        {
            return Some(b);
        }
    }

    // 3. Common worship operator abbreviations (PT-BR & EN)
    let order = match compact.as_str() {
        "gen" | "gn" => 1,
        "exo" | "exod" => 2,
        "lev" => 3,
        "num" => 4,
        "deu" | "deut" => 5,
        "jos" => 6,
        "jui" | "juiz" => 7,
        "rut" => 8,
        "1sam" | "1s" => 9,
        "2sam" | "2s" => 10,
        "1rei" | "1reis" | "1r" => 11,
        "2rei" | "2reis" | "2r" => 12,
        "1cro" | "1cron" => 13,
        "2cro" | "2cron" => 14,
        "esd" => 15,
        "nee" | "neem" => 16,
        "est" => 17,
        "sal" | "salmo" | "slm" | "ps" | "psa" | "psalm" => 19,
        "pro" | "prov" | "prv" => 20,
        "ecl" | "ecle" => 21,
        "can" | "cant" | "canticos" | "canticodesalomao" | "cantaresdesalomao" => 22,
        "isa" => 23,
        "jer" => 24,
        "lam" => 25,
        "eze" | "ezeq" => 26,
        "dan" => 27,
        "ose" => 28,
        "joe" => 29,
        "amo" => 30,
        "oba" | "obad" => 31,
        "jon" | "jn" => 32,
        "miq" => 33,
        "nau" => 34,
        "hab" => 35,
        "sof" => 36,
        "age" => 37,
        "zac" => 38,
        "mal" => 39,
        "mat" => 40,
        "mar" | "marc" => 41,
        "luc" => 42,
        "ato" | "atosdosapostolos" => 44,
        "rom" => 45,
        "1cor" | "1c" => 46,
        "2cor" | "2c" => 47,
        "gal" => 48,
        "efe" | "efes" => 49,
        "fil" | "flp" | "filip" => 50,
        "col" => 51,
        "1tes" | "1tess" => 52,
        "2tes" | "2tess" => 53,
        "1tim" => 54,
        "2tim" => 55,
        "tit" => 56,
        "flm" | "file" | "filem" | "filemon" => 57,
        "heb" => 58,
        "tia" | "tiag" => 59,
        "1ped" | "1pd" | "1p" => 60,
        "2ped" | "2pd" | "2p" => 61,
        "1joao" | "1jn" | "1j" => 62,
        "2joao" | "2jn" | "2j" => 63,
        "3joao" | "3jn" | "3j" => 64,
        "jud" => 65,
        "apo" | "apoc" | "rev" => 66,
        _ => 0,
    };
    if order > 0 {
        return canonical_book_by_order(order);
    }

    // 4. Unambiguous 4+ char prefix match on book names (e.g. "genes", "deuter", "apocal")
    if compact.len() >= 4 {
        let mut matches = CANONICAL_BIBLE_BOOKS.iter().filter(|b| {
            let pt = crate::accent::fold_search_text(b.name_pt).replace(' ', "");
            let en = crate::accent::fold_search_text(b.name_en).replace(' ', "");
            pt.starts_with(&compact) || en.starts_with(&compact)
        });
        if let Some(first) = matches.next()
            && matches.next().is_none()
        {
            return Some(first);
        }
    }

    None
}

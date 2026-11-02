use overture_core::{BibleVerse, BibleVersion};

/// Built-in Public Domain scripture seed so Overture works out-of-the-box in both PT-BR and EN
/// with verified public-domain licenses (`Public Domain`). Users can also import full XML/JSON Bibles.
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

/// Parses human scripture references in PT-BR or EN such as:
/// - `"João 3:16"`, `"Jo 3:16"`, `"Salmos 23:1-6"`, `"Sl 23"`, `"John 3:16"`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBibleRef {
    pub book_query: String,
    pub chapter: i32,
    pub verse_start: Option<i32>,
    pub verse_end: Option<i32>,
}

pub fn parse_bible_reference(query: &str) -> Option<ParsedBibleRef> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Split right-most whitespace separating book name (possibly starting with "1 " / "2 ") from chapter[:verse]
    let last_space = trimmed.rfind(' ')?;
    let raw_book = trimmed[..last_space].trim();
    let numbers_part = trimmed[last_space + 1..].trim();
    if raw_book.is_empty() || numbers_part.is_empty() {
        return None;
    }

    let (chap_str, verse_part) = match numbers_part.split_once(':') {
        Some((c, v)) => (c.trim(), Some(v.trim())),
        None => (numbers_part, None),
    };
    let chapter: i32 = chap_str.parse().ok()?;
    if chapter <= 0 {
        return None;
    }

    let (verse_start, verse_end) = match verse_part {
        None => (None, None),
        Some(vp) => {
            if let Some((s, e)) = vp.split_once('-') {
                let vs: i32 = s.trim().parse().ok()?;
                let ve: i32 = e.trim().parse().ok()?;
                (Some(vs), Some(ve.max(vs)))
            } else {
                let vs: i32 = vp.parse().ok()?;
                (Some(vs), Some(vs))
            }
        }
    };

    let expanded_book = expand_book_alias(raw_book);
    Some(ParsedBibleRef {
        book_query: expanded_book,
        chapter,
        verse_start,
        verse_end,
    })
}

fn expand_book_alias(alias: &str) -> String {
    match crate::accent::fold_search_text(alias).as_str() {
        "gn" | "gen" => "genesis".to_string(),
        "ex" | "exo" => "exodo".to_string(),
        "sl" | "sal" | "salmo" | "ps" | "psa" => "salmos".to_string(),
        "pv" | "prov" => "proverbios".to_string(),
        "is" | "isa" => "isaias".to_string(),
        "mt" | "mat" => "mateus".to_string(),
        "mc" | "mar" => "marcos".to_string(),
        "lc" | "luc" => "lucas".to_string(),
        "jo" | "joao" => "joao".to_string(),
        "jn" | "jhn" => "john".to_string(),
        "at" | "atos" => "atos".to_string(),
        "rm" | "rom" => "romanos".to_string(),
        "fp" | "fil" | "flp" => "filipenses".to_string(),
        "hb" | "heb" => "hebreus".to_string(),
        "ap" | "apo" | "apoc" | "rev" => "apocalipse".to_string(),
        other => other.to_string(),
    }
}

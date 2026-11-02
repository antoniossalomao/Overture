//! Section 9 Fixed Regression Suite: 20 public-domain hymns/songs verifying slide counts,
//! stanza isolation, max 2 lines per slide, punctuation splitting, preposition attachment,
//! `quebra_forcada` detection, header removal, UTF-8 / ISO-8859-1 decoding, and SQLite batch import.

use encoding_rs::WINDOWS_1252;
use overture_import::{
    DEFAULT_MAX_LINE_CHARS, MAX_LINES_PER_SLIDE, is_attached_to_next_word, parse_txt_bytes,
    parse_txt_directory,
};
use overture_storage::Storage;

struct RegressionCase {
    file_name: &'static str,
    artist: &'static str,
    title_from_file: &'static str,
    expected_title: &'static str,
    iso_8859_1: bool,
    raw_content: &'static str,
    expected_slide_count: usize,
    expected_first_slide: &'static str,
    expect_forced_break: bool,
}

fn twenty_song_suite() -> Vec<RegressionCase> {
    vec![
        RegressionCase {
            file_name: "Martinho Lutero - Castelo Forte.txt",
            artist: "Martinho Lutero",
            title_from_file: "Castelo Forte",
            expected_title: "Castelo Forte",
            iso_8859_1: false,
            raw_content: "Castelo Forte\nFonte: Hinário Domínio Público\n\nCastelo forte é nosso Deus,\nEspada e bom escudo;\nCom seu poder defende os seus\nEm todo transe agudo.\n\nCom fúria pertinaz\nPersegue Satanás,\nCom ânimo cruel,\nAstuto e mui rebel.",
            expected_slide_count: 4,
            expected_first_slide: "Castelo forte é nosso Deus,\nEspada e bom escudo;",
            expect_forced_break: false,
        },
        RegressionCase {
            file_name: "Reginald Heber - Santo Santo Santo.txt",
            artist: "Reginald Heber",
            title_from_file: "Santo Santo Santo",
            expected_title: "Santo, Santo, Santo",
            iso_8859_1: true,
            raw_content: "Santo, Santo, Santo\nhttps://hinario.org/dominiopublico\n\nSanto! Santo! Santo! Deus Onipotente!\nCedo de manhã cantaremos Teu louvor.\n\nSanto! Santo! Santo! Deus Jeová Triúno!\nÉs um só Deus, excelso Criador.",
            expected_slide_count: 2,
            expected_first_slide: "Santo! Santo! Santo! Deus Onipotente!\nCedo de manhã cantaremos Teu louvor.",
            expect_forced_break: false,
        },
        RegressionCase {
            file_name: "John Newton - Maravilhosa Graça.txt",
            artist: "John Newton",
            title_from_file: "Maravilhosa Graça",
            expected_title: "Maravilhosa Graça",
            iso_8859_1: false,
            raw_content: "MARAVILHOSA GRAÇA\nwww.letras.example.org/newton\n\nMaravilhosa graça!\nQuão doce é o som\nQue salvou um pecador\nAssim como eu!\n\nEu estava perdido,\nMas agora fui achado;\nEstava cego outrora,\nMas hoje eu posso ver.",
            expected_slide_count: 4,
            expected_first_slide: "Maravilhosa graça!\nQuão doce é o som",
            expect_forced_break: false,
        },
        RegressionCase {
            file_name: "Carl Boberg - Grandioso És Tu.txt",
            artist: "Carl Boberg",
            title_from_file: "Grandioso És Tu",
            expected_title: "Grandioso És Tu",
            iso_8859_1: true,
            raw_content: "Grandioso És Tu\nDomínio Público\n\nSenhor meu Deus, quando eu maravilhado,\nContemplo a Tua imensa criação,\nO céu e a terra, os mares estrelados,\nMe vêm falar da Tua perfeição.\n\nEntão minh'alma canta a Ti, Senhor:\nGrandioso és Tu! Grandioso és Tu!",
            expected_slide_count: 3,
            expected_first_slide: "Senhor meu Deus, quando eu maravilhado,\nContemplo a Tua imensa criação,",
            expect_forced_break: false,
        },
        RegressionCase {
            file_name: "Henry Lyte - Comigo Habita.txt",
            artist: "Henry Lyte",
            title_from_file: "Comigo Habita",
            expected_title: "Comigo Habita",
            iso_8859_1: false,
            raw_content: "Comigo Habita\nFonte: Domínio Público\n\nComigo habita, ó Deus! A noite vem,\nAs trevas crescem, eis, Senhor, convém\nQue me socorra a Tua proteção;\nÓ vem fazer comigo habitação!",
            expected_slide_count: 2,
            expected_first_slide: "Comigo habita, ó Deus! A noite vem,\nAs trevas crescem, eis, Senhor, convém",
            expect_forced_break: false,
        },
        RegressionCase {
            file_name: "Sarah Adams - Mais Perto Quero Estar.txt",
            artist: "Sarah Adams",
            title_from_file: "Mais Perto Quero Estar",
            expected_title: "Mais Perto Quero Estar",
            iso_8859_1: false,
            raw_content: "01. Mais Perto Quero Estar\nFonte: Cantor Cristão Domínio Público\n\nMais perto quero estar,\nMeu Deus, de Ti!\nInda que seja a dor\nQue me una a Ti!\n\nSempre hei de suplicar:\nMais perto quero estar,\nMeu Deus, de Ti!",
            expected_slide_count: 4,
            expected_first_slide: "Mais perto quero estar,\nMeu Deus, de Ti!",
            expect_forced_break: false,
        },
        RegressionCase {
            file_name: "Fanny Crosby - Segurança Bendita.txt",
            artist: "Fanny Crosby",
            title_from_file: "Segurança Bendita",
            expected_title: "Segurança Bendita",
            iso_8859_1: true,
            raw_content: "Segurança Bendita\nDomínio Público\n\nQue segurança! Sou de Jesus!\nEu já desfruto o gozo da luz!\nSou por Jesus herdeiro de Deus,\nEle me leva à glória dos céus!",
            expected_slide_count: 2,
            expected_first_slide: "Que segurança! Sou de Jesus!\nEu já desfruto o gozo da luz!",
            expect_forced_break: false,
        },
        RegressionCase {
            file_name: "Horatio Spafford - Sou Feliz com Jesus.txt",
            artist: "Horatio Spafford",
            title_from_file: "Sou Feliz com Jesus",
            expected_title: "Sou Feliz com Jesus",
            iso_8859_1: false,
            raw_content: "Sou Feliz com Jesus\nDomínio Público\n\nSe paz a mais doce me deres gozar,\nSe dor a mais forte sofrer,\nOh! Seja o que for, Tu me fazes saber\nQue feliz com Jesus sempre sou!\n\nSou feliz com Jesus!\nSou feliz com Jesus, meu Senhor!",
            expected_slide_count: 3,
            expected_first_slide: "Se paz a mais doce me deres gozar,\nSe dor a mais forte sofrer,",
            expect_forced_break: false,
        },
        RegressionCase {
            file_name: "Charles Wesley - Vem Tu Eterno Rei.txt",
            artist: "Charles Wesley",
            title_from_file: "Vem Tu Eterno Rei",
            expected_title: "Vem, Tu, Eterno Rei",
            iso_8859_1: false,
            raw_content: "Vem, Tu, Eterno Rei\nDomínio Público\n\nVem, Tu, eterno Rei,\nEnsina a Tua grei\nA Te louvar!\nTu és o nosso Pai,\nTua glória nunca sai,\nSobre nós sempre vai\nResplandecer!",
            expected_slide_count: 4,
            expected_first_slide: "Vem, Tu, eterno Rei,\nEnsina a Tua grei",
            expect_forced_break: false,
        },
        RegressionCase {
            file_name: "Isaac Watts - Ao Contemplar a Rude Cruz.txt",
            artist: "Isaac Watts",
            title_from_file: "Ao Contemplar a Rude Cruz",
            expected_title: "Ao Contemplar a Rude Cruz",
            iso_8859_1: false,
            raw_content: "Ao Contemplar a Rude Cruz\nDomínio Público\n\nAo contemplar a rude cruz,\nEm que morreu o meu Jesus,\nRenuncio à glória mundanal\nE tenho tudo por banal.",
            expected_slide_count: 2,
            expected_first_slide: "Ao contemplar a rude cruz,\nEm que morreu o meu Jesus,",
            expect_forced_break: false,
        },
        RegressionCase {
            file_name: "Augustus Toplady - Rocha Eterna.txt",
            artist: "Augustus Toplady",
            title_from_file: "Rocha Eterna",
            expected_title: "Rocha Eterna",
            iso_8859_1: false,
            raw_content: "Rocha Eterna\nDomínio Público\n\nRocha eterna, foi na cruz\nQue morreste Tu, Jesus!\nVem de todo o mal salvar\nQuem em Ti se refugiar.",
            expected_slide_count: 2,
            expected_first_slide: "Rocha eterna, foi na cruz\nQue morreste Tu, Jesus!",
            expect_forced_break: false,
        },
        RegressionCase {
            file_name: "Joseph Scriven - Quão Bondoso Amigo.txt",
            artist: "Joseph Scriven",
            title_from_file: "Quão Bondoso Amigo",
            expected_title: "Quão Bondoso Amigo",
            iso_8859_1: true,
            raw_content: "Quão Bondoso Amigo\nDomínio Público\n\nQuão bondoso amigo é Cristo!\nCarregou a nossa dor,\nE nos manda que levemos\nOs cuidados ao Senhor.",
            expected_slide_count: 2,
            expected_first_slide: "Quão bondoso amigo é Cristo!\nCarregou a nossa dor,",
            expect_forced_break: false,
        },
        RegressionCase {
            file_name: "Robert Lowry - Alvo Mais que a Neve.txt",
            artist: "Robert Lowry",
            title_from_file: "Alvo Mais que a Neve",
            expected_title: "Alvo Mais que a Neve",
            iso_8859_1: false,
            raw_content: "Alvo Mais que a Neve\nDomínio Público\n\nBendito seja o Cordeiro\nQue na cruz por nós padeceu!\nBendito seja o Seu sangue\nQue por nós ali Ele verteu!",
            expected_slide_count: 2,
            expected_first_slide: "Bendito seja o Cordeiro\nQue na cruz por nós padeceu!",
            expect_forced_break: false,
        },
        // Case 14: Long line WITH natural comma break -> splits cleanly without forced_break
        RegressionCase {
            file_name: "Salmista Antigo - Louvor com Pontuação.txt",
            artist: "Salmista Antigo",
            title_from_file: "Louvor com Pontuação",
            expected_title: "Louvor com Pontuação",
            iso_8859_1: false,
            raw_content: "Louvor com Pontuação\nDomínio Público\n\nCantai alegremente ao Deus da nossa eterna salvação, erguei as mãos no santuário com gratidão.",
            expected_slide_count: 1,
            expected_first_slide: "Cantai alegremente ao Deus da nossa eterna salvação,\nerguei as mãos no santuário com gratidão.",
            expect_forced_break: false,
        },
        // Case 15: Long line WITHOUT punctuation -> splits near midpoint respecting prepositions + flags forced_break
        RegressionCase {
            file_name: "Salmista Antigo - Cântico Sem Pontuação.txt",
            artist: "Salmista Antigo",
            title_from_file: "Cântico Sem Pontuação",
            expected_title: "Cântico Sem Pontuação",
            iso_8859_1: false,
            raw_content: "Cântico Sem Pontuação\nDomínio Público\n\nCaminharemos juntos cantando hinos eternos pra sempre na presença do Criador",
            expected_slide_count: 1,
            expected_first_slide: "Caminharemos juntos cantando hinos\neternos pra sempre na presença do Criador",
            expect_forced_break: true,
        },
        // Case 16: English Public Domain Hymn (Amazing Grace)
        RegressionCase {
            file_name: "John Newton - Amazing Grace.txt",
            artist: "John Newton",
            title_from_file: "Amazing Grace",
            expected_title: "Amazing Grace",
            iso_8859_1: false,
            raw_content: "Amazing Grace\nPublic Domain\n\nAmazing grace! How sweet the sound\nThat saved a wretch like me!\nI once was lost, but now am found;\nWas blind, but now I see.",
            expected_slide_count: 2,
            expected_first_slide: "Amazing grace! How sweet the sound\nThat saved a wretch like me!",
            expect_forced_break: false,
        },
        // Case 17: English Public Domain Hymn (Be Thou My Vision)
        RegressionCase {
            file_name: "Dallan Forgaill - Be Thou My Vision.txt",
            artist: "Dallan Forgaill",
            title_from_file: "Be Thou My Vision",
            expected_title: "Be Thou My Vision",
            iso_8859_1: false,
            raw_content: "Be Thou My Vision\nPublic Domain\n\nBe Thou my Vision, O Lord of my heart;\nNaught be all else to me, save that Thou art.\n\nThou my best Thought, by day or by night,\nWaking or sleeping, Thy presence my light.",
            expected_slide_count: 2,
            expected_first_slide: "Be Thou my Vision, O Lord of my heart;\nNaught be all else to me, save that Thou art.",
            expect_forced_break: false,
        },
        // Case 18: Section markers `[Coro]` / `[Verso 1]` stripped cleanly without mixing stanzas
        RegressionCase {
            file_name: "Hinário Clássico - Firme nas Promessas.txt",
            artist: "Hinário Clássico",
            title_from_file: "Firme nas Promessas",
            expected_title: "Firme nas Promessas",
            iso_8859_1: false,
            raw_content: "Firme nas Promessas\nDomínio Público\n\n[Verso 1]\nFirme nas promessas do meu Salvador,\nCantarei louvores ao meu Criador.\n\n[Coro]\nFirme, firme,\nFirme nas promessas de Jesus, meu Mestre!",
            expected_slide_count: 2,
            expected_first_slide: "Firme nas promessas do meu Salvador,\nCantarei louvores ao meu Criador.",
            expect_forced_break: false,
        },
        // Case 19: File with NO header at all -> keeps first lyric line and takes title from filename
        RegressionCase {
            file_name: "Thomas Chisholm - Tu És Fiel Senhor.txt",
            artist: "Thomas Chisholm",
            title_from_file: "Tu És Fiel Senhor",
            expected_title: "Tu És Fiel Senhor",
            iso_8859_1: false,
            raw_content: "Tu és fiel, Senhor, meu Pai celeste:\nPleno poder aos Teus filhos darás;\nNunca mudaste, Tu nunca faltaste,\nTal como eras, Tu sempre serás!",
            expected_slide_count: 2,
            expected_first_slide: "Tu és fiel, Senhor, meu Pai celeste:\nPleno poder aos Teus filhos darás;",
            expect_forced_break: false,
        },
        // Case 20: Prepositions `de`, `da`, `no`, `pra` under tight line wrap never stranded
        RegressionCase {
            file_name: "Cantor Antigo - Preposições Unidas.txt",
            artist: "Cantor Antigo",
            title_from_file: "Preposições Unidas",
            expected_title: "Preposições Unidas",
            iso_8859_1: true,
            raw_content: "Preposições Unidas\nDomínio Público\n\nEntrarei agora mesmo com alegria e gratidão no santuário da eterna aliança\nCantarei hinos sublimes de vitória pra glória do Rei eterno",
            expected_slide_count: 2,
            expected_first_slide: "Entrarei agora mesmo com alegria\ne gratidão no santuário da eterna aliança",
            expect_forced_break: true,
        },
    ]
}

#[test]
fn regression_20_songs_exact_slides_and_invariants() {
    let suite = twenty_song_suite();
    assert_eq!(suite.len(), 20);

    for (idx, case) in suite.iter().enumerate() {
        let bytes: Vec<u8> = if case.iso_8859_1 {
            let (enc, _, _) = WINDOWS_1252.encode(case.raw_content);
            enc.into_owned()
        } else {
            case.raw_content.as_bytes().to_vec()
        };

        let parsed = parse_txt_bytes(
            &bytes,
            case.file_name,
            case.artist,
            case.title_from_file,
            "public_domain_regression",
            DEFAULT_MAX_LINE_CHARS,
        );

        assert_eq!(
            parsed.song.title, case.expected_title,
            "case #{idx} ({}) title mismatch",
            case.file_name
        );
        assert_eq!(
            parsed.song.slides.len(),
            case.expected_slide_count,
            "case #{idx} ({}) slide count mismatch; slides = {:?}",
            case.file_name,
            parsed.song.slides
        );
        assert_eq!(
            parsed.song.slides[0].text, case.expected_first_slide,
            "case #{idx} ({}) first slide mismatch",
            case.file_name
        );
        assert_eq!(
            parsed.validation.forced_break_slides > 0,
            case.expect_forced_break,
            "case #{idx} ({}) forced_break mismatch",
            case.file_name
        );

        // Universal slide invariants across all 20 songs:
        for slide in &parsed.song.slides {
            let lines: Vec<&str> = slide.text.lines().collect();
            assert!(
                !lines.is_empty() && lines.len() <= MAX_LINES_PER_SLIDE,
                "case #{idx}: slide has {} lines (max 2): {:?}",
                lines.len(),
                slide.text
            );
            // If forced_break was applied, verify no line ends with a stranded preposition/article
            if slide.forced_break {
                for l in &lines {
                    let last = l.split_whitespace().next_back().unwrap_or("");
                    assert!(
                        !is_attached_to_next_word(last),
                        "case #{idx}: stranded preposition/article `{last}` in `{l}`"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn batch_directory_import_deduplicates_and_persists_to_sqlite() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let suite = twenty_song_suite();

    for case in &suite {
        let bytes: Vec<u8> = if case.iso_8859_1 {
            let (enc, _, _) = WINDOWS_1252.encode(case.raw_content);
            enc.into_owned()
        } else {
            case.raw_content.as_bytes().to_vec()
        };
        std::fs::write(temp.path().join(case.file_name), bytes)?;
    }

    // Add 1 exact duplicate under a different filename -> must be removed automatically
    std::fs::write(
        temp.path().join("Outro Nome - Copia Exata.txt"),
        suite[0].raw_content.as_bytes(),
    )?;

    // Add 1 same title + artist with different arrangement/second verse -> must be marked duplicate
    std::fs::write(
        temp.path().join("Martinho Lutero - Castelo Forte (2).txt"),
        "Castelo Forte\nDomínio Público\n\nSe nos quisessem devorar\nDemônios não contados,\nNão nos iriam derrotar\nNem ver-nos assustados.",
    )?;
    // Rename stem so filename artist & title match "Martinho Lutero - Castelo Forte" via sub-test:
    let sub = temp.path().join("sub");
    std::fs::create_dir_all(&sub)?;
    std::fs::write(
        sub.join("Martinho Lutero - Castelo Forte.txt"),
        "Castelo Forte\nDomínio Público\n\nSe nos quisessem devorar\nDemônios não contados,",
    )?;

    let (songs, report) =
        parse_txt_directory(temp.path(), "user_folder", DEFAULT_MAX_LINE_CHARS)?;
    assert_eq!(report.exact_duplicates_removed, 1);
    assert_eq!(songs.len(), 21);

    let store = Storage::connect_memory().await?;
    for s in &songs {
        store.insert_song(s, true).await?;
    }
    // Insert the sub-directory song with identical Artist + Title to verify SQLite duplicate_title_artist marking
    let (sub_songs, _) = parse_txt_directory(&sub, "user_folder", DEFAULT_MAX_LINE_CHARS)?;
    let outcome = store.insert_song(&sub_songs[0], true).await?;
    match outcome {
        overture_storage::SongInsertOutcome::Inserted {
            duplicate_title_artist,
            ..
        } => assert!(duplicate_title_artist),
        other => panic!("expected Inserted with duplicate_title_artist, got {other:?}"),
    }

    let found = store.search_songs("Castelo Forte", false, None, 20).await?;
    assert_eq!(found.len(), 3);
    assert!(found.iter().any(|s| s.duplicate_marked));
    Ok(())
}

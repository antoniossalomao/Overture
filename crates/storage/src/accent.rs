/// Accent-insensitive, case-insensitive normalization for fast Portuguese/English search.
pub fn fold_search_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = true;

    for ch in input.chars() {
        let mapped = match ch.to_lowercase().next().unwrap_or(ch) {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        };

        if mapped == ' ' {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(mapped);
            prev_space = false;
        }
    }

    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Canonical lyrics normalization for exact duplicate detection.
pub fn canonical_lyrics_key(slides_text: &[&str]) -> String {
    let joined = slides_text.join("\n");
    fold_search_text(&joined)
}

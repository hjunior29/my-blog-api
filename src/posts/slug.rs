pub const MAX_SLUG_LEN: usize = 120;

fn transliterate_char(c: char) -> Option<char> {
    match c {
        'a'..='z' | '0'..='9' => Some(c),
        'A'..='Z' => Some(c.to_ascii_lowercase()),
        'á' | 'Á' | 'à' | 'À' | 'ã' | 'Ã' | 'â' | 'Â' | 'ä' | 'Ä' | 'å' | 'Å' | 'ā' | 'Ā' => {
            Some('a')
        }
        'é' | 'É' | 'è' | 'È' | 'ê' | 'Ê' | 'ë' | 'Ë' | 'ē' | 'Ē' => Some('e'),
        'í' | 'Í' | 'ì' | 'Ì' | 'î' | 'Î' | 'ï' | 'Ï' | 'ī' | 'Ī' => Some('i'),
        'ó' | 'Ó' | 'ò' | 'Ò' | 'õ' | 'Õ' | 'ô' | 'Ô' | 'ö' | 'Ö' | 'ō' | 'Ō' => {
            Some('o')
        }
        'ú' | 'Ú' | 'ù' | 'Ù' | 'û' | 'Û' | 'ü' | 'Ü' | 'ū' | 'Ū' => Some('u'),
        'ç' | 'Ç' | 'ć' | 'Ć' | 'č' | 'Č' => Some('c'),
        'ñ' | 'Ñ' | 'ń' | 'Ń' => Some('n'),
        'š' | 'Š' => Some('s'),
        'ž' | 'Ž' => Some('z'),
        ' ' | '-' | '_' | '/' | '\\' | '.' | ',' | ':' | ';' | '!' | '?' | '(' | ')' | '['
        | ']' => Some('-'),
        _ => None,
    }
}

pub fn generate_slug(text: &str) -> String {
    let mut slug = String::with_capacity(text.len());
    let mut last_was_dash = true;

    for c in text.chars() {
        if let Some(trans) = transliterate_char(c) {
            if trans == '-' {
                if !last_was_dash {
                    slug.push('-');
                    last_was_dash = true;
                }
            } else {
                slug.push(trans);
                last_was_dash = false;
            }
        }
        if slug.len() >= MAX_SLUG_LEN {
            break;
        }
    }

    let trimmed = slug.trim_end_matches('-');
    if trimmed.is_empty() {
        "post".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_generation_handles_accents_and_punctuation() {
        assert_eq!(
            generate_slug("Olá, Mundo! Programação em Rust 2026"),
            "ola-mundo-programacao-em-rust-2026"
        );
        assert_eq!(
            generate_slug("---Atenção: Teste de Slugs!!!---"),
            "atencao-teste-de-slugs"
        );
        assert_eq!(
            generate_slug("Últimas Notícias e Álbum de Fotos"),
            "ultimas-noticias-e-album-de-fotos"
        );
        assert_eq!(generate_slug("   "), "post");
    }
}

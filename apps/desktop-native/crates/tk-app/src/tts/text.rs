//! KittenTTS text front end (worker **m**): English text clean-up, sentence chunking, punctuation-preserving
//! phrase split for `espeak-ng`, and phoneme string -> model token ids. Pure functions (tested below).
//!
//! Ported from the Apache-2.0 JS SDK `@biwills/kittentts` (`dist/text-preprocess-*.js`: `chunkText`,
//! `ensurePunctuation`, `isSentenceBoundary`; `dist/runtime-*.js`: `phonemizeWithPreservedPunctuation`,
//! `phonemesToInputIds`) and the symbol table of KittenTTS (`KittenML/KittenTTS`, Apache-2.0), see
//! `THIRD_PARTY_NOTICES.md`.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Padding symbol (id 0).
const PAD: char = '$';
const PUNCTUATION: &str = "\u{3B}\u{3A}\u{2C}\u{2E}\u{21}\u{3F}\u{A1}\u{BF}\u{2014}\u{2026}\u{22}\u{AB}\u{BB}\u{201C}\u{201D}\u{20}";
const LETTERS_IPA: &str = "\u{251}\u{250}\u{252}\u{E6}\u{253}\u{299}\u{3B2}\u{254}\u{255}\u{E7}\u{257}\u{256}\u{F0}\u{2A4}\u{259}\u{258}\u{25A}\u{25B}\u{25C}\u{25D}\u{25E}\u{25F}\u{284}\u{261}\u{260}\u{262}\u{29B}\u{266}\u{267}\u{127}\u{265}\u{29C}\u{268}\u{26A}\u{29D}\u{26D}\u{26C}\u{26B}\u{26E}\u{29F}\u{271}\u{26F}\u{270}\u{14B}\u{273}\u{272}\u{274}\u{F8}\u{275}\u{278}\u{3B8}\u{153}\u{276}\u{298}\u{279}\u{27A}\u{27E}\u{27B}\u{280}\u{281}\u{27D}\u{282}\u{283}\u{288}\u{2A7}\u{289}\u{28A}\u{28B}\u{2C71}\u{28C}\u{263}\u{264}\u{28D}\u{3C7}\u{28E}\u{28F}\u{291}\u{290}\u{292}\u{294}\u{2A1}\u{295}\u{2A2}\u{1C0}\u{1C1}\u{1C2}\u{1C3}\u{2C8}\u{2CC}\u{2D0}\u{2D1}\u{2BC}\u{2B4}\u{2B0}\u{2B1}\u{2B2}\u{2B7}\u{2E0}\u{2E4}\u{2DE}\u{2193}\u{2191}\u{2192}\u{2197}\u{2198}\u{27}\u{329}\u{27}\u{1D7B}";
const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Punctuation kept around the phrases sent to `espeak-ng` (it drops punctuation from its IPA output).
const PRESERVED: &[char] = &[
    ',', ':', ';', '.', '!', '?', '¡', '¿', '—', '…', '«', '»', '"', '“', '”',
];

/// Abbreviations whose final dot is not a sentence boundary.
const NON_BOUNDARY: [&str; 25] = [
    "dr", "prof", "mr", "mrs", "ms", "fig", "figs", "pp", "p", "ch", "sec", "jan", "feb", "mar", "apr",
    "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec", "al", "vs",
];

/// Longest chunk the model gets at once (characters).
pub const MAX_CHUNK: usize = 400;

/// Symbol -> id. Built like the Python `{s: i for i, s in enumerate(symbols)}`: a repeated symbol keeps its last id.
fn symbol_ids() -> &'static HashMap<char, i64> {
    static MAP: OnceLock<HashMap<char, i64>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        let all = std::iter::once(PAD)
            .chain(PUNCTUATION.chars())
            .chain(LETTERS.chars())
            .chain(LETTERS_IPA.chars());
        for (i, c) in all.enumerate() {
            m.insert(c, i as i64);
        }
        m
    })
}

/// `basicEnglishTokenize(phonemes).join(" ")`: runs of letters / digits / `_` are words, every other non-space
/// character is a token of its own.
pub fn tokenize_phonemes(phonemes: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut word = String::new();
    for c in phonemes.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            tokens.push(std::mem::take(&mut word));
        }
        if !c.is_whitespace() {
            tokens.push(c.to_string());
        }
    }
    if !word.is_empty() {
        tokens.push(word);
    }
    tokens.join(" ")
}

/// Model input ids of a phoneme string: `[0, ...symbols, 10, 0]` (unknown symbols are skipped).
pub fn input_ids(phonemes: &str) -> Vec<i64> {
    let map = symbol_ids();
    let mut ids = vec![0];
    ids.extend(
        tokenize_phonemes(phonemes)
            .chars()
            .filter_map(|c| map.get(&c).copied()),
    );
    ids.push(10);
    ids.push(0);
    ids
}

/// Light English clean-up before phonemizing: drop URLs / e-mails / markup, expand plain numbers to words, keep
/// sentence punctuation, collapse whitespace.
pub fn preprocess(text: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for raw in text.split_whitespace() {
        let lower = raw.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") {
            continue;
        }
        if raw.contains('@') && raw.contains('.') && !raw.starts_with('@') {
            continue;
        }
        let cleaned: String = raw
            .chars()
            .filter(|c| !matches!(c, '<' | '>' | '*' | '_' | '#' | '~' | '`' | '|'))
            .collect();
        if cleaned.is_empty() {
            continue;
        }
        words.push(expand_number_token(&cleaned));
    }
    words.join(" ")
}

/// `1,234.5,` -> `one thousand two hundred thirty-four point five,`; anything else unchanged.
fn expand_number_token(tok: &str) -> String {
    let start = tok.find(|c: char| c.is_ascii_digit());
    let Some(start) = start else { return tok.to_owned() };
    let (prefix, rest) = tok.split_at(start);
    if !prefix.is_empty() && prefix != "-" && prefix != "(" && prefix != "\"" {
        return tok.to_owned();
    }
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == ',' || c == '.'))
        .unwrap_or(rest.len());
    let (num, suffix) = rest.split_at(end);
    // A trailing dot / comma belongs to the sentence, not the number.
    let num_trim = num.trim_end_matches(['.', ',']);
    let tail = &num[num_trim.len()..];
    if !suffix.is_empty() && suffix.chars().next().is_some_and(char::is_alphabetic) {
        return tok.to_owned(); // `2nd`, `10km`: leave to espeak
    }
    let digits: String = num_trim.chars().filter(|c| *c != ',').collect();
    let (int_part, frac) = match digits.split_once('.') {
        Some((i, f)) => (i.to_owned(), Some(f.to_owned())),
        None => (digits.clone(), None),
    };
    let Ok(n) = int_part.parse::<u64>() else {
        return tok.to_owned();
    };
    let mut out = String::new();
    out.push_str(if prefix == "-" { "minus " } else { prefix });
    out.push_str(&number_words(n));
    if let Some(f) = frac.filter(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit())) {
        out.push_str(" point");
        for d in f.chars() {
            out.push(' ');
            out.push_str(ONES[d.to_digit(10).unwrap_or(0) as usize]);
        }
    }
    out.push_str(tail);
    out.push_str(suffix);
    out
}

const ONES: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

fn below_thousand(n: u64) -> String {
    let mut parts: Vec<String> = Vec::new();
    let h = n / 100;
    let r = n % 100;
    if h > 0 {
        parts.push(format!("{} hundred", ONES[h as usize]));
    }
    if r > 0 {
        if r < 20 {
            parts.push(ONES[r as usize].to_owned());
        } else if r % 10 == 0 {
            parts.push(TENS[(r / 10) as usize].to_owned());
        } else {
            parts.push(format!("{}-{}", TENS[(r / 10) as usize], ONES[(r % 10) as usize]));
        }
    }
    parts.join(" ")
}

/// English words of `n` (short scale).
pub fn number_words(n: u64) -> String {
    if n == 0 {
        return "zero".to_owned();
    }
    const SCALE: [&str; 7] = [
        "",
        "thousand",
        "million",
        "billion",
        "trillion",
        "quadrillion",
        "quintillion",
    ];
    let mut groups = Vec::new();
    let mut rest = n;
    while rest > 0 {
        groups.push(rest % 1000);
        rest /= 1000;
    }
    let mut parts: Vec<String> = Vec::new();
    for (i, g) in groups.iter().enumerate().rev() {
        if *g == 0 {
            continue;
        }
        let words = below_thousand(*g);
        parts.push(if SCALE[i].is_empty() {
            words
        } else {
            format!("{words} {}", SCALE[i])
        });
    }
    parts.join(" ")
}

fn is_sentence_boundary(chars: &[char], i: usize) -> bool {
    let c = chars[i];
    if c != '.' && c != '!' && c != '?' {
        return false;
    }
    if c == '.' {
        if i > 0 && i + 1 < chars.len() && chars[i - 1].is_ascii_digit() && chars[i + 1].is_ascii_digit() {
            return false;
        }
        let mut start = i;
        while start > 0 && chars[start - 1].is_ascii_alphabetic() {
            start -= 1;
        }
        let token: String = chars[start..i].iter().collect::<String>().to_ascii_lowercase();
        if NON_BOUNDARY.contains(&token.as_str()) {
            return false;
        }
        if (token == "a" || token == "p") && chars.get(i + 1).is_some_and(|n| n.eq_ignore_ascii_case(&'m')) {
            return false;
        }
    }
    chars.get(i + 1).is_none_or(|n| n.is_whitespace())
}

/// `ensurePunctuation`: a chunk always ends in punctuation (`,` added otherwise).
pub fn ensure_punctuation(text: &str) -> String {
    let t = text.trim();
    if t.is_empty() || t.ends_with(['.', '!', '?', ',', ';', ':']) {
        t.to_owned()
    } else {
        format!("{t},")
    }
}

/// `chunkText`: sentences, each split at whitespace into pieces of at most `max_len` characters.
pub fn chunk_text(text: &str, max_len: usize) -> Vec<String> {
    let max_len = max_len.max(2);
    let chars: Vec<char> = text.chars().collect();
    let mut sentences: Vec<String> = Vec::new();
    let mut start = 0;
    for i in 0..chars.len() {
        if is_sentence_boundary(&chars, i) {
            sentences.push(chars[start..=i].iter().collect());
            start = i + 1;
        }
    }
    if start < chars.len() {
        sentences.push(chars[start..].iter().collect());
    }
    let mut chunks = Vec::new();
    for sentence in sentences {
        let mut remaining: Vec<char> = sentence.trim().chars().collect();
        while !remaining.is_empty() {
            let as_text: String = remaining.iter().collect();
            let punctuated = ensure_punctuation(&as_text);
            if punctuated.chars().count() <= max_len {
                chunks.push(punctuated);
                break;
            }
            let end = (max_len - 1).min(remaining.len());
            let mut boundary = end;
            while boundary > 0 && !remaining.get(boundary).is_some_and(|c| c.is_whitespace()) {
                boundary -= 1;
            }
            if boundary == 0 {
                boundary = end;
            }
            let part: String = remaining[..boundary].iter().collect();
            let part = part.trim();
            if !part.is_empty() {
                chunks.push(ensure_punctuation(part));
            }
            let rest: String = remaining[boundary..].iter().collect();
            remaining = rest.trim().chars().collect();
        }
    }
    chunks
}

/// One piece of a chunk for the phonemizer: a phrase to send to `espeak-ng`, or punctuation (with its spacing)
/// copied as is (`phonemizeWithPreservedPunctuation`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Phrase(String),
    Literal(String),
}

/// Split a chunk into phrases and preserved punctuation runs.
pub fn split_phrases(text: &str) -> Vec<Piece> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut phrase = String::new();
    while i < chars.len() {
        if PRESERVED.contains(&chars[i]) {
            let trailing: String = phrase.chars().rev().take_while(|c| c.is_whitespace()).collect();
            let body = phrase.trim_end().to_owned();
            if !body.is_empty() {
                out.push(Piece::Phrase(body));
            }
            let mut lit = trailing;
            while i < chars.len() && PRESERVED.contains(&chars[i]) {
                lit.push(chars[i]);
                i += 1;
            }
            let mut spacing = String::new();
            while i < chars.len() && chars[i].is_whitespace() {
                spacing.push(chars[i]);
                i += 1;
            }
            if spacing.is_empty() && i == chars.len() {
                spacing.push(' ');
            }
            lit.push_str(&spacing);
            out.push(Piece::Literal(lit));
            phrase.clear();
            continue;
        }
        phrase.push(chars[i]);
        i += 1;
    }
    if !phrase.trim().is_empty() {
        out.push(Piece::Phrase(phrase));
    }
    out
}

/// Join the IPA lines `espeak-ng --ipa` printed for one phrase.
pub fn clean_ipa(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_table_matches_kitten() {
        let map = symbol_ids();
        assert_eq!(map[&'$'], 0);
        assert_eq!(map[&';'], 1);
        assert_eq!(map[&'…'], 10);
        assert_eq!(map[&'"'], 11);
        assert_eq!(map[&' '], 16);
        assert_eq!(map[&'A'], 17);
        assert_eq!(map[&'a'], 43);
        assert_eq!(map[&'ɑ'], 69);
        assert_eq!(input_ids("a"), vec![0, 43, 10, 0]);
        assert_eq!(input_ids("həlˈoʊ"), {
            let mut v = vec![0];
            v.extend("həlˈoʊ".chars().map(|c| map[&c]));
            v.extend([10, 0]);
            v
        });
    }

    #[test]
    fn tokenizes_like_the_sdk() {
        assert_eq!(tokenize_phonemes("həlˈoʊ, wˈɜːld!"), "həlˈoʊ , wˈɜːld !");
        assert_eq!(tokenize_phonemes("  "), "");
    }

    #[test]
    fn numbers_become_words() {
        assert_eq!(number_words(0), "zero");
        assert_eq!(number_words(21), "twenty-one");
        assert_eq!(number_words(1_234), "one thousand two hundred thirty-four");
        assert_eq!(number_words(2_000_005), "two million five");
        assert_eq!(
            preprocess("Chapter 12. See https://x.y now"),
            "Chapter twelve. See now"
        );
        assert_eq!(preprocess("pi is 3.14, ok"), "pi is three point one four, ok");
        assert_eq!(preprocess("the 2nd -5"), "the 2nd minus five");
    }

    #[test]
    fn chunks_sentences_and_long_text() {
        assert_eq!(
            chunk_text("Hello there. Mr. Smith left! Done", 400),
            vec!["Hello there.", "Mr. Smith left!", "Done,"]
        );
        assert_eq!(chunk_text("3.5 apples.", 400), vec!["3.5 apples."]);
        let long = "word ".repeat(30);
        let chunks = chunk_text(&long, 40);
        assert!(chunks.len() > 2);
        assert!(chunks.iter().all(|c| c.chars().count() <= 40));
        assert!(chunk_text("   ", 400).is_empty());
    }

    #[test]
    fn phrases_keep_punctuation() {
        assert_eq!(
            split_phrases("Hello, world. Bye"),
            vec![
                Piece::Phrase("Hello".into()),
                Piece::Literal(", ".into()),
                Piece::Phrase("world".into()),
                Piece::Literal(". ".into()),
                Piece::Phrase("Bye".into()),
            ]
        );
        assert_eq!(
            split_phrases("End!"),
            vec![Piece::Phrase("End".into()), Piece::Literal("! ".into())]
        );
        assert_eq!(clean_ipa(" həlˈoʊ\n wˈɜːld \n"), "həlˈoʊ wˈɜːld");
    }
}

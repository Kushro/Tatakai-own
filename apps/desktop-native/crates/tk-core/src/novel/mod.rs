//! Novel chapter text (wave B, worker **m**; docs/redesign/16-parity-wave-b-contract.md section 3.5; checklist
//! `novel.chapters`, `novel.reader.*`). UI-free. m owns this directory. Final public shape: `paragraphs(html: &str) ->
//! Vec<String>` (HTML -> plain-text paragraphs with `scraper`: `<p>`, `<br><br>`, headings kept as their own
//! paragraph; scripts / styles / ads dropped; entities decoded), `word_count(&[String]) -> u32`, `Chapter` reuses
//! `crate::manga::Chapter`. Rows: `store/reading.rs` (`offline_chapters.text`).

/// Words of a chapter (whitespace-separated tokens; pure).
pub fn word_count(paragraphs: &[String]) -> u32 {
    paragraphs.iter().map(|p| p.split_whitespace().count() as u32).sum()
}

#[cfg(test)]
mod tests {
    #[test]
    fn counts_words() {
        assert_eq!(super::word_count(&["one two".into(), " three ".into()]), 3);
    }
}

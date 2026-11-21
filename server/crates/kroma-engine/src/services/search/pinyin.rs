//! Pinyin spellings of a title or a name, indexed beside the text itself so a
//! Chinese catalogue is reachable from a Latin keyboard: the syllables ("liu
//! lang di qiu"), the whole ("liulangdiqiu") and the initials ("lldq").
//!
//! Only text that holds a Chinese character gets spellings; a Latin title is
//! already indexed as written.

use pinyin::ToPinyin;

/// Matches the analyzer's own token-length cap: a compact form past it would be
/// dropped on the floor, so it is not emitted.
const MAX_COMPACT: usize = 40;
/// A one-letter initial matches half the alphabet; two is the floor worth
/// indexing.
const MIN_INITIALS: usize = 2;

pub(super) fn forms(text: &str) -> Vec<String> {
    if !text.chars().any(is_cjk) {
        return Vec::new();
    }
    let mut syllables: Vec<String> = Vec::new();
    let mut compact = String::new();
    let mut initials = String::new();
    let mut latin = String::new();

    // A run of Latin letters or digits is one syllable of its own: "流浪地球2"
    // keeps the 2 in every spelling.
    let flush = |latin: &mut String,
                 syllables: &mut Vec<String>,
                 compact: &mut String,
                 initials: &mut String| {
        if latin.is_empty() {
            return;
        }
        syllables.push(latin.clone());
        compact.push_str(latin);
        initials.extend(latin.chars().take(1));
        latin.clear();
    };

    for c in text.chars() {
        if is_cjk(c) {
            flush(&mut latin, &mut syllables, &mut compact, &mut initials);
            let Some(plain) = c.to_pinyin().map(|p| p.plain()) else {
                continue;
            };
            syllables.push(plain.to_string());
            compact.push_str(plain);
            initials.extend(plain.chars().take(1));
        } else if c.is_ascii_alphanumeric() {
            latin.push(c.to_ascii_lowercase());
        } else {
            flush(&mut latin, &mut syllables, &mut compact, &mut initials);
        }
    }
    flush(&mut latin, &mut syllables, &mut compact, &mut initials);

    let mut out: Vec<String> = Vec::new();
    if !syllables.is_empty() {
        out.push(syllables.join(" "));
    }
    if !compact.is_empty() && compact.len() <= MAX_COMPACT && !out.contains(&compact) {
        out.push(compact);
    }
    if initials.chars().count() >= MIN_INITIALS && !out.contains(&initials) {
        out.push(initials);
    }
    out
}

pub(super) fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chinese_title_is_spelled_three_ways() {
        let spellings = forms("流浪地球");

        assert!(
            spellings.contains(&"liu lang di qiu".to_string()),
            "{spellings:?}"
        );
        assert!(
            spellings.contains(&"liulangdiqiu".to_string()),
            "{spellings:?}"
        );
        assert!(spellings.contains(&"lldq".to_string()), "{spellings:?}");
    }

    #[test]
    fn a_number_stays_put_in_every_spelling() {
        let spellings = forms("流浪地球2");

        assert!(
            spellings.contains(&"liulangdiqiu2".to_string()),
            "{spellings:?}"
        );
        assert!(spellings.contains(&"lldq2".to_string()), "{spellings:?}");
    }

    #[test]
    fn a_latin_title_has_no_pinyin_spelling() {
        assert!(forms("Dune").is_empty());
        assert!(forms("Amélie").is_empty());
    }

    #[test]
    fn a_single_chinese_character_gets_no_one_letter_initials() {
        let spellings = forms("无");

        assert_eq!(spellings, ["wu"]);
    }
}

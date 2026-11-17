//! Metric drawing scales written on a page: a title block's "SCALE 1:100" or "Échelle 1/50",
//! read from the page's text so a plan can be measured without calibrating it.
//!
//! Only notation is read, never guessed from the drawing: a scale is offered when its text says
//! it is one. A bare `1/2` is a fraction and a `1/12/2026` a date, so a slash ratio without a
//! scale word nearby is offered only when it is one of the ratios drawings use.

use serde::Serialize;

/// Most characters of page text read: a title block is a few hundred, a dense sheet tens of
/// thousands. Text past this is not searched.
pub const MAX_TEXT: usize = 1 << 20;
/// Most scales returned for one page.
pub const MAX_FOUND: usize = 32;
/// How many characters before a ratio a scale word may sit ("SCALE : 1/100", "ECH. 1:50").
const KEYWORD_REACH: usize = 24;

/// Words that announce a drawing scale, lower case, in the languages architectural sheets use.
const KEYWORDS: [&str; 10] = ["scale", "échelle", "echelle", "éch", "ech", "esc", "maßstab", "massstab", "schaal", "scala"];
/// Ratios (the N of 1:N) drawings use: a slash ratio without a scale word is offered only when
/// it is one of these, so fractions and dates are not taken for scales.
const COMMON: [f64; 22] = [
    5.0, 10.0, 20.0, 25.0, 50.0, 75.0, 100.0, 125.0, 150.0, 200.0, 250.0, 300.0, 400.0, 500.0, 750.0, 1000.0, 1250.0, 1500.0, 2000.0, 2500.0, 5000.0,
    10000.0,
];

/// A scale found in a page's text.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FoundScale {
    /// The N of 1:N: one unit on paper is N of the same unit in reality.
    pub ratio: f64,
    /// The text it was read from, as written.
    pub text: String,
    /// A scale word ("SCALE", "Échelle"…) is written just before it.
    pub labelled: bool,
}

/// The drawing scales written in `text`, labelled ones first, then in reading order, without
/// repeats of the same ratio.
pub fn find(text: &str) -> Vec<FoundScale> {
    let chars: Vec<char> = text.chars().take(MAX_TEXT).map(normalize).collect();
    let lower: Vec<char> = chars.iter().flat_map(|c| c.to_lowercase()).collect();
    // Lower-casing can lengthen a character ('İ'); keyword search then reads the original.
    let lower = if lower.len() == chars.len() { lower } else { chars.clone() };
    let mut found: Vec<FoundScale> = Vec::new();
    let mut i = 0;
    while i < chars.len() && found.len() < MAX_FOUND * 4 {
        if let Some((scale, end)) = ratio_at(&chars, &lower, i) {
            found.push(scale);
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    let mut out: Vec<FoundScale> = Vec::new();
    for labelled in [true, false] {
        for f in found.iter().filter(|f| f.labelled == labelled) {
            if out.len() < MAX_FOUND && !out.iter().any(|o| (o.ratio - f.ratio).abs() < 1e-9) {
                out.push(f.clone());
            }
        }
    }
    out
}

/// Typographic quotes, primes, dashes and spaces as their plain forms.
fn normalize(c: char) -> char {
    match c {
        '\u{2033}' | '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{02BA}' => '"',
        '\u{2032}' | '\u{2018}' | '\u{2019}' | '\u{02B9}' | '`' | '\u{00B4}' => '\'',
        '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
        '\u{2044}' | '\u{2215}' => '/',
        '\u{00A0}' | '\u{2000}'..='\u{200B}' | '\u{202F}' | '\t' | '\r' | '\n' => ' ',
        '\u{FF1A}' => ':',
        _ => c,
    }
}

fn skip_spaces(chars: &[char], mut i: usize) -> usize {
    while chars.get(i) == Some(&' ') {
        i += 1;
    }
    i
}

/// A decimal number starting at `i` (a comma works as the decimal mark): its value and end.
fn number_at(chars: &[char], i: usize) -> Option<(f64, usize)> {
    let mut end = i;
    let mut text = String::new();
    let mut seen_mark = false;
    while let Some(&c) = chars.get(end) {
        if c.is_ascii_digit() {
            text.push(c);
        } else if (c == '.' || c == ',') && !seen_mark && chars.get(end + 1).is_some_and(char::is_ascii_digit) && !text.is_empty() {
            seen_mark = true;
            text.push('.');
        } else {
            break;
        }
        end += 1;
        if text.len() > 12 {
            return None;
        }
    }
    let value: f64 = text.parse().ok()?;
    value.is_finite().then_some((value, end))
}

/// `1:N` or `1/N` starting at `i`, the 1 not part of a longer number.
fn ratio_at(chars: &[char], lower: &[char], i: usize) -> Option<(FoundScale, usize)> {
    if chars.get(i) != Some(&'1') || i.checked_sub(1).and_then(|p| chars.get(p)).is_some_and(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '/')) {
        return None;
    }
    let after_one = i + 1;
    if chars.get(after_one).is_some_and(|c| c.is_ascii_digit() || matches!(c, '.' | ',')) {
        return None;
    }
    let sep_at = skip_spaces(chars, after_one);
    let sep = *chars.get(sep_at)?;
    if sep != ':' && sep != '/' {
        return None;
    }
    let n_at = skip_spaces(chars, sep_at + 1);
    let (n, mut end) = number_at(chars, n_at)?;
    // `1/12/2026` is a date and `1/4"` an inch fraction (imperial scales aren't read);
    // `1:100:5` is not a scale either.
    if chars.get(end).is_some_and(|c| matches!(c, '/' | ':' | '"' | '\'')) {
        return None;
    }
    if !(2.0..=1e6).contains(&n) {
        return None;
    }
    // French sheets write "1/100e" or "1/100ème".
    for suffix in ["ème", "eme", "e"] {
        let s: Vec<char> = suffix.chars().collect();
        if lower.get(end..end + s.len()) == Some(&s[..]) && !lower.get(end + s.len()).is_some_and(|c| c.is_alphanumeric()) {
            end += s.len();
            break;
        }
    }
    if chars.get(end).is_some_and(|c| c.is_alphanumeric()) {
        return None;
    }
    let labelled = labelled_before(lower, i);
    let common = COMMON.iter().any(|c| (c - n).abs() < 1e-9);
    if !labelled && !(common && (sep == ':' || n >= 20.0)) {
        return None;
    }
    let text: String = chars.get(i..end)?.iter().collect();
    Some((FoundScale { ratio: n, text, labelled }, end))
}

/// A scale word ends within [`KEYWORD_REACH`] characters before `i`, with only spaces and
/// punctuation between.
fn labelled_before(lower: &[char], i: usize) -> bool {
    let start = i.saturating_sub(KEYWORD_REACH);
    let Some(window) = lower.get(start..i) else { return false };
    let mut end = window.len();
    while end > 0 && window.get(end - 1).is_some_and(|c| !c.is_alphanumeric()) {
        end -= 1;
    }
    let Some(head) = window.get(..end) else { return false };
    KEYWORDS.iter().any(|k| {
        let k: Vec<char> = k.chars().collect();
        head.len() >= k.len()
            && head.get(head.len() - k.len()..) == Some(&k[..])
            && !head.len().checked_sub(k.len() + 1).and_then(|p| head.get(p)).is_some_and(|c| c.is_alphabetic())
    }) || head.last().is_some_and(|c| *c == 'm') && head.len() >= 2 && head.get(head.len() - 2) == Some(&' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratios(text: &str) -> Vec<f64> {
        find(text).iter().map(|f| f.ratio).collect()
    }

    #[test]
    fn metric_scales_are_read_with_or_without_a_scale_word() {
        assert_eq!(ratios("PLAN RDC  ECHELLE : 1/100"), vec![100.0]);
        assert_eq!(ratios("Échelle 1/50e"), vec![50.0]);
        assert_eq!(ratios("SCALE 1 : 200 @ A1"), vec![200.0]);
        assert_eq!(ratios("Coupe AA 1:20"), vec![20.0]);
        assert_eq!(ratios("Maßstab 1:75"), vec![75.0]);
        assert_eq!(ratios("Niveau 1 - 1/100ème"), vec![100.0]);
    }

    #[test]
    fn labelled_scales_come_first_and_repeats_go() {
        let found = find("Detail 1:20   Plan 1:100   SCALE 1:50   1:20");
        let r: Vec<f64> = found.iter().map(|f| f.ratio).collect();
        assert_eq!(r, vec![50.0, 20.0, 100.0]);
        assert!(found[0].labelled);
    }

    #[test]
    fn fractions_dates_and_numbers_are_not_scales() {
        assert!(ratios("1/2 of the wall").is_empty());
        assert!(ratios("Date 1/12/2026").is_empty());
        assert!(ratios("Room 11:100").is_empty());
        assert!(ratios("Index 21/100").is_empty());
        assert!(ratios("1/3").is_empty());
        assert!(ratios("ratio 1:100x").is_empty());
        assert!(ratios("").is_empty());
    }

    #[test]
    fn an_uncommon_unlabelled_ratio_is_not_offered_but_a_labelled_one_is() {
        assert!(ratios("1/37").is_empty());
        assert_eq!(ratios("Scale 1/37"), vec![37.0]);
    }

    #[test]
    fn imperial_scales_are_not_read() {
        assert!(ratios("SCALE: 1/4\" = 1'-0\"").is_empty());
        assert!(ratios("1\" = 20'").is_empty());
    }

    #[test]
    fn huge_or_hostile_text_stays_bounded() {
        let text = "1:100 ".repeat(100_000);
        assert_eq!(ratios(&text), vec![100.0]);
        let digits = "9".repeat(5000);
        assert!(ratios(&format!("1:{digits}")).is_empty());
        let many: String = (2..2000).map(|n| format!("Scale 1:{n} ")).collect();
        assert_eq!(find(&many).len(), MAX_FOUND);
    }
}

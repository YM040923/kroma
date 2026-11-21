//! The age limit a member carries, and the rating table it is measured
//! against. One place decides "may this viewer see this title": the browse
//! filters, the detail gates and the playback gates all ask the same value, so
//! a list can never narrow what the player still opens.
//!
//! Ratings are the provider's own symbols (PG-13, TV-MA, FSK 16, …); they are
//! folded into an age because every one of them was set with an age in mind.
//! A symbol this table does not know is treated as *unrated*, never guessed:
//! mapping an unfamiliar "T" to 0 would hand an adult film to a child.

use crate::model::{Metadata, User};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentLimit {
    max_age: u8,
    allow_unrated: bool,
}

/// The limit a user carries, or `None` when nothing is restricted. An account
/// that manages users, libraries or settings is never limited: it can lift the
/// restriction itself, so enforcing one would only hide the server from the
/// people running it.
pub fn for_user(user: &User) -> Option<ContentLimit> {
    if user.sees_every_library() {
        return None;
    }
    user.max_age.map(|max_age| ContentLimit {
        max_age: max_age.min(21),
        allow_unrated: user.allow_unrated,
    })
}

impl ContentLimit {
    /// Whether a title carrying `certification` may be seen.
    pub fn allows(&self, certification: Option<&str>) -> bool {
        match certification.map(str::trim).filter(|raw| !raw.is_empty()) {
            Some(raw) => match rating_age(raw) {
                Some(age) => age <= self.max_age,
                None => self.allow_unrated,
            },
            None => self.allow_unrated,
        }
    }

    pub fn allows_metadata(&self, metadata: Option<&Metadata>) -> bool {
        self.allows(metadata.and_then(|meta| meta.certification.as_deref()))
    }
}

/// A provider rating string → the age it stands for. Unknown strings answer
/// `None`; a bare number is the age itself (FSK 12, KMRB 15, …).
pub fn rating_age(raw: &str) -> Option<u8> {
    let key: String = raw
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect();
    if key.is_empty() {
        return None;
    }
    if let Some(age) = TABLE
        .iter()
        .find(|(symbol, _)| *symbol == key)
        .map(|(_, age)| *age)
    {
        return Some(age);
    }
    // A few sources prefix the system name (FSK16 / KMRB12 / BBFC15).
    for prefix in ["FSK", "KMRB", "CNC", "BBFC"] {
        if let Some(rest) = key.strip_prefix(prefix) {
            if let Some(age) = bare_age(rest) {
                return Some(age);
            }
        }
    }
    bare_age(&key)
}

fn bare_age(text: &str) -> Option<u8> {
    let digits = text.strip_suffix('+').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let age: u8 = digits.parse().ok()?;
    // Anything past 21 is a year or a count, not a rating.
    (age <= 21).then_some(age)
}

/// Symbols from the systems the catalogue actually carries. Where one symbol
/// means different things in different countries, the stricter age wins.
const TABLE: &[(&str, u8)] = &[
    // US film (MPA)
    ("G", 0),
    ("PG", 8),
    ("PG-13", 13),
    ("R", 17),
    ("NC-17", 18),
    // US television
    ("TV-Y", 0),
    ("TV-Y7", 7),
    ("TV-Y7-FV", 7),
    ("TV-G", 0),
    ("TV-PG", 8),
    ("TV-14", 14),
    ("TV-MA", 17),
    // UK (BBFC)
    ("U", 0),
    ("UC", 0),
    ("12A", 12),
    ("15", 15),
    ("18", 18),
    ("R18", 18),
    // France (CNC)
    ("TP", 0),
    ("T", 0),
    // Japan (Eirin)
    ("PG12", 12),
    ("R15+", 15),
    ("R18+", 18),
    // Korea
    ("ALL", 0),
    ("19", 19),
    // Australia (ACB)
    ("M", 15),
    ("MA15+", 15),
    ("R18+AU", 18),
    ("X18+", 18),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn limit(max_age: u8, allow_unrated: bool) -> ContentLimit {
        ContentLimit {
            max_age,
            allow_unrated,
        }
    }

    #[test]
    fn a_rating_becomes_the_age_it_was_set_for() {
        assert_eq!(rating_age("PG-13"), Some(13));
        assert_eq!(rating_age("TV-MA"), Some(17));
        assert_eq!(rating_age("R"), Some(17));
        assert_eq!(rating_age("FSK 16"), Some(16));
        assert_eq!(rating_age("12+"), Some(12));
    }

    #[test]
    fn an_unknown_symbol_is_not_guessed_at() {
        assert_eq!(rating_age("Unrated"), None);
        assert_eq!(rating_age("XYZ"), None);
        assert_eq!(rating_age("2024"), None, "a year is not an age");
        assert_eq!(rating_age(""), None);
    }

    #[test]
    fn a_known_rating_over_the_limit_is_hidden() {
        let twelve = limit(12, true);

        assert!(twelve.allows(Some("PG-13")));
        assert!(twelve.allows(Some("PG")));
        assert!(!twelve.allows(Some("R")));
        assert!(!twelve.allows(Some("TV-MA")));
    }

    #[test]
    fn an_unrated_title_follows_the_accounts_choice() {
        assert!(limit(12, true).allows(None));
        assert!(limit(12, true).allows(Some("")));
        assert!(limit(12, true).allows(Some("XYZ")), "unknown is unrated");
        assert!(!limit(12, false).allows(None));
        assert!(!limit(12, false).allows(Some("Unrated")));
    }
}

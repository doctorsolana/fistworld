//! Character names.
//!
//! Every person in the world gets a name from here, so the encyclopedia lists
//! *people* rather than entity ids. Names are generated from a `u64` seed and are
//! **deterministic**: the same seed always yields the same name, on the server,
//! on every client, and after a restart. That matters because a name is
//! identity — a villager who is Aldric Fenn today and Bryn Ashdown tomorrow is
//! not a person the player can come to know.
//!
//! Curated word lists rather than syllable mashing. Procedural syllables produce
//! endless unpronounceable filler ("Zrengoth", "Vaelthux"); a few hundred real
//! early-medieval English and Norse elements recombine into names that sound
//! like they belong to the same culture, which is what makes a world feel
//! inhabited. The combinatorial space is still far larger than the number of
//! people this world will ever hold.
//!
//! A name is a given name plus a byname, because that is how the period actually
//! worked and because it is what lets two Aldrics coexist legibly:
//!
//! - **locational** — Aldric of Brackwater
//! - **occupational** — Aldric the Cooper
//! - **descriptive** — Aldric the Red
//! - **patronymic** — Aldric Osricson
//!
//! Bynames are weighted toward locational and occupational, which read as
//! belonging to a settled economy rather than to a saga.

use crate::rng::XorShift64;

/// Given names. Early-medieval English and Norse, chosen to be pronounceable and
/// visually distinct at a glance in a list.
const GIVEN: &[&str] = &[
    "Aldric", "Alwin", "Anselm", "Aubrey", "Bardolf", "Bertram", "Brand", "Bryn", "Cedric",
    "Cuthbert", "Dunstan", "Eadric", "Edmund", "Egil", "Elric", "Everard", "Faelan", "Frode",
    "Gareth", "Godwin", "Gunnar", "Hakon", "Halden", "Harald", "Hollis", "Ivar", "Ivo", "Jarl",
    "Jorunn", "Kettil", "Leofric", "Lucan", "Magnus", "Merek", "Odo", "Osric", "Oswin", "Rurik",
    "Sigurd", "Snorri", "Sverre", "Theobald", "Thoren", "Torvald", "Ulric", "Wilfred", "Wystan",
    "Alfreda", "Astrid", "Aud", "Beatrix", "Brenna", "Cassia", "Edith", "Eirwen", "Elgiva",
    "Freyda", "Gudrun", "Gwyneth", "Hilda", "Ingrid", "Isolde", "Linnea", "Maerwyn", "Mildred",
    "Rowena", "Sigrun", "Solveig", "Thyra", "Wynflaed", "Ysolde",
];

/// Place-name stems, combined with [`PLACE_TAIL`] into settlements and holdings.
/// Also used by locational bynames, so a person can be "of" somewhere that sounds
/// like it is on the same map as everywhere else.
const PLACE_HEAD: &[&str] = &[
    "Brack", "Ash", "Black", "Cold", "Elder", "Fern", "Grim", "Hart", "Haw", "Holl", "Marsh",
    "Mill", "Nether", "Oak", "Raven", "Red", "Rye", "Stone", "Thorn", "West", "Wind", "Wolf",
    "Yew", "Bram", "Clay", "Dun", "Fal", "Gil", "Har", "Kirk",
];

const PLACE_TAIL: &[&str] = &[
    "water", "mere", "ford", "wick", "combe", "dale", "hollow", "reach", "fell", "moor", "stead",
    "thorpe", "bury", "crag", "march", "hythe", "wold", "barrow", "gate", "haven",
];

/// Occupational bynames. Trades a settlement in this economy would actually have.
const TRADES: &[&str] = &[
    "Cooper",
    "Fletcher",
    "Smith",
    "Miller",
    "Mason",
    "Tanner",
    "Carter",
    "Shepherd",
    "Thatcher",
    "Wright",
    "Baker",
    "Brewer",
    "Chandler",
    "Dyer",
    "Fisher",
    "Forester",
    "Reeve",
    "Salter",
    "Turner",
    "Weaver",
    "Warden",
    "Ploughman",
];

/// Descriptive bynames.
const EPITHETS: &[&str] = &[
    "Red", "Bold", "Quiet", "Elder", "Younger", "Tall", "Grim", "Fair", "Swift", "Stern", "Lame",
    "Wanderer", "Silent", "Ready", "Cross", "Patient", "Lucky", "Grey",
];

/// Placeholder banners, until clans are real (ROADMAP Phase 8).
///
/// Named after places, because a clan in this world is named for its seat --
/// which is also why they are drawn from the same word tables as settlements.
/// This is a fixed roster on purpose: god mode cycles it to test the affiliation
/// plumbing before there is anything to found a clan with.
pub const BANNERS: &[&str] = &[
    "HOLLOWMERE",
    "BRACKWATER",
    "ASHFELL",
    "THORNDALE",
    "RAVENSTEAD",
    "COLDBARROW",
];

/// Display name for an affiliation index, or `None` for unaffiliated.
pub fn banner_name(index: Option<u8>) -> Option<&'static str> {
    index.and_then(|i| BANNERS.get(i as usize).copied())
}

/// One of `list`, chosen by `rng`. Panics only on an empty list, which would be a
/// build-time mistake in the tables above.
fn pick<'a>(rng: &mut XorShift64, list: &'a [&'a str]) -> &'a str {
    list[(rng.next_u64() % list.len() as u64) as usize]
}

/// A place name, e.g. "Brackwater", "Ashfell".
pub fn place_name(seed: u64) -> String {
    let mut rng = XorShift64::new(seed ^ 0x9E37_79B9_7F4A_7C15);
    format!(
        "{}{}",
        pick(&mut rng, PLACE_HEAD),
        pick(&mut rng, PLACE_TAIL)
    )
}

/// A full person name, e.g. "Sigrun of Brackwater", "Osric the Cooper".
///
/// Deterministic in `seed`.
pub fn person_name(seed: u64) -> String {
    // Offset the stream so a settlement and a person built from the same id do
    // not draw correlated words.
    let mut rng = XorShift64::new(seed ^ 0xD1B5_4A32_D192_ED03);
    let given = pick(&mut rng, GIVEN);

    // Weighted: a world of "the Bold" reads as a saga, a world of coopers and
    // places reads as somewhere people live and work.
    match rng.next_u64() % 100 {
        0..=37 => format!("{given} of {}", place_name(rng.next_u64())),
        38..=71 => format!("{given} the {}", pick(&mut rng, TRADES)),
        72..=89 => format!("{given} the {}", pick(&mut rng, EPITHETS)),
        _ => {
            // Patronymic. "-sson" would double the s on names already ending in
            // one, so the join is spelled rather than concatenated blindly.
            let parent = pick(&mut rng, GIVEN);
            let joined = if parent.ends_with('s') {
                format!("{parent}on")
            } else {
                format!("{parent}son")
            };
            format!("{given} {joined}")
        }
    }
}

/// Just the given name, for tight spaces like the selection plate.
pub fn short_name(full: &str) -> &str {
    full.split(' ').next().unwrap_or(full)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A name is identity. If it is not stable across processes and restarts,
    /// the encyclopedia is listing strangers every session.
    #[test]
    fn names_are_deterministic_in_the_seed() {
        for seed in [0u64, 1, 42, 9_999, u64::MAX] {
            assert_eq!(person_name(seed), person_name(seed));
            assert_eq!(place_name(seed), place_name(seed));
        }
    }

    /// Distinct seeds must mostly give distinct names, or a village reads as a
    /// family of clones.
    #[test]
    fn names_are_varied_across_seeds() {
        let names: HashSet<String> = (0..500).map(person_name).collect();
        assert!(
            names.len() > 440,
            "only {} distinct names from 500 seeds",
            names.len()
        );
        let places: HashSet<String> = (0..500).map(place_name).collect();
        assert!(
            places.len() > 300,
            "only {} distinct places from 500 seeds",
            places.len()
        );
    }

    /// The UI font has no glyphs beyond ASCII, so a non-ASCII name would render
    /// as tofu boxes in the encyclopedia and on the selection plate.
    #[test]
    fn names_are_ascii_and_reasonably_short() {
        for seed in 0..2000 {
            let name = person_name(seed);
            assert!(name.is_ascii(), "non-ASCII name: {name}");
            assert!(!name.is_empty(), "empty name at seed {seed}");
            assert!(name.len() <= 34, "name too long for a list row: {name}");
            assert!(!name.contains("  "), "double space in {name}");
            assert!(!name.ends_with(' '), "trailing space in {name}");
        }
    }

    /// The patronymic join must not produce "Osricsson".
    #[test]
    fn patronymics_do_not_double_the_s() {
        for seed in 0..5000 {
            let name = person_name(seed);
            assert!(!name.contains("sson"), "doubled s in {name}");
        }
    }

    #[test]
    fn short_name_takes_the_given_name() {
        assert_eq!(short_name("Sigrun of Brackwater"), "Sigrun");
        assert_eq!(short_name("Osric"), "Osric");
    }
}

#[cfg(test)]
mod sample {
    use super::*;

    /// Not an assertion — a way to eyeball the generator's voice.
    /// `cargo test -p shared show_sample_names -- --ignored --nocapture`
    #[test]
    #[ignore = "sample output; run with --ignored --nocapture"]
    fn show_sample_names() {
        for seed in 0..24 {
            println!("  {}", person_name(seed));
        }
        println!("  --- places ---");
        for seed in 0..10 {
            println!("  {}", place_name(seed));
        }
    }
}

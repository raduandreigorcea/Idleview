//! The one font catalogue.
//!
//! Font names, CSS stacks, and - critically - the weights each font actually ships
//! with used to be hand-copied across `main.js`, `App.vue`, and two `<link>` tags of
//! Google Fonts URLs. They drifted, as copies do:
//!
//!   * `google_sans` was offered in both apps and is not on Google Fonts at all, so
//!     choosing it rendered system sans.
//!   * `space_grotesk` was the *fallback* for the date font and was never loaded.
//!   * every font offered "thin" (weight 200), but Roboto was loaded at 100/300/400/
//!     500/700 - there is no 200 - so "thin" was silently synthesised or snapped.
//!   * Arimo has no weight below 400. Most script fonts have only 400.
//!
//! So the catalogue is defined here, once. The dashboard and the control panel both
//! read it (by Tauri command and by `GET /api/fonts`), and the Google Fonts URL is
//! generated from it. A font cannot be offered unless it is loaded, and a weight
//! cannot be offered unless the font has it.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Sans,
    Script,
}

#[derive(Debug, Clone, Serialize)]
pub struct FontDef {
    /// Stable id stored in settings, e.g. "roboto".
    pub id: &'static str,
    /// Human label for the picker.
    pub label: &'static str,
    /// The family name as Google Fonts knows it.
    pub family: &'static str,
    /// The full CSS stack, fallback included.
    pub stack: &'static str,
    pub category: Category,
    /// Weights this family genuinely offers. Anything else is a lie.
    pub weights: &'static [u16],
    /// Optical size correction. `font-size` sets the em-box, not the letters, and every
    /// family fills that box differently - so the same px value renders visibly larger in
    /// a tall face (Unbounded) than a short one (Italianno). This is `roboto_cap_height /
    /// this_cap_height`, so `size * size_scale` gives a matching cap-height across fonts.
    /// Roboto is the 1.000 reference. To regenerate if the catalogue changes: read each
    /// family's cap-height from its `OS/2` table (`sCapHeight`, falling back to the 'H'
    /// glyph bbox) over `head.unitsPerEm`, then divide Roboto's ratio by the font's.
    pub size_scale: f32,
}

/// Which fonts a given role may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Clock,
    Weekday,
    Date,
}

impl Role {
    pub fn from_str(role: &str) -> Option<Role> {
        match role {
            "clock" => Some(Role::Clock),
            "weekday" => Some(Role::Weekday),
            "date" => Some(Role::Date),
            _ => None,
        }
    }

    fn allows(&self, category: Category) -> bool {
        match self {
            // A 180px clock in a script face is unreadable across a room.
            Role::Clock => category == Category::Sans,
            Role::Weekday => category == Category::Script,
            Role::Date => true,
        }
    }

    pub fn default_font(&self) -> &'static str {
        match self {
            Role::Clock => "roboto",
            Role::Weekday => "great_vibes",
            Role::Date => "kaushan_script",
        }
    }

    pub fn default_weight(&self) -> u16 {
        match self {
            Role::Clock => 400,
            // The old defaults asked for weights these faces do not have: the weekday
            // default was "thin" (200) on Great Vibes and the date default "medium"
            // (500) on Kaushan Script, both of which publish only 400. Both were
            // silently synthesised by the browser.
            Role::Weekday => 400,
            Role::Date => 400,
        }
    }
}

// Weight lists below are what each family actually publishes on Google Fonts. Keep
// them honest: an entry here becomes an option in the picker and a weight in the
// stylesheet request.
pub const FONTS: &[FontDef] = &[
    // ----- Sans -----
    FontDef {
        id: "roboto",
        label: "Roboto",
        family: "Roboto",
        stack: "'Roboto', sans-serif",
        category: Category::Sans,
        weights: &[100, 300, 400, 500, 700, 900],
        size_scale: 1.000,
    },
    FontDef {
        id: "open_sans",
        label: "Open Sans",
        family: "Open Sans",
        stack: "'Open Sans', sans-serif",
        category: Category::Sans,
        weights: &[300, 400, 500, 600, 700, 800],
        size_scale: 0.996,
    },
    FontDef {
        id: "inter",
        label: "Inter",
        family: "Inter",
        stack: "'Inter', sans-serif",
        category: Category::Sans,
        weights: &[100, 200, 300, 400, 500, 600, 700, 800, 900],
        size_scale: 0.977,
    },
    FontDef {
        id: "montserrat",
        label: "Montserrat",
        family: "Montserrat",
        stack: "'Montserrat', sans-serif",
        category: Category::Sans,
        weights: &[100, 200, 300, 400, 500, 600, 700, 800, 900],
        size_scale: 1.016,
    },
    FontDef {
        id: "poppins",
        label: "Poppins",
        family: "Poppins",
        stack: "'Poppins', sans-serif",
        category: Category::Sans,
        weights: &[100, 200, 300, 400, 500, 600, 700, 800, 900],
        size_scale: 1.019,
    },
    FontDef {
        id: "lato",
        label: "Lato",
        family: "Lato",
        stack: "'Lato', sans-serif",
        category: Category::Sans,
        // No 500, no 600. "Medium" was never real for Lato.
        weights: &[100, 300, 400, 700, 900],
        size_scale: 0.992,
    },
    FontDef {
        id: "noto_sans_japanese",
        label: "Noto Sans Japanese",
        family: "Noto Sans JP",
        stack: "'Noto Sans JP', sans-serif",
        category: Category::Sans,
        weights: &[100, 200, 300, 400, 500, 600, 700, 800, 900],
        size_scale: 0.970,
    },
    FontDef {
        id: "arimo",
        label: "Arimo",
        family: "Arimo",
        stack: "'Arimo', sans-serif",
        category: Category::Sans,
        // Nothing below 400 exists. "Thin" used to be offered anyway.
        weights: &[400, 500, 600, 700],
        size_scale: 1.033,
    },
    FontDef {
        id: "roboto_condensed",
        label: "Roboto Condensed",
        family: "Roboto Condensed",
        stack: "'Roboto Condensed', sans-serif",
        category: Category::Sans,
        weights: &[100, 200, 300, 400, 500, 600, 700, 800, 900],
        size_scale: 1.000,
    },
    FontDef {
        id: "unbounded",
        label: "Unbounded",
        family: "Unbounded",
        stack: "'Unbounded', sans-serif",
        category: Category::Sans,
        weights: &[200, 300, 400, 500, 600, 700, 800, 900],
        size_scale: 0.948,
    },
    FontDef {
        id: "space_grotesk",
        label: "Space Grotesk",
        family: "Space Grotesk",
        stack: "'Space Grotesk', sans-serif",
        category: Category::Sans,
        // Was referenced as a fallback but never actually loaded. Now it is.
        weights: &[300, 400, 500, 600, 700],
        size_scale: 1.016,
    },
    // `google_sans` used to be offered here. It is a proprietary Google font, not
    // available through Google Fonts, so selecting it silently rendered system sans.
    // Removed rather than faked.

    // ----- Script -----
    // Most of these publish a single weight, which is exactly why the picker has to
    // ask the font what it supports instead of assuming six options.
    FontDef {
        id: "sacramento",
        label: "Sacramento",
        family: "Sacramento",
        stack: "'Sacramento', cursive",
        category: Category::Script,
        weights: &[400],
        size_scale: 0.939,
    },
    FontDef {
        id: "great_vibes",
        label: "Great Vibes",
        family: "Great Vibes",
        stack: "'Great Vibes', cursive",
        category: Category::Script,
        weights: &[400],
        size_scale: 0.948,
    },
    FontDef {
        id: "dancing_script",
        label: "Dancing Script",
        family: "Dancing Script",
        stack: "'Dancing Script', cursive",
        category: Category::Script,
        weights: &[400, 500, 600, 700],
        size_scale: 0.987,
    },
    FontDef {
        id: "pacifico",
        label: "Pacifico",
        family: "Pacifico",
        stack: "'Pacifico', cursive",
        category: Category::Script,
        weights: &[400],
        size_scale: 0.846,
    },
    FontDef {
        id: "satisfy",
        label: "Satisfy",
        family: "Satisfy",
        stack: "'Satisfy', cursive",
        category: Category::Script,
        weights: &[400],
        size_scale: 0.949,
    },
    FontDef {
        id: "pinyon_script",
        label: "Pinyon Script",
        family: "Pinyon Script",
        stack: "'Pinyon Script', cursive",
        category: Category::Script,
        weights: &[400],
        size_scale: 1.049,
    },
    FontDef {
        id: "alex_brush",
        label: "Alex Brush",
        family: "Alex Brush",
        stack: "'Alex Brush', cursive",
        category: Category::Script,
        weights: &[400],
        size_scale: 1.016,
    },
    FontDef {
        id: "kaushan_script",
        label: "Kaushan Script",
        family: "Kaushan Script",
        stack: "'Kaushan Script', cursive",
        category: Category::Script,
        weights: &[400],
        size_scale: 0.996,
    },
    FontDef {
        id: "italianno",
        label: "Italianno",
        family: "Italianno",
        stack: "'Italianno', cursive",
        category: Category::Script,
        weights: &[400],
        size_scale: 1.232,
    },
];

pub fn font(id: &str) -> Option<&'static FontDef> {
    FONTS.iter().find(|f| f.id == id)
}

/// The fonts a role may offer, in catalogue order.
pub fn fonts_for_role(role: Role) -> Vec<&'static FontDef> {
    FONTS.iter().filter(|f| role.allows(f.category)).collect()
}

/// Resolve a stored font id to a real font for this role, falling back rather than
/// rendering nothing when the id is unknown or not allowed here.
pub fn resolve_font(role: Role, id: &str) -> &'static FontDef {
    font(id)
        .filter(|f| role.allows(f.category))
        .or_else(|| font(role.default_font()))
        .unwrap_or(&FONTS[0])
}

/// Snap a requested weight to one the font actually has. Picks the nearest, breaking
/// ties downward, so asking Arimo for "thin" gives its lightest (400) instead of a
/// synthesised smear.
pub fn nearest_weight(font: &FontDef, requested: u16) -> u16 {
    font.weights
        .iter()
        .copied()
        .min_by_key(|w| (w.abs_diff(requested), *w))
        .unwrap_or(400)
}

/// The stylesheet request covering every font in the catalogue, at exactly the weights
/// it declares. Generated, so it cannot fall out of step with the pickers.
pub fn google_fonts_url() -> String {
    let families: Vec<String> = FONTS
        .iter()
        .map(|font| {
            let family = font.family.replace(' ', "+");
            if font.weights == [400] {
                format!("family={}", family)
            } else {
                let weights: Vec<String> = font.weights.iter().map(|w| w.to_string()).collect();
                format!("family={}:wght@{}", family, weights.join(";"))
            }
        })
        .collect();

    format!(
        "https://fonts.googleapis.com/css2?{}&display=swap",
        families.join("&")
    )
}

/// What the clients need to build a picker: the fonts per role, plus the stylesheet.
#[derive(Debug, Serialize)]
pub struct FontCatalogue {
    pub fonts: &'static [FontDef],
    pub clock: Vec<&'static str>,
    pub weekday: Vec<&'static str>,
    pub date: Vec<&'static str>,
    pub stylesheet: String,
    pub clock_size: (u16, u16),
    pub secondary_size: (u16, u16),
}

pub fn catalogue() -> FontCatalogue {
    let ids = |role: Role| fonts_for_role(role).into_iter().map(|f| f.id).collect();

    FontCatalogue {
        fonts: FONTS,
        clock: ids(Role::Clock),
        weekday: ids(Role::Weekday),
        date: ids(Role::Date),
        stylesheet: google_fonts_url(),
        clock_size: (
            crate::settings_manager::CLOCK_FONT_SIZE_MIN,
            crate::settings_manager::CLOCK_FONT_SIZE_MAX,
        ),
        secondary_size: (
            crate::settings_manager::SECONDARY_FONT_SIZE_MIN,
            crate::settings_manager::SECONDARY_FONT_SIZE_MAX,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_font_declares_at_least_one_weight() {
        for font in FONTS {
            assert!(!font.weights.is_empty(), "{} has no weights", font.id);
        }
    }

    #[test]
    fn font_ids_are_unique() {
        let mut ids: Vec<&str> = FONTS.iter().map(|f| f.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate font id in the catalogue");
    }

    #[test]
    fn the_phantom_fonts_are_gone() {
        // Both were offered in the UI and never loaded: google_sans is not on Google
        // Fonts at all, and space_grotesk was a fallback nobody fetched.
        assert!(font("google_sans").is_none());
        assert!(font("space_grotesk").is_some());
    }

    #[test]
    fn every_role_default_is_a_font_that_role_can_use() {
        for role in [Role::Clock, Role::Weekday, Role::Date] {
            let def = font(role.default_font()).expect("default font must exist");
            assert!(role.allows(def.category), "{:?} default is not allowed", role);
            // The default weight must also be one the default font really has.
            assert_eq!(nearest_weight(def, role.default_weight()), role.default_weight());
        }
    }

    #[test]
    fn clock_offers_only_sans_and_weekday_only_script() {
        assert!(fonts_for_role(Role::Clock).iter().all(|f| f.category == Category::Sans));
        assert!(fonts_for_role(Role::Weekday).iter().all(|f| f.category == Category::Script));
        // Date takes anything.
        assert_eq!(fonts_for_role(Role::Date).len(), FONTS.len());
    }

    #[test]
    fn an_unknown_or_disallowed_font_falls_back_instead_of_rendering_nothing() {
        assert_eq!(resolve_font(Role::Clock, "nonsense").id, "roboto");
        // A script font is not a legal clock face, so it falls back rather than being
        // honoured into an unreadable 180px wall of cursive.
        assert_eq!(resolve_font(Role::Clock, "italianno").id, "roboto");
        assert_eq!(resolve_font(Role::Clock, "inter").id, "inter");
    }

    #[test]
    fn weights_snap_to_something_the_font_actually_has() {
        let arimo = font("arimo").unwrap();
        // Arimo has nothing below 400; "thin" (200) used to be offered anyway.
        assert_eq!(nearest_weight(arimo, 200), 400);
        assert_eq!(nearest_weight(arimo, 600), 600);

        let lato = font("lato").unwrap();
        // Lato publishes no 500, so "medium" was never real for it.
        assert!(!lato.weights.contains(&500));
        assert_eq!(nearest_weight(lato, 500), 400);

        let sacramento = font("sacramento").unwrap();
        assert_eq!(nearest_weight(sacramento, 700), 400);
    }

    #[test]
    fn the_stylesheet_requests_every_font_and_only_real_weights() {
        let url = google_fonts_url();

        for font in FONTS {
            let family = font.family.replace(' ', "+");
            assert!(url.contains(&family), "{} missing from the stylesheet", font.id);
        }

        // Single-weight families are requested without a wght axis.
        assert!(url.contains("family=Sacramento&") || url.ends_with("family=Sacramento"));
        // Arimo must not be asked for a weight it does not have.
        assert!(url.contains("family=Arimo:wght@400;500;600;700"));
        assert!(!url.contains("Arimo:wght@100"));
    }
}

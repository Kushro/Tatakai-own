//! `Settings::look`: layout and style choices of the UI (2026-11 UI refresh), grouped in presets.
//!
//! Every choice is a small enum persisted by id. The shell writes the global ones as attributes on `<html>`
//! ([`LookPrefs::html_attrs`]: `data-density`, `data-rail`, `data-cards`, `data-card-size`, `data-motion`,
//! `data-effects`) so CSS
//! applies them without re-rendering; the screens read the rest (home layout, episode view, relations view,
//! calendar view, hero autoplay) from `Settings::look`.
//!
//! A preset ([`LookPreset`]) is a named bundle of every field. Picking one overwrites the fields; changing any
//! field afterwards leaves `preset` as it was and [`LookPrefs::matching_preset`] reports `None` ("Custom").

use serde::{Deserialize, Serialize};


/// Declares a persisted choice enum: `ALL`, `id()`, `from_id()`, the i18n key `look.<group>.<id>` and a lenient
/// serde form (the id as a string; an unknown id reads as the default instead of failing the whole settings file).
macro_rules! choice {
    ($(#[$m:meta])* $name:ident, $group:literal, { $($(#[$vm:meta])* $var:ident => $id:literal),+ $(,)? }, default = $def:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vm])* $var,)+
        }

        impl Default for $name {
            fn default() -> Self {
                Self::$def
            }
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$(Self::$var),+];

            pub fn id(self) -> &'static str {
                match self {
                    $(Self::$var => $id,)+
                }
            }

            pub fn from_id(id: &str) -> Option<Self> {
                match id {
                    $($id => Some(Self::$var),)+
                    _ => None,
                }
            }

            /// i18n key of the option label.
            pub fn label_key(self) -> String {
                format!("look.{}.{}", $group, self.id())
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.id())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(d)?;
                Ok(Self::from_id(&raw).unwrap_or_default())
            }
        }
    };
}

choice!(
    /// How Home opens.
    HomeLayout, "home", {
        /// Full-bleed banner of the trending titles (title, facts, synopsis, genres, Watch / list buttons, timed
        /// progress dots) with the rows below.
        Spotlight => "spotlight",
        /// The multi-browse carousel: one wide slide and narrow previews.
        Carousel => "carousel",
        /// No hero: a greeting and the rows, denser.
        Rows => "rows",
    },
    default = Spotlight
);

choice!(
    /// Poster cards in rows and grids.
    CardStyle, "cards", {
        /// Poster, title and the `2024 - 12 ep` line.
        Classic => "classic",
        /// Poster, a status dot before the title, year and format with icons under it.
        Detailed => "detailed",
        /// Poster only; the title shows on hover / focus.
        Minimal => "minimal",
    },
    default = Detailed
);

choice!(
    /// 0.3.2: size of the cards in rows and grids (`data-card-size`). `Small` / `Medium` are posters; `Large` / `Xl`
    /// are horizontal "detailed" cards (poster on the left; title, facts, score, genres, the episode line and the
    /// synopsis when known on the right), about 3 to 5 per row.
    CardSize, "card_size", {
        /// ~118 px posters.
        Small => "small",
        /// 150 px posters (the compact density still shrinks them).
        Medium => "medium",
        /// Horizontal detailed cards, ~300 px.
        Large => "large",
        /// Horizontal detailed cards, ~400 px.
        Xl => "xl",
    },
    default = Medium
);

impl CardSize {
    /// Horizontal detailed layout.
    pub fn is_detailed(self) -> bool {
        matches!(self, Self::Large | Self::Xl)
    }

    /// Nominal card width in px (media.css `--tk-card-w`; the grids use it as the column minimum).
    pub fn card_width_px(self) -> u32 {
        match self {
            Self::Small => 118,
            Self::Medium => 150,
            Self::Large => 300,
            Self::Xl => 400,
        }
    }

    /// Cards that fit side by side in a row `row_px` wide with `gap_px` between them (at least one).
    pub fn per_row(self, row_px: u32, gap_px: u32) -> u32 {
        ((row_px + gap_px) / (self.card_width_px() + gap_px)).max(1)
    }
}

choice!(
    /// Navigation rail.
    RailStyle, "rail", {
        /// 80 px with labels under the icons (M3).
        Labeled => "labeled",
        /// 64 px, icons only (labels as tooltips).
        Slim => "slim",
    },
    default = Labeled
);

choice!(
    /// Spacing and card sizes.
    Density, "density", {
        Comfortable => "comfortable",
        Compact => "compact",
    },
    default = Comfortable
);

choice!(
    /// Hover / press / enter animations. `Off` also sets `data-reduce-motion` (same as Settings > Reduce motion).
    Motion, "motion", {
        /// Lifts, shape morphs, staggered entrances.
        Expressive => "expressive",
        /// Color and opacity only.
        Subtle => "subtle",
        Off => "off",
    },
    default = Expressive
);

choice!(
    /// Episode list of Details.
    EpisodeView, "episodes", {
        /// Two columns of large cards: thumbnail with length and rating, title, synopsis, relative date.
        Cards => "cards",
        /// One row per episode with a small thumbnail.
        List => "list",
        /// Numbers and titles only.
        Compact => "compact",
    },
    default = Cards
);

choice!(
    /// Related titles of Details.
    RelationsView, "relations", {
        /// Pan / zoom graph of the franchise.
        Graph => "graph",
        /// Grouped list with covers.
        List => "list",
    },
    default = Graph
);

choice!(
    /// Default view of the airing calendar.
    CalendarView, "calendar", {
        /// Seven columns, one per day.
        Week => "week",
        /// One row per day with wide slots (the narrow-window layout).
        Agenda => "agenda",
        /// Month grid with poster stacks per day.
        Month => "month",
        /// Today as a time line with a "now" marker.
        Day => "day",
    },
    default = Week
);

choice!(
    /// A named bundle of every `LookPrefs` field.
    LookPreset, "preset", {
        /// The default: expressive Material 3 with the spotlight hero.
        Tatakai => "tatakai",
        /// Cinema: slim rail, spotlight hero, minimal posters, amoled-friendly.
        Cinema => "cinema",
        /// Dense: rows first, compact cards and lists.
        Dense => "dense",
        /// Calm: no autoplay, subtle motion, classic cards.
        Calm => "calm",
    },
    default = Tatakai
);

/// Hero autoplay period bounds (seconds).
pub const HERO_INTERVAL_MIN: u8 = 4;
pub const HERO_INTERVAL_MAX: u8 = 30;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LookPrefs {
    /// Last preset applied (shown as selected while the fields still match it).
    pub preset: LookPreset,
    pub home_layout: HomeLayout,
    pub card_style: CardStyle,
    /// 0.3.2.
    pub card_size: CardSize,
    pub rail_style: RailStyle,
    pub density: Density,
    pub motion: Motion,
    /// Blur / glass effects (backdrop-filter). Off by default: they cost a lot in WebKitGTK.
    pub glass: bool,
    pub episode_view: EpisodeView,
    pub relations_view: RelationsView,
    pub calendar_view: CalendarView,
    /// The Home hero rotates by itself.
    pub hero_autoplay: bool,
    /// Seconds per hero slide, [`HERO_INTERVAL_MIN`]..=[`HERO_INTERVAL_MAX`].
    pub hero_interval_secs: u8,
    /// The greeting ("Good evening") above Home.
    pub show_greeting: bool,
}

impl Default for LookPrefs {
    fn default() -> Self {
        Self::preset(LookPreset::Tatakai)
    }
}

impl LookPrefs {
    /// Every field of a preset.
    pub fn preset(p: LookPreset) -> Self {
        let base = Self {
            preset: p,
            home_layout: HomeLayout::Spotlight,
            card_style: CardStyle::Detailed,
            card_size: CardSize::Medium,
            rail_style: RailStyle::Labeled,
            density: Density::Comfortable,
            motion: Motion::Expressive,
            glass: false,
            episode_view: EpisodeView::Cards,
            relations_view: RelationsView::Graph,
            calendar_view: CalendarView::Week,
            hero_autoplay: true,
            hero_interval_secs: 9,
            show_greeting: false,
        };
        match p {
            LookPreset::Cinema => Self {
                card_style: CardStyle::Minimal,
                card_size: CardSize::Large,
                rail_style: RailStyle::Slim,
                glass: true,
                hero_interval_secs: 12,
                ..base
            },
            LookPreset::Dense => Self {
                home_layout: HomeLayout::Rows,
                card_style: CardStyle::Classic,
                card_size: CardSize::Small,
                rail_style: RailStyle::Slim,
                density: Density::Compact,
                motion: Motion::Subtle,
                episode_view: EpisodeView::List,
                relations_view: RelationsView::List,
                calendar_view: CalendarView::Agenda,
                show_greeting: true,
                ..base
            },
            LookPreset::Calm => Self {
                home_layout: HomeLayout::Carousel,
                card_style: CardStyle::Classic,
                motion: Motion::Subtle,
                hero_autoplay: false,
                episode_view: EpisodeView::List,
                calendar_view: CalendarView::Day,
                show_greeting: true,
                ..base
            },
            LookPreset::Tatakai => base,
        }
    }

    /// The preset whose fields all equal these (`preset` itself ignored); `None` = custom.
    pub fn matching_preset(&self) -> Option<LookPreset> {
        LookPreset::ALL.iter().copied().find(|p| {
            let mut x = Self::preset(*p);
            x.preset = self.preset;
            x == *self
        })
    }

    pub fn sanitized(self) -> Self {
        let mut s = self;
        s.hero_interval_secs = s.hero_interval_secs.clamp(HERO_INTERVAL_MIN, HERO_INTERVAL_MAX);
        s
    }

    /// Hero autoplay period in milliseconds, `None` when autoplay is off.
    pub fn hero_autoplay_ms(&self) -> Option<u64> {
        self.hero_autoplay
            .then(|| u64::from(self.hero_interval_secs.clamp(HERO_INTERVAL_MIN, HERO_INTERVAL_MAX)) * 1000)
    }

    /// Attributes the shell sets on `<html>` (CSS hooks in app.css / the area stylesheets).
    pub fn html_attrs(&self) -> [(&'static str, &'static str); 6] {
        [
            ("data-density", self.density.id()),
            ("data-rail", self.rail_style.id()),
            ("data-cards", self.card_style.id()),
            ("data-card-size", self.card_size.id()),
            ("data-motion", self.motion.id()),
            ("data-effects", if self.glass { "glass" } else { "flat" }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for v in HomeLayout::ALL {
            assert_eq!(HomeLayout::from_id(v.id()), Some(*v));
        }
        for v in CalendarView::ALL {
            assert_eq!(CalendarView::from_id(v.id()), Some(*v));
        }
        for v in LookPreset::ALL {
            assert_eq!(LookPreset::from_id(v.id()), Some(*v));
        }
        assert_eq!(EpisodeView::default().id(), "cards");
        assert_eq!(Motion::default().label_key(), "look.motion.expressive");
    }

    #[test]
    fn presets_are_recognised_until_a_field_changes() {
        for p in LookPreset::ALL {
            assert_eq!(LookPrefs::preset(*p).matching_preset(), Some(*p));
        }
        let mut s = LookPrefs::preset(LookPreset::Cinema);
        s.density = Density::Compact;
        assert_eq!(s.matching_preset(), None);
        assert_eq!(LookPrefs::default().matching_preset(), Some(LookPreset::Tatakai));
    }

    #[test]
    fn old_files_get_defaults_and_bad_values_are_repaired() {
        let s: LookPrefs =
            serde_json::from_str(r#"{"home_layout":"nope","card_style":"minimal","hero_interval_secs":200}"#).unwrap();
        // An unknown id reads as the default; the rest of the file survives.
        assert_eq!(s.home_layout, HomeLayout::Spotlight);
        assert_eq!(s.card_style, CardStyle::Minimal);
        let s = s.sanitized();
        assert_eq!(s.hero_interval_secs, HERO_INTERVAL_MAX);
        let empty: LookPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, LookPrefs::default());
        assert_eq!(empty.hero_autoplay_ms(), Some(9000));
    }

    #[test]
    fn html_attrs_follow_the_fields() {
        let s = LookPrefs::preset(LookPreset::Dense);
        let a = s.html_attrs();
        assert!(a.contains(&("data-density", "compact")));
        assert!(a.contains(&("data-rail", "slim")));
        assert!(a.contains(&("data-effects", "flat")));
        assert!(a.contains(&("data-card-size", "small")));
        assert!(LookPrefs::default().html_attrs().contains(&("data-card-size", "medium")));
    }

    #[test]
    fn card_sizes_follow_the_presets_and_read_leniently() {
        let size = |p| LookPrefs::preset(p).card_size;
        assert_eq!(size(LookPreset::Tatakai), CardSize::Medium);
        assert_eq!(size(LookPreset::Cinema), CardSize::Large);
        assert_eq!(size(LookPreset::Dense), CardSize::Small);
        assert_eq!(size(LookPreset::Calm), CardSize::Medium);
        let s: LookPrefs = serde_json::from_str(r#"{"card_size":"huge"}"#).unwrap();
        assert_eq!(s.card_size, CardSize::Medium);
        let s: LookPrefs = serde_json::from_str(r#"{"card_size":"xl"}"#).unwrap();
        assert_eq!(s.card_size, CardSize::Xl);
        assert_eq!(CardSize::Large.label_key(), "look.card_size.large");
        for v in CardSize::ALL {
            assert_eq!(CardSize::from_id(v.id()), Some(*v));
        }
    }

    #[test]
    fn card_size_math() {
        assert!(CardSize::Large.is_detailed() && CardSize::Xl.is_detailed() && !CardSize::Medium.is_detailed());
        // Rows of 1400 / 1700 px with 16 px gaps: 8 medium posters; 4-5 large and 3-4 xl cards ("3-5 per row").
        assert_eq!(CardSize::Medium.per_row(1400, 16), 8);
        assert_eq!(CardSize::Large.per_row(1400, 16), 4);
        assert_eq!(CardSize::Large.per_row(1700, 16), 5);
        assert_eq!(CardSize::Xl.per_row(1400, 16), 3);
        assert_eq!(CardSize::Xl.per_row(1700, 16), 4);
        assert_eq!(CardSize::Xl.per_row(300, 16), 1);
    }
}

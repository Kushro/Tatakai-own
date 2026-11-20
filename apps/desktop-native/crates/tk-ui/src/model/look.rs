//! `Settings::look`: layout and style choices of the UI (2026-11 UI refresh), grouped in presets.
//!
//! Every choice is a small enum persisted by id. The shell writes the global ones as attributes on `<html>`
//! ([`LookPrefs::html_attrs`]: `data-density`, `data-rail`, `data-cards`, `data-card-size`, `data-motion`,
//! `data-effects`, `data-style`) so CSS
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
        /// One row per episode with a small thumbnail (the default since look rev 1: the next episode fits on
        /// the first screen).
        List => "list",
        /// Numbers and titles only.
        Compact => "compact",
    },
    default = List
);

choice!(
    /// Page style of the Details header and the Home hero (`data-style`). Same fonts, palette and shapes; only the
    /// composition changes.
    PageStyle, "style", {
        /// Poster beside the title, blurred fanart behind, the sources panel at the side.
        Standard => "standard",
        /// "Cine editorial": sharp full-bleed fanart under a veil, no poster, large display title, metadata as one
        /// text line and the source as one line in the header.
        Editorial => "editorial",
    },
    default = Standard
);

choice!(
    /// Composition of the Details header (`data-dhead` on `.tk-dhead`, Details of anime only). The genres live in the
    /// Info tab in every composition.
    HeaderLayout, "header", {
        /// A: the facts as one quiet line; the main button (with the resume time and progress inside), the tools and
        /// the source as one block.
        Line => "line",
        /// B: labelled facts (status, next, episodes, year, score) and named secondary actions.
        Facts => "facts",
        /// C: the facts line and icon tools on the left; a card with the next episode, its progress, the main
        /// button and the source on the right.
        NextUp => "nextup",
        /// D: B's labelled facts and named actions plus C's episode card (the default).
        FactsNextUp => "facts_nextup",
    },
    default = FactsNextUp
);

impl HeaderLayout {
    /// The next-episode card holds the main button and the source line.
    pub fn has_card(self) -> bool {
        matches!(self, Self::NextUp | Self::FactsNextUp)
    }

    /// Labelled facts (`dl.tk-dhead__facts`) instead of the one-line metadata.
    pub fn uses_facts(self) -> bool {
        matches!(self, Self::Facts | Self::FactsNextUp)
    }
}

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

/// Revision of the stored look. A file without `rev` predates rev 1, when the default episode view went from Cards
/// to List: such a file with Cards is moved to List once (the old default; a later choice of Cards is kept).
pub const LOOK_REV: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, from = "LookPrefsWire")]
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
    /// Details header and Home hero composition (`data-style`).
    pub page_style: PageStyle,
    /// Composition of the Details header (`data-dhead`).
    pub header_layout: HeaderLayout,
    /// Always [`LOOK_REV`] once read (see the migration in `From<LookPrefsWire>`).
    pub rev: u8,
}

impl Default for LookPrefs {
    fn default() -> Self {
        Self::preset(LookPreset::Tatakai)
    }
}

/// What a settings file holds. `card_size` arrived in 0.3.2: a file written before has none, and then the size of
/// its preset applies (a "Cinema" user keeps reading as Cinema instead of turning "Custom"). `page_style` follows
/// the same rule; a missing `rev` triggers the one-time Cards → List move of the episode view.
#[derive(Deserialize)]
#[serde(default)]
struct LookPrefsWire {
    preset: LookPreset,
    home_layout: HomeLayout,
    card_style: CardStyle,
    card_size: Option<CardSize>,
    rail_style: RailStyle,
    density: Density,
    motion: Motion,
    glass: bool,
    episode_view: EpisodeView,
    relations_view: RelationsView,
    calendar_view: CalendarView,
    hero_autoplay: bool,
    hero_interval_secs: u8,
    show_greeting: bool,
    page_style: Option<PageStyle>,
    header_layout: Option<HeaderLayout>,
    rev: Option<u8>,
}

impl Default for LookPrefsWire {
    fn default() -> Self {
        let d = LookPrefs::default();
        Self {
            preset: d.preset,
            home_layout: d.home_layout,
            card_style: d.card_style,
            card_size: None,
            rail_style: d.rail_style,
            density: d.density,
            motion: d.motion,
            glass: d.glass,
            episode_view: d.episode_view,
            relations_view: d.relations_view,
            calendar_view: d.calendar_view,
            hero_autoplay: d.hero_autoplay,
            hero_interval_secs: d.hero_interval_secs,
            show_greeting: d.show_greeting,
            page_style: None,
            header_layout: None,
            rev: None,
        }
    }
}

impl From<LookPrefsWire> for LookPrefs {
    fn from(w: LookPrefsWire) -> Self {
        Self {
            preset: w.preset,
            home_layout: w.home_layout,
            card_style: w.card_style,
            card_size: w.card_size.unwrap_or_else(|| LookPrefs::preset(w.preset).card_size),
            rail_style: w.rail_style,
            density: w.density,
            motion: w.motion,
            glass: w.glass,
            episode_view: match (w.rev, w.episode_view) {
                (None, EpisodeView::Cards) => EpisodeView::List,
                (_, v) => v,
            },
            relations_view: w.relations_view,
            calendar_view: w.calendar_view,
            hero_autoplay: w.hero_autoplay,
            hero_interval_secs: w.hero_interval_secs,
            show_greeting: w.show_greeting,
            page_style: w.page_style.unwrap_or_else(|| LookPrefs::preset(w.preset).page_style),
            header_layout: w.header_layout.unwrap_or_else(|| LookPrefs::preset(w.preset).header_layout),
            rev: LOOK_REV,
        }
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
            episode_view: EpisodeView::List,
            relations_view: RelationsView::Graph,
            calendar_view: CalendarView::Week,
            hero_autoplay: true,
            hero_interval_secs: 9,
            show_greeting: false,
            page_style: PageStyle::Standard,
            header_layout: HeaderLayout::FactsNextUp,
            rev: LOOK_REV,
        };
        match p {
            LookPreset::Cinema => Self {
                card_style: CardStyle::Minimal,
                card_size: CardSize::Large,
                rail_style: RailStyle::Slim,
                glass: true,
                hero_interval_secs: 12,
                page_style: PageStyle::Editorial,
                ..base
            },
            LookPreset::Dense => Self {
                home_layout: HomeLayout::Rows,
                card_style: CardStyle::Classic,
                card_size: CardSize::Small,
                rail_style: RailStyle::Slim,
                density: Density::Compact,
                motion: Motion::Subtle,
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
                calendar_view: CalendarView::Day,
                show_greeting: true,
                ..base
            },
            LookPreset::Tatakai => base,
        }
    }

    /// The preset whose fields all equal these (`preset` itself ignored); `None` = custom. The Details header
    /// composition is not part of a preset: choosing one never turns the look "Custom" (as on Android).
    pub fn matching_preset(&self) -> Option<LookPreset> {
        LookPreset::ALL.iter().copied().find(|p| {
            let mut x = Self::preset(*p);
            x.preset = self.preset;
            x.header_layout = self.header_layout;
            x == *self
        })
    }

    /// Applies preset `p` and keeps the chosen Details header composition (not part of any preset).
    pub fn with_preset(&self, p: LookPreset) -> Self {
        Self { header_layout: self.header_layout, ..Self::preset(p) }
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
    pub fn html_attrs(&self) -> [(&'static str, &'static str); 7] {
        [
            ("data-density", self.density.id()),
            ("data-rail", self.rail_style.id()),
            ("data-cards", self.card_style.id()),
            ("data-card-size", self.card_size.id()),
            ("data-motion", self.motion.id()),
            ("data-effects", if self.glass { "glass" } else { "flat" }),
            ("data-style", self.page_style.id()),
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
        assert_eq!(EpisodeView::default().id(), "list");
        for v in PageStyle::ALL {
            assert_eq!(PageStyle::from_id(v.id()), Some(*v));
        }
        assert_eq!(PageStyle::Editorial.label_key(), "look.style.editorial");
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
        assert!(a.contains(&("data-style", "standard")));
        assert!(LookPrefs::preset(LookPreset::Cinema).html_attrs().contains(&("data-style", "editorial")));
    }

    #[test]
    fn every_preset_lists_episodes_and_only_cinema_is_editorial() {
        for p in LookPreset::ALL {
            let s = LookPrefs::preset(*p);
            assert_eq!(s.episode_view, EpisodeView::List, "{p:?}");
            assert_eq!(s.page_style == PageStyle::Editorial, *p == LookPreset::Cinema, "{p:?}");
            assert_eq!(s.rev, LOOK_REV);
        }
    }

    #[test]
    fn a_file_without_rev_moves_cards_to_list_once() {
        // Written before rev 1 with the old default (Cards): read as List, and the preset still matches.
        let mut old = serde_json::to_value(LookPrefs::preset(LookPreset::Tatakai)).unwrap();
        let o = old.as_object_mut().unwrap();
        o.remove("rev");
        o.remove("page_style");
        o.remove("header_layout");
        o.insert("episode_view".into(), "cards".into());
        let s: LookPrefs = serde_json::from_value(old).unwrap();
        assert_eq!(s.episode_view, EpisodeView::List);
        assert_eq!(s.rev, LOOK_REV);
        assert_eq!(s.matching_preset(), Some(LookPreset::Tatakai));
        // Compact is left alone.
        let s: LookPrefs = serde_json::from_str(r#"{"episode_view":"compact"}"#).unwrap();
        assert_eq!(s.episode_view, EpisodeView::Compact);
        // Cards chosen after the migration (the file carries rev) stays, also across a save / load round trip.
        let mut chosen = LookPrefs::default();
        chosen.episode_view = EpisodeView::Cards;
        let back: LookPrefs = serde_json::from_str(&serde_json::to_string(&chosen).unwrap()).unwrap();
        assert_eq!(back.episode_view, EpisodeView::Cards);
        assert_eq!(back, chosen);
    }

    #[test]
    fn a_cinema_file_from_before_page_styles_opens_editorial() {
        let mut old = serde_json::to_value(LookPrefs::preset(LookPreset::Cinema)).unwrap();
        old.as_object_mut().unwrap().remove("page_style");
        let s: LookPrefs = serde_json::from_value(old).unwrap();
        assert_eq!(s.page_style, PageStyle::Editorial);
        assert_eq!(s.matching_preset(), Some(LookPreset::Cinema));
        let s: LookPrefs = serde_json::from_str(r#"{"preset":"dense"}"#).unwrap();
        assert_eq!(s.page_style, PageStyle::Standard);
        // An explicit choice wins over the preset, and an unknown id reads as Standard.
        let s: LookPrefs = serde_json::from_str(r#"{"preset":"cinema","page_style":"standard"}"#).unwrap();
        assert_eq!(s.page_style, PageStyle::Standard);
        let s: LookPrefs = serde_json::from_str(r#"{"page_style":"brutal"}"#).unwrap();
        assert_eq!(s.page_style, PageStyle::Standard);
    }

    #[test]
    fn header_layouts_default_to_facts_and_card_and_migrate() {
        for v in HeaderLayout::ALL {
            assert_eq!(HeaderLayout::from_id(v.id()), Some(*v));
        }
        assert_eq!(HeaderLayout::default(), HeaderLayout::FactsNextUp);
        assert_eq!(HeaderLayout::default().label_key(), "look.header.facts_nextup");
        assert!(HeaderLayout::NextUp.has_card() && HeaderLayout::FactsNextUp.has_card());
        assert!(!HeaderLayout::Line.has_card() && !HeaderLayout::Facts.has_card());
        assert!(HeaderLayout::Facts.uses_facts() && !HeaderLayout::NextUp.uses_facts());
        // A file from before the setting opens in D, and every preset still matches.
        for p in LookPreset::ALL {
            let mut old = serde_json::to_value(LookPrefs::preset(*p)).unwrap();
            old.as_object_mut().unwrap().remove("header_layout");
            let s: LookPrefs = serde_json::from_value(old).unwrap();
            assert_eq!(s.header_layout, HeaderLayout::FactsNextUp, "{p:?}");
            assert_eq!(s.matching_preset(), Some(*p), "{p:?}");
        }
        let s: LookPrefs = serde_json::from_str(r#"{"header_layout":"stacked"}"#).unwrap();
        assert_eq!(s.header_layout, HeaderLayout::FactsNextUp);
        let s: LookPrefs = serde_json::from_str(r#"{"header_layout":"line"}"#).unwrap();
        assert_eq!(s.header_layout, HeaderLayout::Line);
        let back: LookPrefs = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.header_layout, HeaderLayout::Line);
    }

    #[test]
    fn the_header_layout_is_not_part_of_a_preset() {
        // Choosing a header keeps the preset recognised (never "Custom").
        for p in LookPreset::ALL {
            for h in HeaderLayout::ALL {
                let mut s = LookPrefs::preset(*p);
                s.header_layout = *h;
                assert_eq!(s.matching_preset(), Some(*p), "{p:?} {h:?}");
            }
        }
        // Applying a preset keeps the chosen header; the other fields follow the preset.
        let mut s = LookPrefs::preset(LookPreset::Dense);
        s.header_layout = HeaderLayout::NextUp;
        let next = s.with_preset(LookPreset::Cinema);
        assert_eq!(next.header_layout, HeaderLayout::NextUp);
        assert_eq!(next.page_style, PageStyle::Editorial);
        assert_eq!(next.matching_preset(), Some(LookPreset::Cinema));
        assert_eq!(LookPrefs::default().with_preset(LookPreset::Calm).header_layout, HeaderLayout::FactsNextUp);
    }

    #[test]
    fn a_file_from_before_card_sizes_keeps_its_preset() {
        let mut old = serde_json::to_value(LookPrefs::preset(LookPreset::Cinema)).unwrap();
        old.as_object_mut().unwrap().remove("card_size");
        let s: LookPrefs = serde_json::from_value(old).unwrap();
        assert_eq!(s.card_size, LookPrefs::preset(LookPreset::Cinema).card_size);
        assert_eq!(s.matching_preset(), Some(LookPreset::Cinema));
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

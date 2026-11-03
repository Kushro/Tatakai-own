//! Dioxus components (docs/redesign/01 section 6, docs/redesign/07 section 2). Plain functions of
//! props: no backend access (exception: `SourceSummary` loads its own sources, see `sources.rs`). The only
//! shared state is the global [`Snackbar`] from context.

pub mod button;
pub mod chips;
pub mod dialog;
pub mod floating_toolbar;
pub mod form;
pub mod loading;
pub mod media;
pub mod menu;
pub mod nav_rail;
pub mod player_controls;
pub mod progress;
pub mod segmented;
pub mod sheet;
pub mod snackbar;
pub mod sources;
pub mod state;
pub mod title_bar;

pub use button::{Button, ButtonSize, ButtonVariant, IconButton, IconButtonSize, IconButtonVariant, SplitButton};
pub use chips::{Banner, Chip, ChipKind, InfoChip, Kbd, Pill, Tone};
pub use dialog::ConfirmDialog;
pub use floating_toolbar::{FloatingToolbar, ToolbarSeparator};
pub use form::{Switch, TextField};
pub use loading::LoadingIndicator;
pub use media::{EpisodeRow, MediaCard, MediaCover};
pub use media::{ContinueCard, HeroCarousel, format_score, monogram, safe_css_color};
pub use menu::{Menu, MenuAlign, MenuItem, MenuPlacement, MenuSeparator};
pub use nav_rail::{NavRail, RailFootItem};
pub use player_controls::PlayerControls;
pub use progress::WavyProgress;
pub use segmented::{SegOption, Segmented};
pub use sheet::Sheet;
pub use snackbar::{Snackbar, SnackbarHost, use_snackbar};
pub use sources::{SourcePicker, SourceRow, SourceSummary};
pub use state::{EmptyState, ErrorState, SkeletonGrid, SkeletonLines, SkeletonRow};
pub use title_bar::TitleBar;

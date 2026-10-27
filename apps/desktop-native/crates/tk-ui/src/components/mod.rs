//! Dioxus components (docs/redesign/01 section 6). Plain functions of props: no backend access.

pub mod button;
pub mod chips;
pub mod dialog;
pub mod form;
pub mod loading;
pub mod media;
pub mod nav_rail;
pub mod player_controls;
pub mod progress;
pub mod state;
pub mod title_bar;

pub use button::{Button, ButtonSize, ButtonVariant, IconButton};
pub use chips::{InfoChip, Pill};
pub use dialog::ConfirmDialog;
pub use form::{Segmented, Switch, TextField};
pub use loading::LoadingIndicator;
pub use media::{EpisodeRow, MediaCard, MediaCover};
pub use nav_rail::NavRail;
pub use player_controls::PlayerControls;
pub use progress::WavyProgress;
pub use state::{EmptyState, ErrorState, SkeletonGrid, SkeletonRow};
pub use title_bar::TitleBar;

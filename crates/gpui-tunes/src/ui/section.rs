//! The sidebar's library sections.

/// Which column-browser arrangement is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Genres, Artists and Albums panes over the song table.
    Songs,
    /// Artists and Albums panes.
    Artists,
    /// Albums pane only.
    Albums,
    /// Genres, Artists and Albums panes.
    Genres,
}

impl Section {
    /// Sidebar order.
    pub const ALL: [Section; 4] = [
        Section::Songs,
        Section::Artists,
        Section::Albums,
        Section::Genres,
    ];

    /// Sidebar label.
    pub fn label(self) -> &'static str {
        match self {
            Section::Songs => "Songs",
            Section::Artists => "Artists",
            Section::Albums => "Albums",
            Section::Genres => "Genres",
        }
    }

    /// Whether the Genres pane is visible in this section.
    pub fn shows_genres(self) -> bool {
        matches!(self, Section::Songs | Section::Genres)
    }

    /// Whether the Artists pane is visible in this section.
    pub fn shows_artists(self) -> bool {
        !matches!(self, Section::Albums)
    }
}

//! Table delegate for the song list.

use std::sync::Arc;

use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme, Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
#[allow(clippy::wildcard_imports)] // gpui_kit re-exports the gpui prelude
use gpui_kit::*;
use tunes_core::{Library, Sort, SortColumn, TrackId};

use crate::ui::format::fmt_duration;

const COLUMNS: [(&str, &str, f32, SortColumn); 6] = [
    ("title", "Name", 320., SortColumn::Title),
    ("time", "Time", 70., SortColumn::Duration),
    ("artist", "Artist", 200., SortColumn::Artist),
    ("album", "Album", 220., SortColumn::Album),
    ("genre", "Genre", 140., SortColumn::Genre),
    ("track", "#", 50., SortColumn::TrackNo),
];

/// Rows are track ids; cells are looked up in the shared library.
pub struct SongTableDelegate {
    /// Library the ids refer to. `None` until the scan finishes.
    pub library: Option<Arc<Library>>,
    /// Visible rows in display order.
    pub rows: Vec<TrackId>,
    /// Track to mark with a play glyph.
    pub now_playing: Option<TrackId>,
    /// Sort currently applied to `rows`.
    pub sort: Sort,
    columns: Vec<Column>,
}

impl SongTableDelegate {
    /// Empty delegate with the fixed column set.
    pub fn new() -> Self {
        let columns = COLUMNS
            .iter()
            .map(|(key, name, width, _)| {
                let col = Column::new(*key, *name).width(px(*width)).sortable();
                if *key == "time" || *key == "track" {
                    col.text_right()
                } else {
                    col
                }
            })
            .collect();
        Self {
            library: None,
            rows: Vec::new(),
            now_playing: None,
            sort: Sort::default(),
            columns,
        }
    }

    /// Track id shown on row `ix`.
    pub fn track_at(&self, ix: usize) -> Option<TrackId> {
        self.rows.get(ix).copied()
    }

    fn cell(&self, row_ix: usize, col_ix: usize) -> SharedString {
        let Some((library, id)) = self.library.as_ref().zip(self.track_at(row_ix)) else {
            return SharedString::default();
        };
        let Some(track) = library.track(id) else {
            return SharedString::default();
        };
        match COLUMNS[col_ix].0 {
            "title" => track.title.clone().into(),
            "time" => fmt_duration(track.duration).into(),
            "artist" => track.artist.clone().into(),
            "album" => track.album.clone().into(),
            "genre" => track.genre.clone().into(),
            _ => track
                .track_no
                .map(|n| n.to_string())
                .unwrap_or_default()
                .into(),
        }
    }
}

impl Default for SongTableDelegate {
    fn default() -> Self {
        Self::new()
    }
}

impl TableDelegate for SongTableDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        self.sort = match sort {
            ColumnSort::Default => Sort::default(),
            ColumnSort::Ascending => Sort {
                column: COLUMNS[col_ix].3,
                ascending: true,
            },
            ColumnSort::Descending => Sort {
                column: COLUMNS[col_ix].3,
                ascending: false,
            },
        };
        if let Some(library) = &self.library {
            library.sort_ids(&mut self.rows, self.sort);
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let text = self.cell(row_ix, col_ix);
        let is_playing =
            col_ix == 0 && self.now_playing.is_some() && self.now_playing == self.track_at(row_ix);
        h_flex()
            .size_full()
            .items_center()
            .gap_1()
            .overflow_hidden()
            .when(is_playing, |el| {
                el.child(Icon::new(gpui_kit::assets::IconName::Play).text_color(cx.theme().primary))
            })
            .child(div().whitespace_nowrap().overflow_hidden().child(text))
    }
}

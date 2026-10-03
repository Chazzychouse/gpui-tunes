//! Classic column browser: Genres | Artists | Albums, each with an "All" row.

use std::sync::Arc;

use gpui_kit::component::list::{List, ListDelegate, ListEvent, ListItem, ListState};
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::{ActiveTheme, IndexPath, Selectable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
#[allow(clippy::wildcard_imports)] // gpui_kit re-exports the gpui prelude
use gpui_kit::*;
use tunes_core::{Filter, Library};

use crate::ui::section::Section;

/// Emitted whenever a pane selection changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserEvent {
    /// The genre/artist/album narrowing changed.
    FilterChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Genres,
    Artists,
    Albums,
}

impl Pane {
    fn title(self) -> &'static str {
        match self {
            Pane::Genres => "Genres",
            Pane::Artists => "Artists",
            Pane::Albums => "Albums",
        }
    }
}

/// One row in a pane.
#[derive(IntoElement)]
struct BrowserRow {
    base: ListItem,
    label: SharedString,
    selected: bool,
}

impl Selectable for BrowserRow {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self.base = self.base.selected(selected);
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for BrowserRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = if self.selected {
            cx.theme().accent_foreground
        } else {
            cx.theme().foreground
        };
        self.base.px_2().py_0p5().child(
            div()
                .text_sm()
                .text_color(color)
                .whitespace_nowrap()
                .overflow_hidden()
                .child(self.label),
        )
    }
}

/// Items of one pane. Row 0 is always "All (n)".
struct PaneDelegate {
    values: Vec<String>,
    selected: Option<usize>,
}

impl PaneDelegate {
    fn new() -> Self {
        Self {
            values: Vec::new(),
            selected: Some(0),
        }
    }

    /// The chosen value, or `None` when "All" is selected.
    fn chosen(&self) -> Option<&str> {
        match self.selected {
            Some(0) | None => None,
            Some(i) => self.values.get(i - 1).map(String::as_str),
        }
    }
}

impl ListDelegate for PaneDelegate {
    type Item = BrowserRow;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.values.len() + 1
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let label: SharedString = if ix.row == 0 {
            format!("All ({})", self.values.len()).into()
        } else {
            self.values.get(ix.row - 1)?.clone().into()
        };
        let selected = self.selected == Some(ix.row);
        Some(BrowserRow {
            base: ListItem::new(ix).selected(selected),
            label,
            selected,
        })
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix.map(|ix| ix.row);
        cx.notify();
    }
}

/// The three-pane browser view.
pub struct ColumnBrowser {
    library: Option<Arc<Library>>,
    section: Section,
    genres: Entity<ListState<PaneDelegate>>,
    artists: Entity<ListState<PaneDelegate>>,
    albums: Entity<ListState<PaneDelegate>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<BrowserEvent> for ColumnBrowser {}

impl ColumnBrowser {
    /// Empty browser; call [`Self::set_library`] once the scan finishes.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let make = |window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| ListState::new(PaneDelegate::new(), window, cx))
        };
        let genres = make(window, cx);
        let artists = make(window, cx);
        let albums = make(window, cx);

        let subscriptions = vec![
            cx.subscribe_in(&genres, window, |this, _, ev, window, cx| {
                if matches!(ev, ListEvent::Select(_) | ListEvent::Confirm(_)) {
                    this.on_pane_changed(Pane::Genres, window, cx);
                }
            }),
            cx.subscribe_in(&artists, window, |this, _, ev, window, cx| {
                if matches!(ev, ListEvent::Select(_) | ListEvent::Confirm(_)) {
                    this.on_pane_changed(Pane::Artists, window, cx);
                }
            }),
            cx.subscribe_in(&albums, window, |this, _, ev, window, cx| {
                if matches!(ev, ListEvent::Select(_) | ListEvent::Confirm(_)) {
                    this.on_pane_changed(Pane::Albums, window, cx);
                }
            }),
        ];

        Self {
            library: None,
            section: Section::Songs,
            genres,
            artists,
            albums,
            _subscriptions: subscriptions,
        }
    }

    /// Install (or clear) the library and reset every pane to "All".
    pub fn set_library(
        &mut self,
        library: Option<Arc<Library>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.library = library;
        self.reset_from(Pane::Genres, window, cx);
    }

    /// Switch section. Clears pane selections.
    pub fn set_section(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        self.section = section;
        self.reset_from(Pane::Genres, window, cx);
    }

    /// The narrowing the panes currently express (search is left empty).
    pub fn filter(&self, cx: &App) -> Filter {
        let pick = |list: &Entity<ListState<PaneDelegate>>| {
            list.read(cx).delegate().chosen().map(str::to_owned)
        };
        Filter {
            genre: if self.section.shows_genres() {
                pick(&self.genres)
            } else {
                None
            },
            artist: if self.section.shows_artists() {
                pick(&self.artists)
            } else {
                None
            },
            album: pick(&self.albums),
            search: String::new(),
        }
    }

    fn on_pane_changed(&mut self, pane: Pane, window: &mut Window, cx: &mut Context<Self>) {
        match pane {
            Pane::Genres => self.reset_from(Pane::Artists, window, cx),
            Pane::Artists => self.reset_from(Pane::Albums, window, cx),
            Pane::Albums => {}
        }
        cx.emit(BrowserEvent::FilterChanged);
        cx.notify();
    }

    /// Recompute the values of `from` and every pane to its right, selecting "All".
    fn reset_from(&mut self, from: Pane, window: &mut Window, cx: &mut Context<Self>) {
        let Some(library) = self.library.clone() else {
            for list in [&self.genres, &self.artists, &self.albums] {
                Self::fill(list, Vec::new(), window, cx);
            }
            return;
        };

        if from == Pane::Genres {
            Self::fill(&self.genres, library.genres().to_vec(), window, cx);
        }
        let genre = self
            .section
            .shows_genres()
            .then(|| self.genres.read(cx).delegate().chosen().map(str::to_owned))
            .flatten();
        if matches!(from, Pane::Genres | Pane::Artists) {
            Self::fill(
                &self.artists,
                library.artists_in(genre.as_deref()),
                window,
                cx,
            );
        }
        let artist = self
            .section
            .shows_artists()
            .then(|| self.artists.read(cx).delegate().chosen().map(str::to_owned))
            .flatten();
        Self::fill(
            &self.albums,
            library.albums_in(genre.as_deref(), artist.as_deref()),
            window,
            cx,
        );
    }

    fn fill(
        list: &Entity<ListState<PaneDelegate>>,
        values: Vec<String>,
        window: &mut Window,
        cx: &mut App,
    ) {
        list.update(cx, |state, cx| {
            state.delegate_mut().values = values;
            state.delegate_mut().selected = Some(0);
            state.set_selected_index(Some(IndexPath::new(0)), window, cx);
            cx.notify();
        });
    }

    fn pane(
        pane: Pane,
        list: &Entity<ListState<PaneDelegate>>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .size_full()
            .min_w_0()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().muted_foreground)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(pane.title()),
            )
            .child(List::new(list).flex_1())
    }
}

impl Render for ColumnBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.section;
        h_flex()
            .size_full()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_resizable("column-browser")
                    .when(section.shows_genres(), |group| {
                        group.child(resizable_panel().child(Self::pane(
                            Pane::Genres,
                            &self.genres,
                            cx,
                        )))
                    })
                    .when(section.shows_artists(), |group| {
                        group.child(resizable_panel().child(Self::pane(
                            Pane::Artists,
                            &self.artists,
                            cx,
                        )))
                    })
                    .child(resizable_panel().child(Self::pane(Pane::Albums, &self.albums, cx))),
            )
    }
}

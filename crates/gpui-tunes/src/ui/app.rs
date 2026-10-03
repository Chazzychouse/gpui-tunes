//! Root view: owns the core objects and wires the child views together.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
#[allow(clippy::wildcard_imports)] // gpui_kit re-exports the gpui prelude
use gpui_kit::*;
use tunes_core::{Filter, Library, Player, PlayerError, ScanError, Sort, scan};

use crate::ui::browser::{BrowserEvent, ColumnBrowser};
use crate::ui::sidebar::{LibrarySidebar, SidebarEvent};

enum Phase {
    Scanning,
    Missing(PathBuf),
    Ready(Arc<Library>),
}

/// The application window's root view.
pub struct AppView {
    root: PathBuf,
    player: Result<Player, PlayerError>,
    phase: Phase,
    filter: Filter,
    sidebar: Entity<LibrarySidebar>,
    browser: Entity<ColumnBrowser>,
    _subscriptions: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl AppView {
    /// Build the view and kick off the background scan of `root`.
    pub fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let player = Player::new();
        if let Err(err) = &player {
            tracing::error!(%err, "audio output unavailable");
        }

        let sidebar = cx.new(LibrarySidebar::new);
        let browser = cx.new(|cx| ColumnBrowser::new(window, cx));
        let subscriptions = vec![
            cx.subscribe_in(
                &sidebar,
                window,
                |this, _, ev: &SidebarEvent, window, cx| {
                    let SidebarEvent::Select(section) = *ev;
                    this.browser
                        .update(cx, |b, cx| b.set_section(section, window, cx));
                    this.refresh_filter(window, cx);
                },
            ),
            cx.subscribe_in(
                &browser,
                window,
                |this, _, ev: &BrowserEvent, window, cx| {
                    let BrowserEvent::FilterChanged = ev;
                    this.refresh_filter(window, cx);
                },
            ),
        ];

        let scan_root = root.clone();
        let scan_task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { scan(&scan_root) })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.phase = match result {
                    Ok(tracks) => Phase::Ready(Arc::new(Library::new(tracks))),
                    Err(ScanError::RootMissing(path)) => Phase::Missing(path),
                };
                let library = match &this.phase {
                    Phase::Ready(l) => Some(Arc::clone(l)),
                    _ => None,
                };
                this.browser
                    .update(cx, |b, cx| b.set_library(library, window, cx));
                this.refresh_filter(window, cx);
            })
            .ok();
        });

        Self {
            root,
            player,
            phase: Phase::Scanning,
            filter: Filter::default(),
            sidebar,
            browser,
            _subscriptions: subscriptions,
            _tasks: vec![scan_task],
        }
    }

    fn refresh_filter(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let search = std::mem::take(&mut self.filter.search);
        self.filter = self.browser.read(cx).filter(cx);
        self.filter.search = search;
        cx.notify();
    }

    fn render_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        match &self.phase {
            Phase::Scanning => h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(muted)
                .child(Spinner::new())
                .child(format!("Scanning {}", self.root.display()))
                .into_any_element(),
            Phase::Missing(path) => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(muted)
                .child("No library folder found")
                .child(format!("Looked in {}", path.display()))
                .child("Set GPUI_TUNES_LIBRARY to point at your music")
                .into_any_element(),
            Phase::Ready(library) => {
                let count = library.query(&self.filter, Sort::default()).len();
                h_resizable("main")
                    .child(
                        resizable_panel()
                            .size(px(180.))
                            .size_range(px(140.)..px(320.))
                            .child(self.sidebar.clone()),
                    )
                    .child(
                        resizable_panel().child(
                            v_resizable("content")
                                .child(
                                    resizable_panel()
                                        .size(px(200.))
                                        .size_range(px(120.)..px(400.))
                                        .child(self.browser.clone()),
                                )
                                .child(
                                    resizable_panel()
                                        .child(div().p_4().child(format!("{count} tracks match"))),
                                ),
                        ),
                    )
                    .into_any_element()
            }
        }
    }
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.render_content(cx);
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(content)
    }
}

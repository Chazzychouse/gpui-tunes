//! Root view: owns the core objects and wires the child views together.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
#[allow(clippy::wildcard_imports)] // gpui_kit re-exports the gpui prelude
use gpui_kit::*;
use tunes_core::{
    Event, Filter, Library, PlaybackState, PlaybackStatus, Player, PlayerError, Queue, ScanError,
    TrackId, scan,
};

use crate::ui::art;
use crate::ui::browser::{BrowserEvent, ColumnBrowser};
use crate::ui::format::fmt_status;
use crate::ui::sidebar::{LibrarySidebar, SidebarEvent};
use crate::ui::song_table::SongTableDelegate;
use crate::ui::toolbar::{Toolbar, ToolbarEvent, ToolbarModel};

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
    table: Entity<TableState<SongTableDelegate>>,
    queue: Queue,
    toolbar: Entity<Toolbar>,
    status: PlaybackStatus,
    now_playing: Option<TrackId>,
    art_cache: Option<(TrackId, Option<Arc<Image>>)>,
    notice: Option<(String, Instant)>,
    /// Set after a decode error auto-advanced once; cleared by every
    /// `play_track`, so a run of broken files stops after one skip.
    auto_skipped: bool,
    status_line: String,
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
        let table = cx.new(|cx| TableState::new(SongTableDelegate::new(), window, cx));
        let toolbar = cx.new(|cx| Toolbar::new(window, cx));
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
            cx.subscribe_in(&table, window, |this, _, ev: &TableEvent, window, cx| {
                if let TableEvent::DoubleClickedRow(row) = ev {
                    let id = this.table.read(cx).delegate().track_at(*row);
                    if let Some(id) = id {
                        this.play_track(id, window, cx);
                    }
                }
            }),
            // Header sorts reorder `rows` inside the table; mirror that order
            // into the queue so next/previous follow what the user sees.
            cx.observe(&table, |this, table, cx| {
                let rows = &table.read(cx).delegate().rows;
                if rows.as_slice() != this.queue.order() {
                    this.queue.set_order(rows.clone());
                }
            }),
            cx.subscribe_in(&toolbar, window, |this, _, ev, window, cx| {
                this.on_toolbar_event(ev, window, cx);
            }),
        ];

        let poll_task = Self::spawn_poll(window, cx);

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
                    .update(cx, |b, cx| b.set_library(library.clone(), window, cx));
                this.table
                    .update(cx, |t, _| t.delegate_mut().library = library);
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
            table,
            queue: Queue::new(),
            toolbar,
            status: PlaybackStatus::default(),
            now_playing: None,
            art_cache: None,
            notice: None,
            auto_skipped: false,
            status_line: String::new(),
            _subscriptions: subscriptions,
            _tasks: vec![scan_task, poll_task],
        }
    }

    fn refresh_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = std::mem::take(&mut self.filter.search);
        self.filter = self.browser.read(cx).filter(cx);
        self.filter.search = search;
        self.apply_query(window, cx);
    }

    /// Re-run the library query and push the rows into the table and queue.
    fn apply_query(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Phase::Ready(library) = &self.phase else {
            return;
        };
        let sort = self.table.read(cx).delegate().sort;
        let ids = library.query(&self.filter, sort);
        let total: Duration = ids
            .iter()
            .filter_map(|id| library.track(*id))
            .map(|t| t.duration)
            .sum();
        self.status_line = fmt_status(ids.len(), total);
        self.queue.set_order(ids.clone());
        self.table.update(cx, |table, cx| {
            table.delegate_mut().rows = ids;
            // Every caller of `apply_query` changed the filter, so a kept
            // highlight would point at a different track.
            table.clear_selection(cx);
            cx.notify();
        });
        cx.notify();
    }

    /// Start `id` playing, point the queue at it and mark its row.
    fn play_track(&mut self, id: TrackId, _window: &mut Window, cx: &mut Context<Self>) {
        let Phase::Ready(library) = &self.phase else {
            return;
        };
        let Some(track) = library.track(id) else {
            return;
        };
        let Ok(player) = &self.player else {
            return;
        };
        player.play(&track.path);
        // Optimistic: the status snapshot lags up to one tick, and a
        // play/pause press in between must toggle, not restart.
        self.status.state = PlaybackState::Playing;
        // `id` always comes from `rows`, which the table observer keeps in sync
        // with the queue, so it is always present.
        self.queue.jump_to(id);
        self.now_playing = Some(id);
        self.auto_skipped = false;
        self.table.update(cx, |table, cx| {
            table.delegate_mut().now_playing = Some(id);
            cx.notify();
        });
        cx.notify();
    }

    fn on_toolbar_event(&mut self, ev: &ToolbarEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            ToolbarEvent::Prev => self.step(false, window, cx),
            ToolbarEvent::Next => self.step(true, window, cx),
            ToolbarEvent::PlayPause => self.play_pause(window, cx),
            ToolbarEvent::Seek(pos) => {
                if let Ok(p) = &self.player {
                    p.seek(*pos);
                }
            }
            ToolbarEvent::Volume(v) => {
                if let Ok(p) = &self.player {
                    p.set_volume(*v);
                }
            }
            ToolbarEvent::Search(text) => {
                self.filter.search.clone_from(text);
                self.apply_query(window, cx);
            }
        }
    }

    /// Every 250 ms, run `tick` until the view is dropped.
    fn spawn_poll(window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this.update_in(cx, Self::tick).is_err() {
                    break;
                }
            }
        })
    }

    /// Poll the player: refresh status, react to events, expire the notice.
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(player) = &self.player else {
            // Keep pushing so the toolbar shows the "No audio output" notice.
            self.push_toolbar_model(window, cx);
            return;
        };
        // Drain first, then read status: the audio thread publishes status
        // before sending events, so a fresh read after draining is
        // authoritative. If the user started another track after a queued
        // event was sent, that newer Play makes the state non-Stopped and the
        // stale event is ignored instead of skipping the user's choice.
        let mut events = Vec::new();
        while let Some(event) = player.try_recv_event() {
            events.push(event);
        }
        self.status = player.status();
        for event in events {
            match event {
                Event::TrackEnded => {
                    if self.status.state == PlaybackState::Stopped {
                        self.step(true, window, cx);
                    }
                }
                Event::Error(message) => {
                    self.notice = Some((message, Instant::now()));
                    // Skip past a broken file once; `play_track` clears the
                    // flag, so re-arm it after stepping.
                    // Non-fatal errors (seek) leave playback running: don't skip.
                    if !self.auto_skipped && self.status.state == PlaybackState::Stopped {
                        self.step(true, window, cx);
                        self.auto_skipped = true;
                    }
                }
            }
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(5))
        {
            self.notice = None;
        }
        self.push_toolbar_model(window, cx);
    }

    fn step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let next = if forward {
            self.queue.next()
        } else {
            self.queue.prev()
        };
        if let Some(id) = next {
            self.play_track(id, window, cx);
        }
    }

    fn play_pause(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.status.state == PlaybackState::Stopped {
            let id = self.queue.current().or_else(|| self.queue.next());
            if let Some(id) = id {
                self.play_track(id, window, cx);
            }
        } else if let Ok(p) = &self.player {
            p.toggle();
        }
    }

    fn push_toolbar_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut model = ToolbarModel {
            position: self.status.position,
            duration: self.status.duration,
            state: self.status.state,
            enabled: self.player.is_ok(),
            notice: self.notice.as_ref().map(|(m, _)| m.clone()),
            ..ToolbarModel::default()
        };
        if self.player.is_err() {
            model.notice = Some("No audio output".into());
        }
        if let (Phase::Ready(library), Some(id)) = (&self.phase, self.now_playing) {
            if let Some(track) = library.track(id) {
                model.title.clone_from(&track.title);
                model.artist = format!("{} — {}", track.artist, track.album);
            }
            if self
                .art_cache
                .as_ref()
                .is_none_or(|(cached, _)| *cached != id)
            {
                let img = library.cover_art(id).map(|bytes| art::decode(&bytes));
                self.art_cache = Some((id, img));
            }
            model.art = self.art_cache.as_ref().and_then(|(_, img)| img.clone());
        }
        self.toolbar
            .update(cx, |t, cx| t.set_model(model, window, cx));
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
            Phase::Ready(_) => h_resizable("main")
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
                                resizable_panel().child(DataTable::new(&self.table).stripe(true)),
                            ),
                    ),
                )
                .into_any_element(),
        }
    }
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.render_content(cx);
        let ready = matches!(self.phase, Phase::Ready(_));
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.toolbar.clone())
            .child(div().flex_1().min_h_0().child(content))
            .when(ready, |el| {
                el.child(
                    h_flex()
                        .w_full()
                        .h(px(24.))
                        .flex_shrink_0()
                        .justify_center()
                        .items_center()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .bg(cx.theme().title_bar)
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(self.status_line.clone()),
                )
            })
    }
}

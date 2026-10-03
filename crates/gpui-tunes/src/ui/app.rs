//! Root view: owns the core objects and wires the child views together.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
#[allow(clippy::wildcard_imports)] // gpui_kit re-exports the gpui prelude
use gpui_kit::*;
use tunes_core::{Library, Player, PlayerError, ScanError, scan};

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
    _tasks: Vec<Task<()>>,
}

impl AppView {
    /// Build the view and kick off the background scan of `root`.
    pub fn new(root: PathBuf, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let player = Player::new();
        if let Err(err) = &player {
            tracing::error!(%err, "audio output unavailable");
        }

        let scan_root = root.clone();
        let scan_task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { scan(&scan_root) })
                .await;
            this.update(cx, |this, cx| {
                this.phase = match result {
                    Ok(tracks) => Phase::Ready(Arc::new(Library::new(tracks))),
                    Err(ScanError::RootMissing(path)) => Phase::Missing(path),
                };
                cx.notify();
            })
            .ok();
        });

        Self {
            root,
            player,
            phase: Phase::Scanning,
            _tasks: vec![scan_task],
        }
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
            Phase::Ready(library) => div()
                .p_4()
                .child(format!("{} tracks", library.len()))
                .into_any_element(),
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

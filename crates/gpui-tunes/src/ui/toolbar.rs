//! Top bar: transport buttons, now-playing display with seek, volume, search.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{
    ActiveTheme, Disableable as _, Icon, Selectable as _, Sizable as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
#[allow(clippy::wildcard_imports)] // gpui_kit re-exports the gpui prelude
use gpui_kit::*;
use tunes_core::PlaybackState;

use crate::ui::format::fmt_duration;

/// What the toolbar shows. Built by `AppView` from core state.
#[derive(Debug, Clone, Default)]
pub struct ToolbarModel {
    /// Current track title, empty when nothing is loaded.
    pub title: String,
    /// Artist and album line shown under the title.
    pub artist: String,
    /// Elapsed time.
    pub position: Duration,
    /// Track length when known.
    pub duration: Option<Duration>,
    /// Playing, paused or stopped.
    pub state: PlaybackState,
    /// Cover art for the current track.
    pub art: Option<Arc<Image>>,
    /// `false` when no audio device is available.
    pub enabled: bool,
    /// Transient error or status line.
    pub notice: Option<String>,
}

/// User intents from the toolbar.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolbarEvent {
    /// Previous track.
    Prev,
    /// Toggle play/pause.
    PlayPause,
    /// Next track.
    Next,
    /// Seek to an absolute position.
    Seek(Duration),
    /// Volume 0.0..=1.0.
    Volume(f32),
    /// Search text changed.
    Search(String),
    /// Collapse or expand the sidebar.
    ToggleSidebar,
    /// Escape was pressed in the search field; give focus back to the app.
    SearchEscape,
}

/// The toolbar view.
pub struct Toolbar {
    model: ToolbarModel,
    seek: Entity<SliderState>,
    volume: Entity<SliderState>,
    search: Entity<InputState>,
    seeking: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ToolbarEvent> for Toolbar {}

impl Toolbar {
    /// Move keyboard focus to the search field.
    pub fn focus_search(&self, window: &mut Window, cx: &mut App) {
        self.search.read(cx).focus_handle(cx).focus(window, cx);
    }

    /// Whether the search field has keyboard focus.
    pub fn search_focused(&self, window: &Window, cx: &App) -> bool {
        self.search.read(cx).focus_handle(cx).is_focused(window)
    }

    /// Toolbar with empty now-playing state.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let seek = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(1.)
                .step(0.001)
                .default_value(0.)
        });
        let volume = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(1.)
                .step(0.01)
                .default_value(1.)
        });
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search")
                .clean_on_escape()
        });

        let subscriptions = vec![
            cx.subscribe(&seek, |this, _, ev: &SliderEvent, cx| match ev {
                SliderEvent::Change(_) => this.seeking = true,
                SliderEvent::Release(value) => {
                    this.seeking = false;
                    if let Some(duration) = this.model.duration {
                        cx.emit(ToolbarEvent::Seek(
                            duration.mul_f32(value.start().clamp(0., 1.)),
                        ));
                    }
                }
            }),
            cx.subscribe(&volume, |_, _, ev: &SliderEvent, cx| {
                let (SliderEvent::Change(value) | SliderEvent::Release(value)) = ev;
                cx.emit(ToolbarEvent::Volume(value.start()));
            }),
            cx.subscribe(&search, |_, state, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    cx.emit(ToolbarEvent::Search(text));
                }
            }),
        ];

        Self {
            model: ToolbarModel::default(),
            seek,
            volume,
            search,
            seeking: false,
            _subscriptions: subscriptions,
        }
    }

    /// Push fresh state in. The seek slider is left alone while being dragged.
    pub fn set_model(&mut self, model: ToolbarModel, window: &mut Window, cx: &mut Context<Self>) {
        if !self.seeking {
            let fraction = match model.duration {
                Some(d) if !d.is_zero() => {
                    (model.position.as_secs_f32() / d.as_secs_f32()).clamp(0., 1.)
                }
                _ => 0.,
            };
            self.seek
                .update(cx, |s, cx| s.set_value(fraction, window, cx));
        }
        self.model = model;
        cx.notify();
    }

    fn transport_button(
        &self,
        id: &'static str,
        icon: IconName,
        event: ToolbarEvent,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .ghost()
            .icon(Icon::new(icon))
            .disabled(!self.model.enabled)
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(event.clone())))
    }

    /// A toggle that works without an audio device, unlike the transport buttons.
    #[allow(clippy::unused_self)] // keeps the method shape shared with `transport_button`
    fn chrome_button(
        &self,
        id: &'static str,
        icon: IconName,
        event: ToolbarEvent,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .ghost()
            .icon(Icon::new(icon))
            .selected(selected)
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(event.clone())))
    }

    fn now_playing(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let has_track = !self.model.title.is_empty();
        let total = self
            .model
            .duration
            .map_or_else(|| "--:--".into(), fmt_duration);
        // One secondary line under the seek row: a notice replaces the
        // artist/album line so the panel never grows a fourth row.
        let second_line = match &self.model.notice {
            Some(notice) => Some((notice.clone(), cx.theme().danger)),
            None if has_track => Some((self.model.artist.clone(), muted)),
            None => None,
        };

        h_flex()
            .flex_1()
            // Keep room for title + seek bar; the search box gives way first.
            .min_w(px(220.))
            .min_h(px(52.))
            .py_1()
            .gap_3()
            .px_3()
            .rounded(cx.theme().radius)
            .bg(cx.theme().secondary)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .size(px(40.))
                    .flex_shrink_0()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .overflow_hidden()
                    .when_some(self.model.art.clone(), |el, art| {
                        el.child(img(art).size_full())
                    }),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .justify_center()
                    .child(
                        div()
                            .text_sm()
                            .line_height(relative(1.2))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(if has_track {
                                self.model.title.clone()
                            } else {
                                "Not Playing".into()
                            }),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .text_xs()
                            .line_height(relative(1.2))
                            .text_color(muted)
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .whitespace_nowrap()
                                    .child(fmt_duration(self.model.position)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(60.))
                                    .child(Slider::new(&self.seek).disabled(!has_track)),
                            )
                            .child(div().flex_shrink_0().whitespace_nowrap().child(total)),
                    )
                    .when_some(second_line, |el, (text, color)| {
                        el.child(
                            div()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_xs()
                                .line_height(relative(1.2))
                                .text_color(color)
                                .child(text),
                        )
                    }),
            )
    }
}

impl Render for Toolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let play_icon = match self.model.state {
            PlaybackState::Playing => IconName::Pause,
            _ => IconName::Play,
        };

        h_flex()
            .w_full()
            .h(px(76.))
            .px_3()
            .gap_4()
            .items_center()
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(self.chrome_button(
                "toggle-sidebar",
                IconName::PanelLeft,
                ToolbarEvent::ToggleSidebar,
                false,
                cx,
            ))
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(self.transport_button(
                        "prev",
                        IconName::SkipBack,
                        ToolbarEvent::Prev,
                        cx,
                    ))
                    .child(self.transport_button("play", play_icon, ToolbarEvent::PlayPause, cx))
                    .child(self.transport_button(
                        "next",
                        IconName::SkipForward,
                        ToolbarEvent::Next,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .w(px(140.))
                    .child(Icon::new(IconName::Volume2).small())
                    .child(
                        Slider::new(&self.volume)
                            .disabled(!self.model.enabled)
                            .flex_1(),
                    ),
            )
            .child(self.now_playing(cx))
            .child(
                div()
                    .w(px(220.))
                    .min_w(px(100.))
                    // Capture so the field still clears itself; we only
                    // report that focus should leave it.
                    .capture_action(cx.listener(|_, _: &Escape, _, cx| {
                        cx.emit(ToolbarEvent::SearchEscape);
                        cx.propagate();
                    }))
                    .child(Input::new(&self.search).small().cleanable(true)),
            )
    }
}

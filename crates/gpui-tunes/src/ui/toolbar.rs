//! Top bar: transport buttons, now-playing display with seek, volume, search.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{ActiveTheme, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
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
    /// Current track artist.
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

    fn now_playing(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let has_track = !self.model.title.is_empty();
        let total = self
            .model
            .duration
            .map_or_else(|| "--:--".into(), fmt_duration);

        h_flex()
            .flex_1()
            // Keep room for title + seek bar; the search box gives way first.
            .min_w(px(220.))
            .h(px(52.))
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
                    .child(div().text_sm().whitespace_nowrap().overflow_hidden().child(
                        if has_track {
                            self.model.title.clone()
                        } else {
                            "Not Playing".into()
                        },
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .text_xs()
                            .text_color(muted)
                            .child(fmt_duration(self.model.position))
                            .child(Slider::new(&self.seek).disabled(!has_track).flex_1())
                            .child(total),
                    )
                    .when_some(self.model.notice.clone(), |el, notice| {
                        el.child(div().text_xs().text_color(cx.theme().danger).child(notice))
                    }),
            )
            .when(has_track, |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .whitespace_nowrap()
                        .child(self.model.artist.clone()),
                )
            })
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
            .h(px(72.))
            .px_3()
            .gap_4()
            .items_center()
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().border)
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
                    .child(Slider::new(&self.volume).flex_1()),
            )
            .child(self.now_playing(cx))
            .child(
                div()
                    .w(px(220.))
                    .min_w(px(100.))
                    .child(Input::new(&self.search).small().cleanable(true)),
            )
    }
}

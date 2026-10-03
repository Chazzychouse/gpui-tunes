//! gpui-tunes binary entry point.

mod ui;

use std::path::PathBuf;

use gpui_kit::component::{Theme, ThemeMode};
#[allow(clippy::wildcard_imports)] // gpui_kit re-exports the gpui prelude
use gpui_kit::*;

use crate::ui::app::AppView;

fn library_root() -> PathBuf {
    std::env::var_os("GPUI_TUNES_LIBRARY").map_or_else(
        || dirs::home_dir().unwrap_or_default().join("music/library"),
        PathBuf::from,
    )
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let root = library_root();

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);

            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("gpui-tunes".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1200.), px(760.)),
                    cx,
                ))),
                ..Default::default()
            };

            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| AppView::new(root.clone(), window, cx))
            })
            .expect("failed to open window");
        });
}

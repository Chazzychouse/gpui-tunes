//! Left sidebar listing the library sections.

use gpui_kit::component::sidebar::{Sidebar, SidebarGroup, SidebarMenu, SidebarMenuItem};
use gpui_kit::component::{ActiveTheme, Icon};
#[allow(clippy::wildcard_imports)] // gpui_kit re-exports the gpui prelude
use gpui_kit::*;

use crate::ui::section::Section;

/// Emitted when the user picks a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarEvent {
    /// A section was clicked.
    Select(Section),
}

/// The sidebar view.
pub struct LibrarySidebar {
    active: Section,
    collapsed: bool,
}

impl EventEmitter<SidebarEvent> for LibrarySidebar {}

impl LibrarySidebar {
    /// Sidebar with Songs selected.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            active: Section::Songs,
            collapsed: false,
        }
    }

    /// Show the icon-only rail (`true`) or the full sidebar.
    pub fn set_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.collapsed = collapsed;
        cx.notify();
    }

    fn icon(section: Section) -> gpui_kit::assets::IconName {
        use gpui_kit::assets::IconName;
        match section {
            Section::Songs => IconName::Music,
            Section::Artists => IconName::User,
            Section::Albums => IconName::Disc,
            Section::Genres => IconName::ListMusic,
        }
    }
}

impl Render for LibrarySidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = Section::ALL.map(|section| {
            SidebarMenuItem::new(section.label())
                .icon(Icon::new(Self::icon(section)))
                .active(self.active == section)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.active = section;
                    cx.emit(SidebarEvent::Select(section));
                    cx.notify();
                }))
        });

        Sidebar::new("library-sidebar")
            .collapsed(self.collapsed)
            .w_full()
            .h_full()
            .bg(cx.theme().sidebar)
            .child(SidebarGroup::new("Library").child(SidebarMenu::new().children(items)))
    }
}

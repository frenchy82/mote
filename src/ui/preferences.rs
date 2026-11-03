//! The Settings window, laid out like Hylki's: one window, two panes, a
//! sidebar of categories under section headings and the chosen category's
//! groups beside it. Narrow, it collapses to one pane with the categories
//! first. Built once; closing hides it, so it opens again at once.

mod pages;
mod video;

use adw::prelude::*;
use relm4::prelude::*;

use crate::config::{ColorScheme, Settings, SortOrder};
use crate::connection::{Quality, Scaling};
use crate::i18n::{i18n, i18n_noop};

/// A category in the sidebar: the stack page it shows.
struct SidePage {
    id: &'static str,
    title: &'static str,
    icon: &'static str,
}

/// The sidebar, section by section. Titles are msgids, translated when the
/// rows are built.
const SIDE_PAGES: &[(&str, &[SidePage])] = &[
    (
        i18n_noop("Connections"),
        &[SidePage {
            id: "defaults",
            title: i18n_noop("Connection Defaults"),
            icon: "network-server-symbolic",
        }],
    ),
    (
        i18n_noop("Settings"),
        &[
            SidePage {
                id: "appearance",
                title: i18n_noop("Appearance"),
                icon: "preferences-desktop-appearance-symbolic",
            },
            SidePage {
                id: "sessions",
                title: i18n_noop("Sessions"),
                icon: "preferences-desktop-remote-desktop-symbolic",
            },
            SidePage {
                id: "sidebar",
                title: i18n_noop("Sidebar"),
                icon: "sidebar-show-symbolic",
            },
            SidePage {
                id: "video",
                title: i18n_noop("Video"),
                icon: "video-display-symbolic",
            },
        ],
    ),
];

fn side_page(id: &str) -> Option<&'static SidePage> {
    SIDE_PAGES
        .iter()
        .flat_map(|(_, pages)| pages.iter())
        .find(|p| p.id == id)
}

/// A setting the user changed, for the main window to apply and save.
#[derive(Debug)]
pub(crate) enum PrefChange {
    ColorScheme(ColorScheme),
    Sort(SortOrder),
    ShowGroups(bool),
    ShowHosts(bool),
    ShowAvatars(bool),
    ShowLargeAvatars(bool),
    DefaultPort(u16),
    DefaultUsername(String),
    DefaultDomain(String),
    DefaultScaling(Scaling),
    DefaultQuality(Quality),
    DefaultClipboard(bool),
    DefaultSound(bool),
    DefaultMicrophone(bool),
    DefaultFullscreen(bool),
    DefaultNewWindow(bool),
    /// A Windows layout ID; 0 follows this computer's.
    KeyboardLayout(u32),
    GrabKeyboard(bool),
    Reconnect(bool),
    GrabNotice(bool),
    H264Warning(bool),
    SizeNotice(bool),
    ClipboardNotice(bool),
    Decoder(Option<crate::rdp::h264::Source>),
}

#[derive(Debug)]
pub(crate) enum PrefInput {
    SelectPage(String),
}

pub(crate) struct Preferences {
    side_list: gtk::ListBox,
    stack: gtk::Stack,
    content_page: adw::NavigationPage,
    split: adw::NavigationSplitView,
}

#[relm4::component(pub(crate))]
impl SimpleComponent for Preferences {
    type Init = Settings;
    type Input = PrefInput;
    type Output = PrefChange;

    view! {
        adw::Window {
            set_modal: false,
            set_default_width: 860,
            set_default_height: 640,
            set_title: Some(&i18n("Settings")),
            set_hide_on_close: true,

            #[local_ref]
            split -> adw::NavigationSplitView {
                set_min_sidebar_width: 180.0,
                set_max_sidebar_width: 250.0,
                set_sidebar_width_fraction: 0.27,

                #[wrap(Some)]
                set_sidebar = &adw::NavigationPage {
                    set_title: &i18n("Settings"),

                    #[wrap(Some)]
                    set_child = &adw::ToolbarView {
                        add_top_bar = &adw::HeaderBar {},

                        #[wrap(Some)]
                        set_content = &gtk::ScrolledWindow {
                            set_hscrollbar_policy: gtk::PolicyType::Never,

                            set_child: Some(&side_list),
                        },
                    },
                },

                #[local_ref]
                #[wrap(Some)]
                set_content = content_page -> adw::NavigationPage {
                    #[wrap(Some)]
                    set_child = &adw::ToolbarView {
                        add_top_bar = &adw::HeaderBar {},

                        #[local_ref]
                        #[wrap(Some)]
                        set_content = stack -> gtk::Stack {
                            // Categories switch outright, no fade, and only the
                            // shown one is measured.
                            set_transition_type: gtk::StackTransitionType::None,
                            set_hhomogeneous: false,
                            set_vhomogeneous: false,
                        },
                    },
                },
            },
        }
    }

    fn init(
        settings: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let split = adw::NavigationSplitView::new();
        let content_page = adw::NavigationPage::new(&gtk::Box::default(), "");
        let stack = gtk::Stack::new();
        for (id, page) in pages::build(&settings, &sender) {
            stack.add_named(&page, Some(id));
        }

        let side_list = gtk::ListBox::new();
        side_list.add_css_class("navigation-sidebar");
        side_list.add_css_class("settings-side");
        for (section, pages) in SIDE_PAGES {
            let heading = gtk::ListBoxRow::new();
            heading.set_selectable(false);
            heading.set_activatable(false);
            heading.add_css_class("settings-side-heading");
            let label = gtk::Label::new(Some(&i18n(section)));
            label.set_xalign(0.0);
            label.add_css_class("caption-heading");
            label.add_css_class("dim-label");
            heading.set_child(Some(&label));
            heading.update_property(&[gtk::accessible::Property::Label(&i18n(section))]);
            side_list.append(&heading);
            for page in pages.iter() {
                let row = gtk::ListBoxRow::new();
                row.set_widget_name(&format!("page:{}", page.id));
                let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                line.append(&gtk::Image::from_icon_name(page.icon));
                let label = gtk::Label::new(Some(&i18n(page.title)));
                label.set_xalign(0.0);
                line.append(&label);
                row.set_child(Some(&line));
                row.update_property(&[gtk::accessible::Property::Label(&i18n(page.title))]);
                side_list.append(&row);
            }
        }
        let input = sender.input_sender().clone();
        side_list.connect_row_selected(move |_, row| {
            if let Some(id) =
                row.and_then(|r| r.widget_name().strip_prefix("page:").map(str::to_owned))
            {
                input.emit(PrefInput::SelectPage(id));
            }
        });

        let model = Preferences {
            side_list: side_list.clone(),
            stack: stack.clone(),
            content_page: content_page.clone(),
            split: split.clone(),
        };
        let (split, content_page, stack) = (&split, &content_page, &stack);
        let widgets = view_output!();

        // One pane under 640sp: the categories first, a category on top.
        let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            640.0,
            adw::LengthUnit::Sp,
        ));
        narrow.add_setter(split, "collapsed", Some(&true.to_value()));
        root.add_breakpoint(narrow);

        // The first category, past its section heading.
        side_list.select_row(side_list.row_at_index(1).as_ref());
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            PrefInput::SelectPage(id) => {
                let Some(page) = side_page(&id) else { return };
                // Asked for from outside, the sidebar follows; picked there,
                // the row is already selected and this changes nothing.
                let name = format!("page:{id}");
                let mut i = 0;
                while let Some(row) = self.side_list.row_at_index(i) {
                    if row.widget_name() == name && !row.is_selected() {
                        self.side_list.select_row(Some(&row));
                    }
                    i += 1;
                }
                self.stack.set_visible_child_name(page.id);
                self.content_page.set_title(&i18n(page.title));
                self.split.set_show_content(true);
            }
        }
    }
}

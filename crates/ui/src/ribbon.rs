//! The ribbon: File, Home, Send / Receive, Folder and View, for mail and for the calendar.

use egui::{Ui, vec2};
use mailcraft_store::{ListFilter, SortBy};

use crate::app::{Backstage, Confirm, MailApp, View};
use crate::settings::{CalendarView, ReadingPane};
use crate::widgets::{self, big, group, small, stack};

pub const MAIL_TABS: [&str; 5] = ["File", "Home", "Send / Receive", "Folder", "View"];

pub fn ribbon(app: &mut MailApp, ui: &mut Ui) {
    if let Some(i) = widgets::tab_strip(ui, &MAIL_TABS, app.tab) {
        if i == 0 {
            app.backstage = Some(Backstage::Info);
        } else {
            app.tab = i;
        }
    }
    ui.add_space(2.0);
    egui::ScrollArea::horizontal().id_salt("ribbon_scroll").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.set_height(widgets::CONTENT_H + widgets::LABEL_H);
            ui.add_space(2.0);
            match (app.view, app.tab) {
                (View::Mail, 1) => mail_home(app, ui),
                (View::Calendar, 1) => calendar_home(app, ui),
                (_, 2) => send_receive(app, ui),
                (_, 3) => folder_tab(app, ui),
                (View::Mail, _) => mail_view_tab(app, ui),
                (View::Calendar, _) => calendar_view_tab(app, ui),
            }
        });
    });
}

fn mail_home(app: &mut MailApp, ui: &mut Ui) {
    let has_sel = !app.mail.selected.is_empty();
    let one = app.mail.selected.len() == 1;
    group(ui, "New", |ui| {
        if big(ui, "new_mail", "New\nEmail", true, false, false).on_hover_text("New Email (Ctrl+N)").clicked() {
            app.new_mail();
        }
        let r = big(ui, "new_items", "New\nItems", true, false, true);
        egui::Popup::menu(&r).show(|ui| {
            if widgets::menu_item(ui, "mail", "Email Message", "Ctrl+N").clicked() {
                app.new_mail();
                ui.close();
            }
            if widgets::menu_item(ui, "new_appointment", "Appointment", "").clicked() {
                app.new_event(false, None);
                ui.close();
            }
            if widgets::menu_item(ui, "new_meeting", "Meeting", "").clicked() {
                app.new_event(true, None);
                ui.close();
            }
        });
    });
    group(ui, "Delete", |ui| {
        if big(ui, "delete", "Delete", has_sel, false, false).on_hover_text("Delete (Del)").clicked() && has_sel {
            app.delete_selected();
        }
        if big(ui, "archive", "Archive", has_sel, false, false).on_hover_text("Archive (Backspace)").clicked() && has_sel {
            app.archive_selected();
        }
    });
    group(ui, "Respond", |ui| {
        let can = one && !app.targets().first().is_some_and(|m| m.id.starts_with("local:"));
        if big(ui, "reply", "Reply", can, false, false).on_hover_text("Reply (Ctrl+R)").clicked() && can {
            app.reply("reply");
        }
        if big(ui, "reply_all", "Reply\nAll", can, false, false).on_hover_text("Reply All (Ctrl+Shift+R)").clicked() && can {
            app.reply("replyAll");
        }
        if big(ui, "forward", "Forward", can, false, false).on_hover_text("Forward (Ctrl+F)").clicked() && can {
            app.reply("forward");
        }
    });
    group(ui, "Move", |ui| {
        let r = big(ui, "move", "Move", has_sel, false, true);
        if has_sel {
            egui::Popup::menu(&r).show(|ui| move_menu(app, ui));
        }
    });
    group(ui, "Tags", |ui| {
        stack(ui, |ui| {
            if small(ui, "unread", "Unread/ Read", has_sel, false, false).on_hover_text("Mark as Unread or Read (Ctrl+U / Ctrl+Q)").clicked() && has_sel {
                app.toggle_read_selected();
            }
            let r = small(ui, "categorize", "Categorize", has_sel, false, true);
            if has_sel {
                egui::Popup::menu(&r).show(|ui| categorize_menu(app, ui));
            }
            let r = small(ui, "flag", "Follow Up", has_sel, false, true);
            if has_sel {
                egui::Popup::menu(&r).show(|ui| follow_up_menu(app, ui));
            }
        });
    });
    group(ui, "Find", |ui| {
        stack(ui, |ui| {
            let label = match app.mail.filter {
                ListFilter::All | ListFilter::Focused | ListFilter::Other => "Filter Email",
                ListFilter::Unread => "Filter: Unread",
                ListFilter::Flagged => "Filter: Flagged",
                ListFilter::Attachments => "Filter: Attachments",
                ListFilter::ToMe => "Filter: To Me",
            };
            let r = small(ui, "filter", label, true, app.mail.filter != ListFilter::All, true);
            egui::Popup::menu(&r).show(|ui| filter_menu(app, ui));
            if small(ui, "search", "Search", true, false, false).on_hover_text("Search (Ctrl+E)").clicked() {
                ui.memory_mut(|m| m.request_focus(egui::Id::new("search_box")));
            }
        });
    });
    group(ui, "Send/Receive", |ui| {
        if big(ui, "send_receive", "Send/Receive\nAll Folders", true, false, false).on_hover_text("F9").clicked() {
            app.sync_all();
        }
    });
}

pub fn move_menu(app: &mut MailApp, ui: &mut Ui) {
    ui.set_min_width(220.0);
    let folders = app.mail.folders.clone();
    let depth = |f: &mailcraft_store::FolderRow| {
        let mut d = 0;
        let mut p = f.parent_id.clone();
        while let Some(pid) = p {
            d += 1;
            p = folders.iter().find(|x| x.id == pid).and_then(|x| x.parent_id.clone());
            if d > 10 {
                break;
            }
        }
        d
    };
    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        for f in &folders {
            if f.hidden || f.well_known.as_deref().is_some_and(|w| matches!(w, "outbox" | "conversationhistory")) {
                continue;
            }
            let indent = depth(f) as f32 * 14.0;
            ui.horizontal(|ui| {
                ui.add_space(indent);
                if widgets::menu_item(ui, crate::icons::folder_icon(f.well_known.as_deref()), &f.name, "").clicked() {
                    app.move_selected(&f.id);
                    ui.close();
                }
            });
        }
    });
}

pub const CATEGORIES: [&str; 6] = ["Red category", "Orange category", "Yellow category", "Green category", "Blue category", "Purple category"];

pub fn categorize_menu(app: &mut MailApp, ui: &mut Ui) {
    for c in CATEGORIES {
        let checked = app.targets().iter().all(|m| m.categories.iter().any(|x| x == c));
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 3.0, crate::theme::category_color(c));
            if ui.selectable_label(checked, c).clicked() {
                app.categorize_selected(c);
                ui.close();
            }
        });
    }
    ui.separator();
    if ui.button("Clear All Categories").clicked() {
        app.categorize_selected("");
        ui.close();
    }
}

pub fn follow_up_menu(app: &mut MailApp, ui: &mut Ui) {
    if widgets::menu_item(ui, "flag", "Flag Message", "Insert").clicked() {
        app.flag_selected("flagged");
        ui.close();
    }
    if widgets::menu_item(ui, "flag_complete", "Mark Complete", "").clicked() {
        app.flag_selected("complete");
        ui.close();
    }
    if widgets::menu_item(ui, "close", "Clear Flag", "").clicked() {
        app.flag_selected("notFlagged");
        ui.close();
    }
}

fn filter_menu(app: &mut MailApp, ui: &mut Ui) {
    for (f, label) in [
        (ListFilter::All, "All"),
        (ListFilter::Unread, "Unread"),
        (ListFilter::Flagged, "Flagged"),
        (ListFilter::Attachments, "Has Attachments"),
        (ListFilter::ToMe, "To Me"),
    ] {
        if ui.selectable_label(app.mail.filter == f, label).clicked() {
            app.mail.filter = f;
            app.mail.list_dirty = true;
            ui.close();
        }
    }
}

fn send_receive(app: &mut MailApp, ui: &mut Ui) {
    group(ui, "Send & Receive", |ui| {
        if big(ui, "send_receive", "Send/Receive\nAll Folders", true, false, false).on_hover_text("F9").clicked() {
            app.sync_all();
        }
        let folder = app.mail.folder.clone();
        if big(ui, "update_folder", "Update\nFolder", folder.is_some(), false, false).on_hover_text("Shift+F9").clicked() {
            if let Some(f) = folder {
                app.send(mailcraft_sync::Command::SyncFolder(f));
            }
        }
        let queued = app.session().map(|s| s.queued).unwrap_or(0);
        if big(ui, "send_all", "Send\nAll", true, false, false).on_hover_text(format!("{queued} waiting")).clicked() {
            app.sync_all();
        }
    });
    group(ui, "Preferences", |ui| {
        let off = app.settings.work_offline;
        if big(ui, if off { "offline" } else { "online" }, "Work\nOffline", true, off, false).clicked() {
            app.set_work_offline(!off);
        }
    });
    group(ui, "Offline cache", |ui| {
        let r = big(ui, "history", "Keep Mail\nOffline", true, false, true);
        egui::Popup::menu(&r).show(|ui| {
            for (d, label) in [(30, "1 month"), (90, "3 months"), (180, "6 months"), (365, "1 year"), (730, "2 years")] {
                if ui.selectable_label(app.settings.offline_days == d, label).clicked() {
                    app.settings.offline_days = d;
                    app.save_settings();
                    app.send(mailcraft_sync::Command::SetWindow(d));
                    ui.close();
                }
            }
        });
    });
}

fn folder_tab(app: &mut MailApp, ui: &mut Ui) {
    let cur = app.current_folder().cloned();
    let user_folder = cur.as_ref().is_some_and(|f| f.well_known.is_none());
    group(ui, "New", |ui| {
        if big(ui, "folder_new", "New\nFolder", !app.sessions.is_empty(), false, false).clicked() {
            app.mail.new_folder = Some((cur.as_ref().map(|f| f.id.clone()), String::new()));
        }
    });
    group(ui, "Actions", |ui| {
        if big(ui, "rename", "Rename\nFolder", user_folder, false, false).clicked() && user_folder {
            if let Some(f) = &cur {
                app.mail.renaming = Some((f.id.clone(), f.name.clone()));
            }
        }
        if big(ui, "folder_delete", "Delete\nFolder", user_folder, false, false).clicked() && user_folder {
            if let Some(f) = &cur {
                app.mail.confirm = Some(Confirm::DeleteFolder(f.id.clone(), f.name.clone()));
            }
        }
        if big(ui, "mark_all_read", "Mark All\nas Read", cur.is_some(), false, false).clicked() {
            if let Some(f) = &cur {
                app.mark_all_read(&f.id);
            }
        }
    });
    group(ui, "Clean Up", |ui| {
        let emptiable = cur.as_ref().is_some_and(|f| matches!(f.well_known.as_deref(), Some("deleteditems") | Some("junkemail")));
        if big(ui, "empty", "Empty\nFolder", emptiable, false, false).clicked() && emptiable {
            if let Some(f) = &cur {
                app.mail.confirm = Some(Confirm::EmptyFolder(f.id.clone(), f.name.clone()));
            }
        }
    });
    group(ui, "Favorites", |ui| {
        let fav = cur.as_ref().is_some_and(|f| app.settings.favorites.contains(&f.id));
        if big(ui, "star", "Show in\nFavorites", cur.is_some(), fav, false).clicked() {
            if let Some(f) = &cur {
                app.toggle_favorite(&f.id);
            }
        }
    });
}

fn mail_view_tab(app: &mut MailApp, ui: &mut Ui) {
    group(ui, "Layout", |ui| {
        let r = big(ui, "folder_pane", "Folder\nPane", true, false, true);
        egui::Popup::menu(&r).show(|ui| {
            if ui.selectable_label(app.settings.folder_pane, "Normal").clicked() {
                app.settings.folder_pane = true;
                app.save_settings();
                ui.close();
            }
            if ui.selectable_label(!app.settings.folder_pane, "Minimized").clicked() {
                app.settings.folder_pane = false;
                app.save_settings();
                ui.close();
            }
        });
        let icon = if app.settings.reading_pane == ReadingPane::Bottom { "reading_bottom" } else { "reading_pane" };
        let r = big(ui, icon, "Reading\nPane", true, false, true);
        egui::Popup::menu(&r).show(|ui| {
            for (p, label) in [(ReadingPane::Right, "Right"), (ReadingPane::Bottom, "Bottom"), (ReadingPane::Off, "Off")] {
                if ui.selectable_label(app.settings.reading_pane == p, label).clicked() {
                    app.settings.reading_pane = p;
                    app.save_settings();
                    ui.close();
                }
            }
        });
    });
    group(ui, "Focused Inbox", |ui| {
        let on = app.settings.focused_inbox;
        if big(ui, "focused", "Show Focused\nInbox", true, on, false).clicked() {
            app.settings.focused_inbox = !on;
            app.mail.list_dirty = true;
            app.save_settings();
        }
    });
    group(ui, "Arrangement", |ui| {
        let r = big(ui, "sort", "Sort\nBy", true, false, true);
        egui::Popup::menu(&r).show(|ui| {
            for (s, label) in [(SortBy::Date, "Date"), (SortBy::From, "From"), (SortBy::Subject, "Subject"), (SortBy::Importance, "Importance"), (SortBy::Flag, "Flag Status")] {
                if ui.selectable_label(app.mail.sort == s, label).clicked() {
                    app.mail.sort = s;
                    app.mail.ascending = s == SortBy::From || s == SortBy::Subject;
                    app.mail.list_dirty = true;
                    ui.close();
                }
            }
        });
        stack(ui, |ui| {
            if small(ui, "sort", "Reverse Sort", true, false, false).clicked() {
                app.mail.ascending = !app.mail.ascending;
                app.mail.list_dirty = true;
            }
            let prev = app.settings.message_preview;
            if small(ui, "mail", "Message Preview", true, prev, false).clicked() {
                app.settings.message_preview = !prev;
                app.save_settings();
            }
        });
    });
    theme_group(app, ui);
}

fn theme_group(app: &mut MailApp, ui: &mut Ui) {
    group(ui, "Office Theme", |ui| {
        let r = big(ui, "theme", "Theme", true, false, true);
        egui::Popup::menu(&r).show(|ui| {
            for a in crate::theme::Appearance::ALL {
                if ui.selectable_label(app.settings.appearance == a, a.label()).clicked() {
                    app.settings.appearance = a;
                    crate::theme::set_appearance(ui.ctx(), a);
                    app.save_settings();
                    ui.close();
                }
            }
        });
    });
}

fn calendar_home(app: &mut MailApp, ui: &mut Ui) {
    group(ui, "New", |ui| {
        if big(ui, "new_appointment", "New\nAppointment", true, false, false).on_hover_text("Ctrl+N").clicked() {
            app.new_event(false, None);
        }
        if big(ui, "new_meeting", "New\nMeeting", true, false, false).on_hover_text("Ctrl+Shift+Q").clicked() {
            app.new_event(true, None);
        }
        let r = big(ui, "new_items", "New\nItems", true, false, true);
        egui::Popup::menu(&r).show(|ui| {
            if widgets::menu_item(ui, "mail", "Email Message", "").clicked() {
                app.new_mail();
                ui.close();
            }
            if widgets::menu_item(ui, "new_appointment", "Appointment", "").clicked() {
                app.new_event(false, None);
                ui.close();
            }
            if widgets::menu_item(ui, "new_meeting", "Meeting", "").clicked() {
                app.new_event(true, None);
                ui.close();
            }
        });
    });
    group(ui, "Go To", |ui| {
        if big(ui, "today", "Today", true, false, false).clicked() {
            app.cal.go_today();
        }
        if big(ui, "next7", "Next 7\nDays", true, false, false).clicked() {
            app.cal.next_seven();
        }
    });
    group(ui, "Arrange", |ui| {
        for (v, icon) in [(CalendarView::Day, "day"), (CalendarView::WorkWeek, "work_week"), (CalendarView::Week, "week"), (CalendarView::Month, "month")] {
            let label = match v {
                CalendarView::WorkWeek => "Work\nWeek",
                other => other.label(),
            };
            if big(ui, icon, label, true, app.cal.view == v && !app.cal.seven, false).clicked() {
                app.cal.set_view(v);
                app.save_settings();
            }
        }
    });
    let sel = app.cal.selected.clone();
    group(ui, "Appointment", |ui| {
        if big(ui, "open", "Open", sel.is_some(), false, false).clicked() {
            if let Some(id) = &sel {
                app.open_event(id);
            }
        }
        if big(ui, "delete", "Delete", sel.is_some(), false, false).clicked() {
            if let Some(id) = &sel {
                app.delete_event(id);
            }
        }
    });
}

fn calendar_view_tab(app: &mut MailApp, ui: &mut Ui) {
    group(ui, "Arrangement", |ui| {
        for (v, icon) in [(CalendarView::Day, "day"), (CalendarView::WorkWeek, "work_week"), (CalendarView::Week, "week"), (CalendarView::Month, "month")] {
            let label = match v {
                CalendarView::WorkWeek => "Work\nWeek",
                other => other.label(),
            };
            if big(ui, icon, label, true, app.cal.view == v && !app.cal.seven, false).clicked() {
                app.cal.set_view(v);
                app.save_settings();
            }
        }
    });
    group(ui, "Time Scale", |ui| {
        let r = big(ui, "clock", "Working\nHours", true, false, true);
        egui::Popup::menu(&r).show(|ui| {
            ui.label("Start");
            let mut s = app.settings.work_start;
            egui::ComboBox::from_id_salt("ws").selected_text(format!("{s:02}:00")).show_ui(ui, |ui| {
                for h in 5..=12 {
                    ui.selectable_value(&mut s, h, format!("{h:02}:00"));
                }
            });
            ui.label("End");
            let mut e = app.settings.work_end;
            egui::ComboBox::from_id_salt("we").selected_text(format!("{e:02}:00")).show_ui(ui, |ui| {
                for h in 13..=22 {
                    ui.selectable_value(&mut e, h, format!("{h:02}:00"));
                }
            });
            if s != app.settings.work_start || e != app.settings.work_end {
                app.settings.work_start = s;
                app.settings.work_end = e;
                app.save_settings();
            }
        });
    });
    theme_group(app, ui);
}

//! The folder pane: Favorites, the mailbox's folder tree, and the navigation bar (Mail, Calendar).

use egui::{Align2, Rect, Sense, Ui, pos2, vec2};
use mailcraft_store::FolderRow;

use crate::app::{Confirm, MailApp, View};
use crate::icons;
use crate::theme::{Tokens, medium, regular, semibold};
use crate::widgets;

/// Messages being dragged from the list.
#[derive(Clone, Debug)]
pub struct DragMessages(pub Vec<String>);

const ROW_H: f32 = 26.0;

pub fn pane(app: &mut MailApp, ui: &mut Ui) {
    egui::Panel::bottom("nav").exact_size(44.0).frame(egui::Frame::new()).show(ui, |ui| nav_bar(app, ui, false));
    egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(egui::Margin { left: 4, right: 4, top: 6, bottom: 0 })).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("folders").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let favorites = favorite_folders(app);
            if !favorites.is_empty() {
                let open = section(ui, "Favorites", "fav_section", true);
                if open {
                    for f in favorites {
                        folder_row(app, ui, &f, 0, false, true);
                    }
                }
                ui.add_space(8.0);
            }
            let title = app.session.as_ref().map(|s| if s.address.is_empty() { s.name.clone() } else { s.address.clone() }).unwrap_or_default();
            if section(ui, &title, "account_section", true) {
                let folders = app.mail.folders.clone();
                let roots: Vec<FolderRow> = folders.iter().filter(|f| f.parent_id.is_none() && !f.hidden).cloned().collect();
                for f in roots {
                    tree(app, ui, &folders, &f, 0);
                }
                if folders.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Loading folders\u{2026}").color(Tokens::get(ui.ctx()).text_dim));
                }
            }
        });
    });
}

/// The pane minimized to its navigation icons.
pub fn minimized(app: &mut MailApp, ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(8.0);
        if widgets::icon_button(ui, "chevron_right", "Expand the Folder Pane", true).clicked() {
            app.settings.folder_pane = true;
            app.save_settings();
        }
        ui.add_space(8.0);
        nav_bar(app, ui, true);
    });
}

fn favorite_folders(app: &MailApp) -> Vec<FolderRow> {
    let ids: Vec<String> = if app.settings.favorites.is_empty() {
        ["inbox", "sentitems", "drafts", "deleteditems"].iter().filter_map(|wk| app.folder_wk(wk)).collect()
    } else {
        app.settings.favorites.clone()
    };
    ids.iter().filter_map(|id| app.mail.folders.iter().find(|f| &f.id == id).cloned()).collect()
}

impl MailApp {
    pub fn toggle_favorite(&mut self, id: &str) {
        if self.settings.favorites.is_empty() {
            self.settings.favorites = ["inbox", "sentitems", "drafts", "deleteditems"].iter().filter_map(|wk| self.folder_wk(wk)).collect();
        }
        if let Some(p) = self.settings.favorites.iter().position(|f| f == id) {
            self.settings.favorites.remove(p);
        } else {
            self.settings.favorites.push(id.to_string());
        }
        self.save_settings();
    }
}

/// A collapsible section heading. Returns whether it's open.
fn section(ui: &mut Ui, title: &str, key: &str, default_open: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let id = ui.id().with(key);
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(default_open);
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 4.0, t.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 10.0, r.center().y), vec2(11.0, 11.0)), if open { "expand" } else { "collapsed" }, t.text_dim, t.text_dim);
    let text = crate::list::elide(ui.painter(), title, semibold(12.5), r.width() - 28.0);
    ui.painter().text(pos2(r.min.x + 20.0, r.center().y), Align2::LEFT_CENTER, text, semibold(12.5), t.text);
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    open
}

fn tree(app: &mut MailApp, ui: &mut Ui, all: &[FolderRow], f: &FolderRow, depth: usize) {
    let kids: Vec<&FolderRow> = all.iter().filter(|c| c.parent_id.as_deref() == Some(f.id.as_str()) && !c.hidden).collect();
    let expanded = folder_row(app, ui, f, depth, !kids.is_empty(), false);
    if expanded && depth < 12 {
        for k in kids {
            tree(app, ui, all, k, depth + 1);
        }
    }
}

/// One folder. Returns whether its children are showing.
fn folder_row(app: &mut MailApp, ui: &mut Ui, f: &FolderRow, depth: usize, has_kids: bool, favorite: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let selected = app.mail.folder.as_deref() == Some(f.id.as_str()) && app.view == View::Mail && app.mail.searching.is_none();
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let dragging = egui::DragAndDrop::has_payload_of_type::<DragMessages>(ui.ctx());
    let drop_hover = dragging && resp.contains_pointer();
    if selected {
        ui.painter().rect_filled(r, 4.0, t.selected);
    } else if drop_hover {
        ui.painter().rect_filled(r, 4.0, t.checked);
        ui.painter().rect_stroke(r, 4.0, egui::Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 4.0, t.hover);
    }
    let x0 = r.min.x + 6.0 + depth as f32 * 14.0;
    let key = if favorite { format!("fav:{}", f.id) } else { f.id.clone() };
    let mut expanded = app.mail.expanded.contains(&f.id) || (f.well_known.as_deref() == Some("inbox") && !app.mail.expanded.contains(&format!("closed:{}", f.id)));
    if has_kids {
        let ar = Rect::from_center_size(pos2(x0 + 6.0, r.center().y), vec2(14.0, 14.0));
        let a = ui.interact(ar, ui.id().with(("expander", &key)), Sense::click());
        icons::paint(ui.painter(), ar.shrink(2.0), if expanded { "expand" } else { "collapsed" }, if a.hovered() { t.text } else { t.text_dim }, t.text_dim);
        if a.clicked() {
            expanded = !expanded;
            if f.well_known.as_deref() == Some("inbox") {
                let closed = format!("closed:{}", f.id);
                if expanded {
                    app.mail.expanded.remove(&closed);
                } else {
                    app.mail.expanded.insert(closed);
                }
            } else if expanded {
                app.mail.expanded.insert(f.id.clone());
            } else {
                app.mail.expanded.remove(&f.id);
            }
            app.save_settings();
        }
    }
    let icon_x = x0 + 18.0;
    icons::paint(ui.painter(), Rect::from_center_size(pos2(icon_x + 8.0, r.center().y), vec2(16.0, 16.0)), icons::folder_icon(f.well_known.as_deref()), t.icon, t.accent);
    // Drafts and Outbox show how many are in them; other folders how many are unread.
    let (count, bracket) = match f.well_known.as_deref() {
        Some("drafts") | Some("outbox") => (f.total, true),
        _ => (f.unread, false),
    };
    let strong = count > 0 && !bracket;
    let count_text = if count <= 0 {
        String::new()
    } else if bracket {
        format!("[{count}]")
    } else {
        count.to_string()
    };
    let cw = if count_text.is_empty() { 0.0 } else { ui.painter().layout_no_wrap(count_text.clone(), medium(12.0), t.accent_text).size().x + 10.0 };
    let name_font = if strong { semibold(12.5) } else { regular(12.5) };
    let max_w = (r.max.x - cw - icon_x - 26.0).max(20.0);
    let galley = ui.painter().layout(f.name.clone(), name_font, t.text, f32::INFINITY);
    let name = if galley.size().x > max_w {
        let mut s = f.name.clone();
        while s.chars().count() > 2 && ui.painter().layout_no_wrap(format!("{s}\u{2026}"), if strong { semibold(12.5) } else { regular(12.5) }, t.text).size().x > max_w {
            s.pop();
        }
        format!("{s}\u{2026}")
    } else {
        f.name.clone()
    };
    ui.painter().text(pos2(icon_x + 22.0, r.center().y), Align2::LEFT_CENTER, &name, if strong { semibold(12.5) } else { regular(12.5) }, t.text);
    if !count_text.is_empty() {
        ui.painter().text(pos2(r.max.x - 8.0, r.center().y), Align2::RIGHT_CENTER, count_text, medium(12.0), if bracket { t.text_dim } else { t.accent_text });
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &f.name));
    if resp.clicked() {
        app.select_folder(&f.id);
    }
    if let Some(p) = resp.dnd_release_payload::<DragMessages>() {
        let ids = p.0.clone();
        app.mail.selected = ids;
        app.move_selected(&f.id);
    }
    let fid = f.id.clone();
    let fname = f.name.clone();
    let user = f.well_known.is_none();
    let wk = f.well_known.clone();
    resp.context_menu(|ui| {
        ui.set_min_width(220.0);
        if widgets::menu_item(ui, "folder_new", "New Folder\u{2026}", "").clicked() {
            app.mail.new_folder = Some((Some(fid.clone()), String::new()));
            ui.close();
        }
        if user && widgets::menu_item(ui, "rename", "Rename Folder", "").clicked() {
            app.mail.renaming = Some((fid.clone(), fname.clone()));
            ui.close();
        }
        if user && widgets::menu_item(ui, "folder_delete", "Delete Folder", "").clicked() {
            app.mail.confirm = Some(Confirm::DeleteFolder(fid.clone(), fname.clone()));
            ui.close();
        }
        ui.separator();
        if widgets::menu_item(ui, "mark_all_read", "Mark All as Read", "").clicked() {
            app.mark_all_read(&fid);
            ui.close();
        }
        if matches!(wk.as_deref(), Some("deleteditems") | Some("junkemail")) && widgets::menu_item(ui, "empty", "Empty Folder", "").clicked() {
            app.mail.confirm = Some(Confirm::EmptyFolder(fid.clone(), fname.clone()));
            ui.close();
        }
        let fav = app.settings.favorites.contains(&fid) || (app.settings.favorites.is_empty() && matches!(wk.as_deref(), Some("inbox" | "sentitems" | "drafts" | "deleteditems")));
        if widgets::menu_item(ui, "star", if fav { "Remove from Favorites" } else { "Show in Favorites" }, "").clicked() {
            app.toggle_favorite(&fid);
            ui.close();
        }
        if widgets::menu_item(ui, "update_folder", "Update Folder", "").clicked() {
            app.send(mailcraft_sync::Command::SyncFolder(fid.clone()));
            ui.close();
        }
    });
    expanded && has_kids
}

/// Mail and Calendar, at the foot of the pane.
pub fn nav_bar(app: &mut MailApp, ui: &mut Ui, vertical: bool) {
    let t = Tokens::get(ui.ctx());
    if !vertical {
        let r = ui.max_rect();
        ui.painter().line_segment([r.left_top(), r.right_top()], egui::Stroke::new(1.0, t.border));
    }
    let layout = if vertical { egui::Layout::top_down(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) };
    ui.with_layout(layout, |ui| {
        ui.add_space(8.0);
        for (v, icon, label) in [(View::Mail, "mail", "Mail"), (View::Calendar, "calendar", "Calendar")] {
            let on = app.view == v;
            let (r, resp) = ui.allocate_exact_size(vec2(40.0, 36.0), Sense::click());
            if on {
                ui.painter().rect_filled(r.shrink(2.0), 4.0, t.checked);
            } else if resp.hovered() {
                ui.painter().rect_filled(r.shrink(2.0), 4.0, t.hover);
            }
            icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(20.0, 20.0)), icon, if on { t.accent } else { t.icon }, t.accent);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, label));
            let shortcut = if v == View::Mail { "Ctrl+1" } else { "Ctrl+2" };
            if resp.on_hover_text(format!("{label} ({shortcut})")).clicked() {
                app.set_view(v);
            }
        }
    });
}

impl MailApp {
    pub fn set_view(&mut self, v: View) {
        if self.view != v {
            self.view = v;
            self.tab = 1;
            if v == View::Calendar {
                self.cal.dirty = true;
                self.cal.request_window(self.session.as_ref().map(|s| &s.sync), &self.settings);
            }
        }
    }
}

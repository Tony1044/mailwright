//! Small dialogs: new folder, rename folder, and confirmations.

use egui::{Context, vec2};

use crate::app::{Confirm, MailApp};
use crate::theme::semibold;
use crate::widgets;

pub fn show(app: &mut MailApp, ctx: &Context) {
    if let Some((parent, mut name)) = app.mail.new_folder.take() {
        let mut keep = true;
        egui::Modal::new(egui::Id::new("new_folder")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(egui::RichText::new("Create New Folder").font(semibold(15.0)));
            ui.add_space(8.0);
            let parent_name = parent.as_ref().and_then(|p| app.mail.folders.iter().find(|f| &f.id == p)).map(|f| f.name.clone()).unwrap_or_else(|| "the top of the mailbox".into());
            ui.label(format!("In {parent_name}:"));
            let r = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Folder name").desired_width(f32::INFINITY));
            r.request_focus();
            ui.add_space(10.0);
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                if (widgets::primary(ui, "OK").clicked() || enter) && !name.trim().is_empty() {
                    let owner = parent.as_deref().and_then(|p| app.session_for_folder(p)).unwrap_or(app.active);
                    app.send_to(owner, mailwright_sync::Command::CreateFolder { parent: parent.clone(), name: name.trim().to_string() });
                    keep = false;
                }
                if widgets::secondary(ui, "Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                }
            });
        });
        if keep {
            app.mail.new_folder = Some((parent, name));
        }
    }
    if let Some((id, mut name)) = app.mail.renaming.take() {
        let mut keep = true;
        egui::Modal::new(egui::Id::new("rename_folder")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(egui::RichText::new("Rename Folder").font(semibold(15.0)));
            ui.add_space(8.0);
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
            r.request_focus();
            ui.add_space(10.0);
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                if (widgets::primary(ui, "OK").clicked() || enter) && !name.trim().is_empty() {
                    let owner = app.session_for_folder(&id).unwrap_or(app.active);
                    app.send_to(owner, mailwright_sync::Command::RenameFolder { id: id.clone(), name: name.trim().to_string() });
                    keep = false;
                }
                if widgets::secondary(ui, "Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                }
            });
        });
        if keep {
            app.mail.renaming = Some((id, name));
        }
    }
    if let Some(c) = app.mail.confirm.clone() {
        let (title, text, yes) = match &c {
            Confirm::DeleteFolder(_, n) => ("Delete Folder".to_string(), format!("Delete \u{201c}{n}\u{201d} and move everything in it to Deleted Items?"), "Delete"),
            Confirm::EmptyFolder(_, n) => ("Empty Folder".to_string(), format!("Everything in \u{201c}{n}\u{201d} will be deleted for good. Continue?"), "Empty"),
            Confirm::DeleteForever(ids) => (
                "Delete Permanently".to_string(),
                if ids.len() == 1 { "This item will be deleted for good. Continue?".to_string() } else { format!("These {} items will be deleted for good. Continue?", ids.len()) },
                "Delete",
            ),
            Confirm::RemoveAccount(a) => ("Remove Account".to_string(), format!("Remove {a} from Mailwright? This computer's copy of its mail and calendar, and its saved sign-in, are removed. Nothing changes on the server."), "Remove"),
        };
        let mut answer = None;
        egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(egui::RichText::new(title).font(semibold(15.0)));
            ui.add_space(8.0);
            ui.add(egui::Label::new(text).wrap());
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if widgets::primary(ui, yes).clicked() {
                    answer = Some(true);
                }
                if widgets::secondary(ui, "Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    answer = Some(false);
                }
            });
            ui.add_space(2.0);
            let _ = vec2(0.0, 0.0);
        });
        match answer {
            Some(true) => {
                app.mail.confirm = None;
                match c {
                    Confirm::DeleteFolder(id, _) => {
                        if app.mail.folder.as_deref() == Some(id.as_str()) {
                            if let Some(inbox) = app.folder_wk("inbox") {
                                app.select_folder(&inbox);
                            }
                        }
                        let owner = app.session_for_folder(&id).unwrap_or(app.active);
                        app.send_to(owner, mailwright_sync::Command::DeleteFolder { id });
                    }
                    Confirm::EmptyFolder(id, _) => app.empty_folder(&id),
                    Confirm::DeleteForever(ids) => app.delete_forever(ids),
                    Confirm::RemoveAccount(a) => app.remove_account(&a),
                }
            }
            Some(false) => app.mail.confirm = None,
            None => {}
        }
    }
}

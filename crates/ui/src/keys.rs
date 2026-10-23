//! Keyboard shortcuts (Outlook's, where they make sense here).

use egui::{Context, Key, KeyboardShortcut, Modifiers};

use crate::app::{MailApp, View};
use crate::settings::CalendarView;

fn take(ctx: &Context, m: Modifiers, k: Key) -> bool {
    ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)))
}

pub fn shortcuts(app: &mut MailApp, ctx: &Context) {
    if app.session.is_none() || app.backstage.is_some() {
        return;
    }
    let typing = ctx.memory(|m| m.focused().is_some()) && ctx.egui_wants_keyboard_input();
    let cmd = Modifiers::COMMAND;
    let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
    if take(ctx, Modifiers::NONE, Key::F9) {
        app.sync_all();
    }
    if take(ctx, cmd, Key::Num1) {
        app.set_view(View::Mail);
    }
    if take(ctx, cmd, Key::Num2) {
        app.set_view(View::Calendar);
    }
    if take(ctx, cmd_shift, Key::M) {
        app.new_mail();
    }
    if take(ctx, cmd_shift, Key::A) {
        app.new_event(false, None);
    }
    if take(ctx, cmd_shift, Key::Q) {
        app.new_event(true, None);
    }
    if take(ctx, cmd, Key::N) {
        match app.view {
            View::Mail => app.new_mail(),
            View::Calendar => app.new_event(false, None),
        }
    }
    match app.view {
        View::Mail => {
            let one = app.mail.selected.len() == 1;
            if one && take(ctx, cmd_shift, Key::R) {
                app.reply("replyAll");
            }
            if one && take(ctx, cmd, Key::R) {
                app.reply("reply");
            }
            if one && !typing && take(ctx, cmd, Key::F) {
                app.reply("forward");
            }
            if take(ctx, cmd, Key::Q) {
                app.set_read_selected(true);
            }
            if take(ctx, cmd, Key::U) {
                app.set_read_selected(false);
            }
            if typing {
                return;
            }
            if take(ctx, Modifiers::NONE, Key::Delete) {
                app.delete_selected();
            }
            if take(ctx, Modifiers::NONE, Key::Backspace) {
                app.archive_selected();
            }
            if take(ctx, Modifiers::NONE, Key::Insert) {
                app.toggle_flag_selected();
            }
            if take(ctx, Modifiers::NONE, Key::ArrowDown) {
                app.step_selection(1, false);
            }
            if take(ctx, Modifiers::NONE, Key::ArrowUp) {
                app.step_selection(-1, false);
            }
            if take(ctx, Modifiers::SHIFT, Key::ArrowDown) {
                app.step_selection(1, true);
            }
            if take(ctx, Modifiers::SHIFT, Key::ArrowUp) {
                app.step_selection(-1, true);
            }
            if take(ctx, Modifiers::NONE, Key::Home) && !app.mail.list.is_empty() {
                let id = app.mail.list[0].id.clone();
                app.select_message(&id, false, false);
                app.mail.scroll_to_selected = true;
            }
            if take(ctx, Modifiers::NONE, Key::End) && !app.mail.list.is_empty() {
                let id = app.mail.list[app.mail.list.len() - 1].id.clone();
                app.select_message(&id, false, false);
                app.mail.scroll_to_selected = true;
            }
            if take(ctx, cmd, Key::A) {
                app.mail.selected = app.mail.list.iter().map(|m| m.id.clone()).collect();
                app.mail.reading = None;
            }
            if take(ctx, Modifiers::NONE, Key::Enter) {
                if let Some(id) = app.mail.selected.first().cloned() {
                    app.open_message_window(&id);
                }
            }
        }
        View::Calendar => {
            if typing {
                return;
            }
            if take(ctx, Modifiers::NONE, Key::Delete) {
                if let Some(id) = app.cal.selected.clone() {
                    app.delete_event(&id);
                }
            }
            if take(ctx, Modifiers::NONE, Key::Enter) {
                if let Some(id) = app.cal.selected.clone() {
                    app.open_event(&id);
                }
            }
            let alt = Modifiers::ALT;
            for (k, v) in [(Key::Num1, CalendarView::Day), (Key::Num2, CalendarView::WorkWeek), (Key::Num3, CalendarView::Week), (Key::Num4, CalendarView::Month)] {
                if take(ctx, cmd | alt, k) {
                    app.cal.set_view(v);
                }
            }
            if take(ctx, Modifiers::NONE, Key::T) {
                app.cal.go_today();
            }
        }
    }
}

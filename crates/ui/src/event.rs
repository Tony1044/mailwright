//! Appointment and meeting windows: create, edit, respond, cancel.

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike};
use egui::{Context, Ui, vec2};
use mailwright_store::{Action, Addr, AttendeeRow, EventRow};
use serde_json::json;

use crate::app::MailApp;
use crate::calendar::{local_ts, today};
use crate::theme::{Tokens, regular, semibold};
use crate::widgets;

pub struct EventEditor {
    pub wid: u64,
    pub id: Option<String>,
    pub calendar_id: String,
    pub subject: String,
    pub location: String,
    pub all_day: bool,
    pub start_date: NaiveDate,
    pub start_min: u32,
    pub end_date: NaiveDate,
    pub end_min: u32,
    pub attendees: String,
    pub meeting: bool,
    pub show_as: String,
    pub reminder: Option<i64>,
    pub online: bool,
    pub join_url: String,
    pub body: String,
    pub body_loaded: bool,
    pub organizer: Option<Addr>,
    pub is_organizer: bool,
    pub response: String,
    pub attendee_rows: Vec<AttendeeRow>,
    pub recurring: bool,
    pub dirty: bool,
    pub open: bool,
    pub error: Option<String>,
    pub confirm_delete: bool,
    /// Calendars it can go in: (id, "name – account").
    pub calendars: Vec<(String, String)>,
}

fn minutes_of(ts: i64) -> (NaiveDate, u32) {
    let t = crate::list::local(ts);
    (t.date_naive(), t.hour() * 60 + t.minute())
}

fn time_label(m: u32) -> String {
    format!("{:02}:{:02}", m / 60, m % 60)
}

impl MailApp {
    /// A new appointment (or meeting), starting at `start` or the next half hour.
    pub fn new_event(&mut self, meeting: bool, start: Option<i64>) {
        let start = start.unwrap_or_else(|| {
            let now = chrono::Local::now().timestamp();
            (now / 1800 + 1) * 1800
        });
        let (sd, sm) = minutes_of(start);
        let (ed, em) = minutes_of(start + 1800);
        let cal = self.cal.calendars.iter().find(|c| c.is_default).or(self.cal.calendars.first()).map(|c| c.id.clone());
        // The active account's default calendar.
        let cal = self.session().and_then(|s| s.store.default_calendar().ok().flatten()).or(cal).unwrap_or_default();
        let wid = self.next_id();
        self.editors.push(EventEditor {
            wid,
            id: None,
            calendar_id: cal,
            subject: String::new(),
            location: String::new(),
            all_day: false,
            start_date: sd,
            start_min: sm,
            end_date: ed,
            end_min: em,
            attendees: String::new(),
            meeting,
            show_as: "busy".into(),
            reminder: Some(self.settings.default_reminder),
            online: false,
            join_url: String::new(),
            body: String::new(),
            body_loaded: true,
            organizer: None,
            is_organizer: true,
            response: "organizer".into(),
            attendee_rows: Vec::new(),
            recurring: false,
            dirty: false,
            open: true,
            error: None,
            confirm_delete: false,
            calendars: self.calendar_choices(),
        });
    }

    /// Every editable calendar, labelled with its account when there's more than one.
    pub fn calendar_choices(&self) -> Vec<(String, String)> {
        let many = self.sessions.len() > 1;
        let mut out = Vec::new();
        for s in &self.sessions {
            for c in s.store.calendars().unwrap_or_default() {
                if c.can_edit {
                    out.push((c.id.clone(), if many { format!("{} \u{2013} {}", c.name, s.label()) } else { c.name.clone() }));
                }
            }
        }
        out
    }

    pub fn open_event(&mut self, id: &str) {
        if self.editors.iter().any(|e| e.id.as_deref() == Some(id)) {
            return;
        }
        let Some(owner) = self.session_for_event(id) else { return };
        let Some(e) = self.sessions[owner].store.event(id).ok().flatten() else { return };
        if !e.local {
            self.send_to(owner, mailwright_sync::Command::OpenEvent(id.to_string()));
        }
        let wid = self.next_id();
        let ed = editor_from(wid, &e);
        self.editors.push(ed);
    }
}

fn editor_from(wid: u64, e: &EventRow) -> EventEditor {
    let (sd, sm, ed, em) = if e.all_day {
        let s = chrono::DateTime::from_timestamp(e.start, 0).map(|t| t.date_naive()).unwrap_or_else(today);
        let en = chrono::DateTime::from_timestamp(e.end, 0).map(|t| t.date_naive()).unwrap_or(s) - Duration::days(1);
        (s, 0, en.max(s), 0)
    } else {
        let (sd, sm) = minutes_of(e.start);
        let (ed, em) = minutes_of(e.end);
        (sd, sm, ed, em)
    };
    let attendees = e.attendees.iter().filter(|a| a.kind != "resource").map(|a| a.addr.full()).collect::<Vec<_>>().join("; ");
    EventEditor {
        wid,
        id: Some(e.id.clone()),
        calendar_id: e.calendar_id.clone(),
        subject: e.subject.clone(),
        location: e.location.clone(),
        all_day: e.all_day,
        start_date: sd,
        start_min: sm,
        end_date: ed,
        end_min: em,
        attendees: if attendees.is_empty() { String::new() } else { format!("{attendees}; ") },
        meeting: !e.attendees.is_empty(),
        show_as: e.show_as.clone(),
        reminder: e.reminder_minutes,
        online: !e.join_url.is_empty(),
        join_url: e.join_url.clone(),
        body: e.body.as_deref().map(mailwright_html::to_text).unwrap_or_else(|| e.preview.clone()),
        body_loaded: e.body.is_some() || e.local,
        organizer: (!e.organizer.address.is_empty()).then(|| e.organizer.clone()),
        is_organizer: e.is_organizer || e.local,
        response: e.response.clone(),
        attendee_rows: e.attendees.clone(),
        recurring: e.kind == "occurrence" || e.kind == "exception",
        dirty: false,
        open: true,
        error: None,
        confirm_delete: false,
        calendars: Vec::new(),
    }
}

impl EventEditor {
    fn start_ts(&self) -> i64 {
        local_ts(self.start_date, self.start_min / 60, self.start_min % 60)
    }
    fn end_ts(&self) -> i64 {
        local_ts(self.end_date, self.end_min / 60, self.end_min % 60)
    }

    fn graph_time(ts: i64) -> String {
        chrono::DateTime::from_timestamp(ts, 0).unwrap_or_default().format("%Y-%m-%dT%H:%M:%S").to_string()
    }

    /// The Graph body for this appointment.
    fn body_json(&self, font: &str, size: f32) -> Result<serde_json::Value, String> {
        let attendees = crate::compose::parse_recipients(&self.attendees)?;
        let (start, end) = if self.all_day {
            let s = self.start_date.and_time(NaiveTime::MIN).and_utc().timestamp();
            let e = (self.end_date.max(self.start_date) + Duration::days(1)).and_time(NaiveTime::MIN).and_utc().timestamp();
            (s, e)
        } else {
            (self.start_ts(), self.end_ts())
        };
        if end <= start {
            return Err("The end time is before the start time.".into());
        }
        let mut v = json!({
            "subject": self.subject.trim(),
            "body": {"contentType": "html", "content": mailwright_html::text_to_html(&self.body, font, size)},
            "start": {"dateTime": Self::graph_time(start), "timeZone": "UTC"},
            "end": {"dateTime": Self::graph_time(end), "timeZone": "UTC"},
            "isAllDay": self.all_day,
            "location": {"displayName": self.location.trim()},
            "showAs": self.show_as,
            "isReminderOn": self.reminder.is_some(),
            "reminderMinutesBeforeStart": self.reminder.unwrap_or(0),
            "attendees": attendees.iter().map(|a| json!({"emailAddress": {"address": a.address, "name": a.name}, "type": "required"})).collect::<Vec<_>>(),
            "isOnlineMeeting": self.online,
        });
        if self.online {
            v["onlineMeetingProvider"] = json!("teamsForBusiness");
        }
        Ok(v)
    }
}

enum Do {
    Save,
    Delete,
    Respond(&'static str),
    Close,
}

pub fn windows(app: &mut MailApp, ctx: &Context) {
    let mut i = 0;
    while i < app.editors.len() {
        if !app.editors[i].open {
            app.editors.remove(i);
            continue;
        }
        // Pick up the body once it has downloaded.
        if !app.editors[i].body_loaded && !app.editors[i].dirty {
            if let Some(id) = app.editors[i].id.clone() {
                if let Some(e) = app.session_for_event(&id).and_then(|i| app.sessions[i].store.event(&id).ok().flatten()) {
                    if let Some(b) = e.body {
                        app.editors[i].body = mailwright_html::to_text(&b);
                        app.editors[i].body_loaded = true;
                    }
                }
            }
        }
        let wid = app.editors[i].wid;
        let kind = if app.editors[i].meeting { "Meeting" } else { "Appointment" };
        let title = format!("{} \u{2013} {kind}", if app.editors[i].subject.trim().is_empty() { "Untitled" } else { app.editors[i].subject.trim() });
        let mut todo = Vec::new();
        let builder = egui::ViewportBuilder::default().with_title(title).with_inner_size([760.0, 640.0]).with_min_inner_size([520.0, 420.0]);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(("event", wid)), builder, |ui, class| {
            let t = Tokens::get(ui.ctx());
            if class == egui::ViewportClass::EmbeddedWindow {
                ui.set_min_size(vec2(640.0, 480.0));
            }
            if ui.ctx().input(|inp| inp.viewport().close_requested()) {
                todo.push(Do::Close);
            }
            let ed = &mut app.editors[i];
            egui::Panel::top("event_ribbon")
                .frame(egui::Frame::new().fill(t.ribbon).inner_margin(egui::Margin::symmetric(6, 4)).stroke(egui::Stroke::new(1.0, t.border)))
                .show(ui, |ui| event_ribbon(ui, ed, &mut todo));
            egui::CentralPanel::default().frame(egui::Frame::new().fill(t.reading).inner_margin(egui::Margin::symmetric(14, 10))).show(ui, |ui| event_form(ui, ed, &t));
            if ed.confirm_delete {
                egui::Modal::new(egui::Id::new(("confirm_del", wid))).show(ui.ctx(), |ui| {
                    ui.set_width(360.0);
                    let q = if ed.is_organizer && ed.meeting { "Cancel this meeting and tell the attendees?" } else { "Delete this appointment?" };
                    ui.label(egui::RichText::new(q).font(semibold(14.0)));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if widgets::primary(ui, "Yes").clicked() {
                            todo.push(Do::Delete);
                        }
                        if widgets::secondary(ui, "No").clicked() {
                            ed.confirm_delete = false;
                        }
                    });
                });
            }
        });
        for d in todo {
            match d {
                Do::Save => app.save_event(i),
                Do::Delete => {
                    if let Some(id) = app.editors[i].id.clone() {
                        app.delete_event(&id);
                    }
                    app.editors[i].open = false;
                }
                Do::Respond(r) => {
                    if let Some(id) = app.editors[i].id.clone() {
                        app.act_for_event(&id.clone(), Action::RespondEvent { id, response: r.into(), comment: String::new(), send: true });
                    }
                    app.editors[i].open = false;
                }
                Do::Close => app.editors[i].open = false,
            }
        }
        i += 1;
    }
}

impl MailApp {
    fn save_event(&mut self, i: usize) {
        let (font, size) = (self.settings.compose_font.clone(), self.settings.compose_size);
        let ed = &self.editors[i];
        let body = match ed.body_json(&font, size) {
            Ok(b) => b,
            Err(e) => {
                self.editors[i].error = Some(e);
                return;
            }
        };
        if ed.subject.trim().is_empty() && ed.meeting {
            self.editors[i].error = Some("Give the meeting a subject.".into());
            return;
        }
        match ed.id.clone() {
            Some(id) => {
                self.act_for_event(&id.clone(), Action::UpdateEvent { id, patch: body });
            }
            None => {
                let owner = self.session_for_calendar(&self.editors[i].calendar_id).or(Some(self.active));
                let local_id = format!("local:ev-{}", self.next_id());
                let ed = &self.editors[i];
                let (start, end) = if ed.all_day {
                    (ed.start_date.and_time(NaiveTime::MIN).and_utc().timestamp(), (ed.end_date.max(ed.start_date) + Duration::days(1)).and_time(NaiveTime::MIN).and_utc().timestamp())
                } else {
                    (ed.start_ts(), ed.end_ts())
                };
                let row = EventRow {
                    id: local_id.clone(),
                    calendar_id: ed.calendar_id.clone(),
                    subject: ed.subject.trim().to_string(),
                    start,
                    end,
                    all_day: ed.all_day,
                    location: ed.location.clone(),
                    organizer: owner.and_then(|i| self.sessions.get(i)).map(|s| Addr::new(&s.name, &s.address)).unwrap_or_default(),
                    show_as: ed.show_as.clone(),
                    response: "organizer".into(),
                    is_organizer: true,
                    kind: "singleInstance".into(),
                    reminder_minutes: ed.reminder,
                    preview: ed.body.chars().take(200).collect(),
                    local: true,
                    ..Default::default()
                };
                if let Some(s) = owner.and_then(|i| self.sessions.get(i)) {
                    let _ = s.store.insert_local_event(&row);
                }
                let calendar_id = ed.calendar_id.clone();
                match owner {
                    Some(i) => self.send_to(i, mailwright_sync::Command::Do(Action::CreateEvent { local_id, calendar_id, body })),
                    None => self.act(Action::CreateEvent { local_id, calendar_id, body }),
                }
                self.cal.dirty = true;
            }
        }
        self.editors[i].open = false;
    }
}

fn event_ribbon(ui: &mut Ui, ed: &mut EventEditor, todo: &mut Vec<Do>) {
    ui.horizontal(|ui| {
        if ed.is_organizer {
            widgets::group(ui, "Actions", |ui| {
                let label = if ed.meeting && !ed.attendees.trim().is_empty() { if ed.id.is_some() { "Send\nUpdate" } else { "Send" } } else { "Save &\nClose" };
                if widgets::big(ui, if ed.meeting { "send" } else { "save" }, label, true, false, false).clicked() {
                    todo.push(Do::Save);
                }
                if widgets::big(ui, "delete", if ed.meeting && ed.id.is_some() { "Cancel\nMeeting" } else { "Delete" }, ed.id.is_some(), false, false).clicked() && ed.id.is_some() {
                    ed.confirm_delete = true;
                }
            });
            widgets::group(ui, "Attendees", |ui| {
                if widgets::big(ui, "new_meeting", "Invite\nAttendees", true, ed.meeting, false).clicked() {
                    ed.meeting = !ed.meeting;
                }
            });
            widgets::group(ui, "Online Meeting", |ui| {
                if widgets::big(ui, "video", "Online\nMeeting", true, ed.online, false).on_hover_text("Add a Teams meeting link").clicked() {
                    ed.online = !ed.online;
                    ed.dirty = true;
                }
            });
        } else {
            widgets::group(ui, "Respond", |ui| {
                for (icon, label, r) in [("accept", "Accept", "accept"), ("tentative", "Tentative", "tentative"), ("decline", "Decline", "decline")] {
                    if widgets::big(ui, icon, label, true, false, false).clicked() {
                        todo.push(Do::Respond(r));
                    }
                }
            });
            widgets::group(ui, "Actions", |ui| {
                if widgets::big(ui, "close", "Close", true, false, false).clicked() {
                    todo.push(Do::Close);
                }
            });
        }
        widgets::group(ui, "Options", |ui| {
            widgets::stack(ui, |ui| {
                let show = match ed.show_as.as_str() {
                    "free" => "Free",
                    "tentative" => "Tentative",
                    "oof" => "Away",
                    "workingElsewhere" => "Working Elsewhere",
                    _ => "Busy",
                };
                let r = widgets::small(ui, "clock", &format!("Show As: {show}"), ed.is_organizer, false, true);
                if ed.is_organizer {
                    egui::Popup::menu(&r).show(|ui| {
                        for (v, l) in [("free", "Free"), ("workingElsewhere", "Working Elsewhere"), ("tentative", "Tentative"), ("busy", "Busy"), ("oof", "Away")] {
                            if ui.selectable_label(ed.show_as == v, l).clicked() {
                                ed.show_as = v.into();
                                ed.dirty = true;
                                ui.close();
                            }
                        }
                    });
                }
                let rem = match ed.reminder {
                    None => "None".to_string(),
                    Some(0) => "At start".into(),
                    Some(m) if m % 1440 == 0 => format!("{} day{}", m / 1440, if m / 1440 == 1 { "" } else { "s" }),
                    Some(m) if m % 60 == 0 => format!("{} hour{}", m / 60, if m / 60 == 1 { "" } else { "s" }),
                    Some(m) => format!("{m} minutes"),
                };
                let r = widgets::small(ui, "bell", &format!("Reminder: {rem}"), ed.is_organizer, false, true);
                if ed.is_organizer {
                    egui::Popup::menu(&r).show(|ui| {
                        for (v, l) in [(None, "None"), (Some(0), "At start"), (Some(5), "5 minutes"), (Some(15), "15 minutes"), (Some(30), "30 minutes"), (Some(60), "1 hour"), (Some(120), "2 hours"), (Some(1440), "1 day"), (Some(10080), "1 week")] {
                            if ui.selectable_label(ed.reminder == v, l).clicked() {
                                ed.reminder = v;
                                ed.dirty = true;
                                ui.close();
                            }
                        }
                    });
                }
            });
        });
    });
}

fn date_picker(ui: &mut Ui, id: egui::Id, date: &mut NaiveDate, enabled: bool) -> bool {
    let mut changed = false;
    let resp = ui.add_enabled(enabled, egui::Button::new(date.format("%a %d/%m/%Y").to_string()).min_size(vec2(130.0, 26.0)));
    egui::Popup::menu(&resp).id(id).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let t = Tokens::get(ui.ctx());
        let month_key = id.with("month");
        let mut month: NaiveDate = ui.data(|d| d.get_temp(month_key)).unwrap_or_else(|| date.with_day(1).unwrap_or(*date));
        ui.horizontal(|ui| {
            if ui.small_button("\u{2039}").clicked() {
                month = (month - Duration::days(1)).with_day(1).unwrap_or(month);
            }
            ui.label(egui::RichText::new(month.format("%B %Y").to_string()).font(semibold(12.5)));
            if ui.small_button("\u{203a}").clicked() {
                month = (month + Duration::days(32)).with_day(1).unwrap_or(month);
            }
        });
        let start = month - Duration::days(month.weekday().num_days_from_monday() as i64);
        egui::Grid::new(id.with("grid")).spacing(vec2(2.0, 2.0)).show(ui, |ui| {
            for name in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
                ui.label(egui::RichText::new(name).font(regular(11.0)).color(t.text_dim));
            }
            ui.end_row();
            for w in 0..6 {
                for d in 0..7 {
                    let day = start + Duration::days(w * 7 + d);
                    let text = egui::RichText::new(day.day().to_string()).color(if day.month() == month.month() { t.text } else { t.text_disabled });
                    if ui.add(egui::Button::selectable(day == *date, text).min_size(vec2(26.0, 22.0))).clicked() {
                        *date = day;
                        changed = true;
                        ui.close();
                    }
                }
                ui.end_row();
            }
        });
        ui.data_mut(|d| d.insert_temp(month_key, month));
    });
    changed
}

fn time_picker(ui: &mut Ui, id: egui::Id, minutes: &mut u32, enabled: bool) -> bool {
    let before = *minutes;
    ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::from_id_salt(id).width(80.0).selected_text(time_label(*minutes)).height(320.0).show_ui(ui, |ui| {
            for m in (0..24 * 60).step_by(30) {
                ui.selectable_value(minutes, m as u32, time_label(m as u32));
            }
        });
    });
    before != *minutes
}

fn event_form(ui: &mut Ui, ed: &mut EventEditor, t: &Tokens) {
    let edit = ed.is_organizer;
    if let Some(e) = ed.error.clone() {
        if let Some(r) = widgets::info_bar(ui, "info", &e, true, Some("Dismiss")) {
            if r.clicked() {
                ed.error = None;
            }
        }
        ui.add_space(4.0);
    }
    if !ed.is_organizer {
        let who = ed.organizer.as_ref().map(|o| o.label().to_string()).unwrap_or_default();
        let state = match ed.response.as_str() {
            "accepted" => "You accepted this meeting.",
            "tentativelyAccepted" => "You tentatively accepted this meeting.",
            "declined" => "You declined this meeting.",
            _ => "Please respond.",
        };
        widgets::info_bar(ui, "people", &format!("Organiser: {who}. {state}"), false, None);
        ui.add_space(4.0);
    }
    if ed.recurring {
        widgets::info_bar(ui, "recurring", "This is one occurrence of a series. Changes here apply to this occurrence only.", false, None);
        ui.add_space(4.0);
    }
    let label_w = 80.0;
    let row = |ui: &mut Ui, label: &str, add: &mut dyn FnMut(&mut Ui)| {
        ui.horizontal(|ui| {
            ui.add_sized(vec2(label_w, 26.0), egui::Label::new(egui::RichText::new(label).color(t.text_dim)));
            add(ui);
        });
    };
    let mut dirty = false;
    if ed.id.is_none() && ed.calendars.len() > 1 {
        row(ui, "Calendar", &mut |ui| {
            let current = ed.calendars.iter().find(|(id, _)| *id == ed.calendar_id).map(|(_, l)| l.clone()).unwrap_or_default();
            egui::ComboBox::from_id_salt(("cal", ed.wid)).selected_text(current).width(320.0).show_ui(ui, |ui| {
                for (id, label) in ed.calendars.clone() {
                    ui.selectable_value(&mut ed.calendar_id, id, label);
                }
            });
        });
    }
    row(ui, "Title", &mut |ui| {
        dirty |= ui.add_enabled(edit, egui::TextEdit::singleline(&mut ed.subject).desired_width(f32::INFINITY).margin(vec2(6.0, 4.0)).hint_text("Add a title")).changed();
    });
    if ed.meeting {
        row(ui, "Required", &mut |ui| {
            dirty |= ui.add_enabled(edit, egui::TextEdit::singleline(&mut ed.attendees).desired_width(f32::INFINITY).margin(vec2(6.0, 4.0)).hint_text("Invite attendees: names; addresses")).changed();
        });
        if !ed.attendee_rows.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(label_w + 6.0);
                for a in &ed.attendee_rows {
                    let (icon, col) = match a.response.as_str() {
                        "accepted" => ("accept", t.green),
                        "declined" => ("decline", t.high),
                        "tentativelyAccepted" => ("tentative", t.text_dim),
                        _ => ("person", t.text_dim),
                    };
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                    crate::icons::paint(ui.painter(), r, icon, col, col);
                    ui.label(egui::RichText::new(a.addr.label()).font(regular(11.5)));
                    ui.add_space(6.0);
                }
            });
        }
    }
    row(ui, "Start time", &mut |ui| {
        let before = (ed.start_date, ed.start_min);
        let len = ed.end_ts() - ed.start_ts();
        dirty |= date_picker(ui, egui::Id::new(("sd", ed.wid)), &mut ed.start_date, edit);
        if !ed.all_day {
            dirty |= time_picker(ui, egui::Id::new(("st", ed.wid)), &mut ed.start_min, edit);
        }
        if (ed.start_date, ed.start_min) != before {
            // Keep the length when the start moves.
            let new_end = ed.start_ts() + len.max(0);
            let (d, m) = minutes_of(new_end);
            ed.end_date = d;
            ed.end_min = m;
        }
        if ui.add_enabled(edit, egui::Checkbox::new(&mut ed.all_day, "All day")).changed() {
            dirty = true;
            if ed.all_day {
                ed.show_as = "free".into();
                ed.end_date = ed.end_date.max(ed.start_date);
            }
        }
    });
    row(ui, "End time", &mut |ui| {
        dirty |= date_picker(ui, egui::Id::new(("edp", ed.wid)), &mut ed.end_date, edit);
        if !ed.all_day {
            dirty |= time_picker(ui, egui::Id::new(("et", ed.wid)), &mut ed.end_min, edit);
        }
        if !ed.all_day {
            let mins = (ed.end_ts() - ed.start_ts()) / 60;
            if mins > 0 {
                let text = if mins % 60 == 0 { format!("{} h", mins / 60) } else if mins > 60 { format!("{} h {} min", mins / 60, mins % 60) } else { format!("{mins} min") };
                ui.label(egui::RichText::new(text).color(t.text_dim));
            }
        }
    });
    row(ui, "Location", &mut |ui| {
        dirty |= ui.add_enabled(edit, egui::TextEdit::singleline(&mut ed.location).desired_width(f32::INFINITY).margin(vec2(6.0, 4.0)).hint_text("Add a location")).changed();
    });
    if !ed.join_url.is_empty() {
        row(ui, "Online", &mut |ui| {
            if ui.link("Join the online meeting").clicked() {
                let _ = open::that_detached(&ed.join_url);
            }
        });
    }
    ui.add_space(8.0);
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        dirty |= ui
            .add_enabled(edit, egui::TextEdit::multiline(&mut ed.body).desired_width(f32::INFINITY).desired_rows(10).frame(egui::Frame::NONE).font(egui::FontId::proportional(14.0)).hint_text("Add a description"))
            .changed();
    });
    if dirty {
        ed.dirty = true;
    }
    if !ed.all_day && ed.end_ts() <= ed.start_ts() {
        ed.end_date = ed.start_date;
        ed.end_min = (ed.start_min + 30).min(24 * 60 - 1);
        if ed.end_min <= ed.start_min {
            ed.end_date = ed.start_date + Duration::days(1);
            ed.end_min = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_day_events_go_to_graph_as_whole_utc_days() {
        let mut ed = editor_from(1, &EventRow { id: "x".into(), start: 0, end: 3600, is_organizer: true, ..Default::default() });
        ed.all_day = true;
        ed.start_date = NaiveDate::from_ymd_opt(2026, 10, 30).unwrap();
        ed.end_date = ed.start_date;
        ed.attendees = "Ada <ada@x.example>; ".into();
        let v = ed.body_json("Calibri", 11.0).unwrap();
        assert_eq!(v["start"]["dateTime"], "2026-10-30T00:00:00");
        assert_eq!(v["end"]["dateTime"], "2026-10-31T00:00:00");
        assert_eq!(v["attendees"][0]["emailAddress"]["address"], "ada@x.example");
        ed.attendees = "nonsense".into();
        assert!(ed.body_json("Calibri", 11.0).is_err());
    }
}

impl MailApp {
    /// Save appointment window `i` (what its Save & Close button does).
    #[doc(hidden)]
    pub fn test_save_event(&mut self, i: usize) {
        self.save_event(i);
    }
}

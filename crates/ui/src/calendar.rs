//! The calendar: date navigator, calendars, and Day / Work Week / Week / Month views.

use std::collections::HashSet;

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone};
use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, pos2, vec2};
use mailcraft_store::{Action, CalendarRow, EventRow, Store};
use mailcraft_sync::{Command, SyncHandle};

use crate::app::MailApp;
use crate::settings::{CalendarView, Settings};
use crate::theme::{Tokens, medium, regular, semibold};
use crate::widgets;

const SLOT_H: f32 = 22.0;
const TIME_GUTTER: f32 = 56.0;
const DAY_HEADER_H: f32 = 44.0;

pub struct CalState {
    pub view: CalendarView,
    /// "Next 7 Days".
    pub seven: bool,
    pub anchor: NaiveDate,
    pub nav_month: NaiveDate,
    pub events: Vec<EventRow>,
    pub calendars: Vec<CalendarRow>,
    pub hidden: HashSet<String>,
    pub dirty: bool,
    pub selected: Option<String>,
    requested: Option<(i64, i64)>,
    scrolled: bool,
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Local midnight of a date, as Unix seconds.
pub fn local_ts(d: NaiveDate, h: u32, m: u32) -> i64 {
    let naive = d.and_time(NaiveTime::from_hms_opt(h.min(23), m.min(59), 0).unwrap_or_default());
    Local.from_local_datetime(&naive).earliest().map(|t| t.timestamp()).unwrap_or_else(|| naive.and_utc().timestamp())
}

impl CalState {
    pub fn new(view: CalendarView) -> CalState {
        let t = today();
        CalState {
            view,
            seven: false,
            anchor: t,
            nav_month: t.with_day(1).unwrap_or(t),
            events: Vec::new(),
            calendars: Vec::new(),
            hidden: HashSet::new(),
            dirty: true,
            selected: None,
            requested: None,
            scrolled: false,
        }
    }

    fn week_start(&self, d: NaiveDate, settings_start: u32) -> NaiveDate {
        let wd = d.weekday().num_days_from_monday();
        let back = (wd + 7 - settings_start.min(6)) % 7;
        d - Duration::days(back as i64)
    }

    /// The first day shown and how many days.
    pub fn days(&self, s: &Settings) -> (NaiveDate, i64) {
        if self.seven {
            return (self.anchor, 7);
        }
        match self.view {
            CalendarView::Day => (self.anchor, 1),
            CalendarView::WorkWeek => (self.week_start(self.anchor, 0), 5),
            CalendarView::Week => (self.week_start(self.anchor, s.week_start), 7),
            CalendarView::Month => {
                let first = self.anchor.with_day(1).unwrap_or(self.anchor);
                let start = self.week_start(first, s.week_start);
                let next = if first.month() == 12 { NaiveDate::from_ymd_opt(first.year() + 1, 1, 1) } else { NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1) }.unwrap_or(first);
                let span = (next - start).num_days();
                (start, ((span + 6) / 7) * 7)
            }
        }
    }

    pub fn range(&self, s: &Settings) -> (i64, i64) {
        let (start, n) = self.days(s);
        // A day either side: all-day events are stored at UTC midnight.
        (local_ts(start, 0, 0) - 86_400, local_ts(start + Duration::days(n), 0, 0) + 86_400)
    }

    pub fn range_title(&self, s: &Settings) -> String {
        let (start, n) = self.days(s);
        let end = start + Duration::days(n - 1);
        if self.view == CalendarView::Month && !self.seven {
            return self.anchor.format("%B %Y").to_string();
        }
        if n == 1 {
            return start.format("%A %-d %B %Y").to_string();
        }
        if start.month() == end.month() {
            format!("{}\u{2013}{} {}", start.day(), end.day(), end.format("%B %Y"))
        } else if start.year() == end.year() {
            format!("{} \u{2013} {}", start.format("%-d %B"), end.format("%-d %B %Y"))
        } else {
            format!("{} \u{2013} {}", start.format("%-d %B %Y"), end.format("%-d %B %Y"))
        }
    }

    pub fn request_window(&mut self, syncs: &[&SyncHandle], s: &Settings) {
        let r = self.range(s);
        if self.requested != Some(r) {
            self.requested = Some(r);
            self.dirty = true;
            for h in syncs {
                h.send(Command::CalendarWindow { from: r.0, to: r.1 });
            }
        }
    }

    /// Ask every account for the window again (one was added).
    pub fn forget_window(&mut self) {
        self.requested = None;
    }

    pub fn reload(&mut self, stores: &[&Store]) {
        self.dirty = false;
        let (from, to) = self.requested.unwrap_or((0, 0));
        self.calendars.clear();
        self.events.clear();
        for store in stores {
            self.calendars.extend(store.calendars().unwrap_or_default());
            self.events.extend(store.events(from, to).unwrap_or_default().into_iter().filter(|e| !self.hidden.contains(&e.calendar_id)));
        }
        self.events.sort_by(|a, b| (!a.all_day, a.start, -a.end).cmp(&(!b.all_day, b.start, -b.end)));
    }

    pub fn go_today(&mut self) {
        self.anchor = today();
        self.nav_month = self.anchor.with_day(1).unwrap_or(self.anchor);
        self.seven = false;
        self.scrolled = false;
    }

    pub fn next_seven(&mut self) {
        self.anchor = today();
        self.seven = true;
    }

    pub fn set_view(&mut self, v: CalendarView) {
        self.view = v;
        self.seven = false;
    }

    fn step(&mut self, dir: i64) {
        self.anchor = match (self.view, self.seven) {
            (_, true) => self.anchor + Duration::days(7 * dir),
            (CalendarView::Day, _) => self.anchor + Duration::days(dir),
            (CalendarView::WorkWeek | CalendarView::Week, _) => self.anchor + Duration::days(7 * dir),
            (CalendarView::Month, _) => add_months(self.anchor, dir),
        };
        self.nav_month = self.anchor.with_day(1).unwrap_or(self.anchor);
    }
}

fn add_months(d: NaiveDate, n: i64) -> NaiveDate {
    let total = d.year() as i64 * 12 + d.month0() as i64 + n;
    let (y, m) = ((total.div_euclid(12)) as i32, (total.rem_euclid(12) + 1) as u32);
    let last = (28..=31).rev().find(|day| NaiveDate::from_ymd_opt(y, m, *day).is_some()).unwrap_or(28);
    NaiveDate::from_ymd_opt(y, m, d.day().min(last)).unwrap_or(d)
}

/// When an event is, in words ("Wednesday 22 October 2026 10:00–11:30").
pub fn when_text(e: &EventRow) -> String {
    if e.all_day {
        let start = chrono::DateTime::from_timestamp(e.start, 0).map(|t| t.date_naive()).unwrap_or_default();
        let end = chrono::DateTime::from_timestamp(e.end, 0).map(|t| t.date_naive()).unwrap_or_default() - Duration::days(1);
        if end <= start {
            return format!("{} (all day)", start.format("%A %-d %B %Y"));
        }
        return format!("{} \u{2013} {} (all day)", start.format("%A %-d %B"), end.format("%A %-d %B %Y"));
    }
    let s = crate::list::local(e.start);
    let en = crate::list::local(e.end);
    if s.date_naive() == en.date_naive() {
        format!("{} {}\u{2013}{}", s.format("%A %-d %B %Y"), s.format("%H:%M"), en.format("%H:%M"))
    } else {
        format!("{} \u{2013} {}", s.format("%a %-d %b %Y %H:%M"), en.format("%a %-d %b %Y %H:%M"))
    }
}

pub fn event_color(e: &EventRow, cals: &[CalendarRow], t: &Tokens) -> Color32 {
    if let Some(c) = e.categories.first() {
        return crate::theme::category_color(c);
    }
    cals.iter()
        .find(|c| c.id == e.calendar_id)
        .and_then(|c| mailcraft_html::parse_color(&c.color))
        .map(|c| Color32::from_rgb(c[0], c[1], c[2]))
        .unwrap_or(t.accent)
}

fn tint(c: Color32, t: &Tokens, amount: f32) -> Color32 {
    let base = if t.dark { Color32::from_rgb(0x20, 0x20, 0x20) } else { Color32::WHITE };
    let mix = |a: u8, b: u8| (a as f32 * amount + b as f32 * (1.0 - amount)) as u8;
    Color32::from_rgb(mix(c.r(), base.r()), mix(c.g(), base.g()), mix(c.b(), base.b()))
}

pub fn view(app: &mut MailApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let syncs: Vec<&SyncHandle> = app.sessions.iter().map(|s| &s.sync).collect();
    app.cal.request_window(&syncs, &app.settings);
    if app.cal.dirty {
        let stores: Vec<&Store> = app.sessions.iter().map(|s| &s.store).collect();
        app.cal.reload(&stores);
    }
    egui::Panel::left("cal_nav").resizable(false).exact_size(app.settings.folder_pane_width.clamp(200.0, 300.0)).frame(egui::Frame::new().fill(t.panel)).show(ui, |ui| {
        egui::Panel::bottom("cal_navbar").exact_size(44.0).frame(egui::Frame::new()).show(ui, |ui| crate::folders::nav_bar(app, ui, false));
        egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(egui::Margin::same(8))).show(ui, |ui| {
            navigator(app, ui, &t);
            ui.add_space(14.0);
            ui.label(egui::RichText::new("My Calendars").font(semibold(12.5)));
            ui.add_space(4.0);
            let cals = app.cal.calendars.clone();
            for c in cals {
                let mut on = !app.cal.hidden.contains(&c.id);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut on, "").changed() {
                        if on {
                            app.cal.hidden.remove(&c.id);
                        } else {
                            app.cal.hidden.insert(c.id.clone());
                        }
                        app.cal.dirty = true;
                    }
                    let col = mailcraft_html::parse_color(&c.color).map(|c| Color32::from_rgb(c[0], c[1], c[2])).unwrap_or(t.accent);
                    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, col);
                    ui.label(&c.name);
                });
            }
        });
    });
    egui::CentralPanel::default().frame(egui::Frame::new().fill(t.reading)).show(ui, |ui| {
        header(app, ui, &t);
        if app.cal.view == CalendarView::Month && !app.cal.seven {
            month(app, ui, &t);
        } else {
            time_grid(app, ui, &t);
        }
    });
}

fn navigator(app: &mut MailApp, ui: &mut Ui, t: &Tokens) {
    let month = app.cal.nav_month;
    ui.horizontal(|ui| {
        if widgets::icon_button(ui, "chevron_left", "Previous month", true).clicked() {
            app.cal.nav_month = add_months(month, -1);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, "chevron_right", "Next month", true).clicked() {
                app.cal.nav_month = add_months(month, 1);
            }
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new(month.format("%B %Y").to_string()).font(semibold(12.5)));
            });
        });
    });
    let w = ui.available_width();
    let cell = (w / 7.0).floor();
    let start = app.cal.week_start(month, app.settings.week_start);
    let (shown_start, shown_n) = app.cal.days(&app.settings);
    let busy: HashSet<NaiveDate> = app.cal.events.iter().map(|e| crate::list::local(e.start).date_naive()).collect();
    let (hr, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
    for i in 0..7 {
        let d = start + Duration::days(i);
        let name: String = d.format("%a").to_string().chars().take(2).collect();
        ui.painter().text(pos2(hr.min.x + cell * (i as f32 + 0.5), hr.center().y), Align2::CENTER_CENTER, name, regular(11.0), t.text_dim);
    }
    for week in 0..6 {
        let (rr, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
        for i in 0..7 {
            let d = start + Duration::days(week * 7 + i);
            let r = Rect::from_min_size(pos2(rr.min.x + cell * i as f32, rr.min.y), vec2(cell, 24.0));
            let resp = ui.interact(r, ui.id().with(("navday", d)), Sense::click());
            let in_range = d >= shown_start && d < shown_start + Duration::days(shown_n);
            if in_range {
                ui.painter().rect_filled(r.shrink(1.0), 3.0, t.checked);
            } else if resp.hovered() {
                ui.painter().rect_filled(r.shrink(1.0), 3.0, t.hover);
            }
            let col = if d.month() != month.month() { t.text_disabled } else { t.text };
            if d == today() {
                ui.painter().circle_filled(r.center(), 10.0, t.today);
            }
            let font = if busy.contains(&d) { semibold(11.5) } else { regular(11.5) };
            ui.painter().text(r.center(), Align2::CENTER_CENTER, d.day().to_string(), font, if d == today() { t.on_accent } else { col });
            if resp.clicked() {
                app.cal.anchor = d;
                if app.cal.view != CalendarView::Month {
                    app.cal.seven = false;
                }
            }
        }
    }
}

fn header(app: &mut MailApp, ui: &mut Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, t.border));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(12.0, 6.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut child;
    if widgets::icon_button(ui, "chevron_left", "Back", true).clicked() {
        app.cal.step(-1);
    }
    if widgets::icon_button(ui, "chevron_right", "Forward", true).clicked() {
        app.cal.step(1);
    }
    ui.add_space(6.0);
    if widgets::secondary(ui, "Today").clicked() {
        app.cal.go_today();
    }
    ui.add_space(10.0);
    ui.label(egui::RichText::new(app.cal.range_title(&app.settings)).font(semibold(17.0)));
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        for v in [CalendarView::Month, CalendarView::Week, CalendarView::WorkWeek, CalendarView::Day] {
            let on = app.cal.view == v && !app.cal.seven;
            if ui.selectable_label(on, v.label()).clicked() {
                app.cal.set_view(v);
                app.save_settings();
            }
        }
    });
}

/// Events in a day, laid out side by side where they overlap: (event, column, columns).
fn columns(events: &[&EventRow]) -> Vec<(usize, usize, usize)> {
    let mut idx: Vec<usize> = (0..events.len()).collect();
    idx.sort_by_key(|&i| (events[i].start, -(events[i].end)));
    let mut out = vec![(0, 0, 1); events.len()];
    let mut cluster: Vec<usize> = Vec::new();
    let mut cols_end: Vec<i64> = Vec::new();
    let mut cluster_end = i64::MIN;
    let flush = |cluster: &mut Vec<usize>, cols_end: &mut Vec<i64>, out: &mut Vec<(usize, usize, usize)>| {
        let n = cols_end.len().max(1);
        for &i in cluster.iter() {
            out[i].2 = n;
        }
        cluster.clear();
        cols_end.clear();
    };
    for i in idx {
        let e = events[i];
        let end = e.end.max(e.start + 15 * 60);
        if e.start >= cluster_end && !cluster.is_empty() {
            flush(&mut cluster, &mut cols_end, &mut out);
        }
        let col = match cols_end.iter().position(|&ce| ce <= e.start) {
            Some(c) => {
                cols_end[c] = end;
                c
            }
            None => {
                cols_end.push(end);
                cols_end.len() - 1
            }
        };
        out[i] = (i, col, 1);
        cluster.push(i);
        cluster_end = cluster_end.max(end);
    }
    flush(&mut cluster, &mut cols_end, &mut out);
    out
}

fn time_grid(app: &mut MailApp, ui: &mut Ui, t: &Tokens) {
    let (start, n) = app.cal.days(&app.settings);
    let area = ui.available_rect_before_wrap();
    let col_w = ((area.width() - TIME_GUTTER - 14.0) / n as f32).max(40.0);
    // Day headers and the all-day row.
    let all_day: Vec<EventRow> = app.cal.events.iter().filter(|e| e.all_day || e.end - e.start >= 86_400).cloned().collect();
    let rows_all_day = (0..n)
        .map(|d| {
            let day = start + Duration::days(d);
            all_day.iter().filter(|e| covers_day(e, day)).count()
        })
        .max()
        .unwrap_or(0);
    let all_day_h = (rows_all_day as f32 * 22.0 + 4.0).max(12.0);
    let (hr, _) = ui.allocate_exact_size(vec2(area.width(), DAY_HEADER_H + all_day_h), Sense::hover());
    for d in 0..n {
        let day = start + Duration::days(d);
        let x = hr.min.x + TIME_GUTTER + col_w * d as f32;
        let is_today = day == today();
        ui.painter().text(pos2(x + 8.0, hr.min.y + 12.0), Align2::LEFT_CENTER, day.format("%A").to_string(), regular(11.5), if is_today { t.accent_text } else { t.text_dim });
        let num = day.day().to_string();
        if is_today {
            ui.painter().circle_filled(pos2(x + 19.0, hr.min.y + 31.0), 13.0, t.today);
        }
        ui.painter().text(pos2(x + 19.0, hr.min.y + 31.0), Align2::CENTER_CENTER, num, semibold(16.0), if is_today { t.on_accent } else { t.text });
        ui.painter().line_segment([pos2(x, hr.min.y + 4.0), pos2(x, hr.max.y)], Stroke::new(1.0, t.grid_line));
        // All-day events.
        let mut row = 0;
        for e in all_day.iter().filter(|e| covers_day(e, day)) {
            let r = Rect::from_min_size(pos2(x + 2.0, hr.min.y + DAY_HEADER_H + row as f32 * 22.0), vec2(col_w - 4.0, 20.0));
            event_chip(app, ui, r, e, t, true);
            row += 1;
        }
    }
    ui.painter().line_segment([pos2(hr.min.x, hr.max.y), pos2(hr.max.x, hr.max.y)], Stroke::new(1.0, t.border_strong));
    // The hours.
    let total_h = SLOT_H * 48.0;
    let work_y = SLOT_H * 2.0 * app.settings.work_start.saturating_sub(1) as f32;
    let scroll_to = (!app.cal.scrolled).then_some(work_y);
    app.cal.scrolled = true;
    let mut sa = egui::ScrollArea::vertical().id_salt("cal_hours").auto_shrink([false, false]);
    if let Some(y) = scroll_to {
        sa = sa.vertical_scroll_offset(y);
    }
    let timed: Vec<EventRow> = app.cal.events.iter().filter(|e| !(e.all_day || e.end - e.start >= 86_400)).cloned().collect();
    sa.show(ui, |ui| {
        let (gr, bg) = ui.allocate_exact_size(vec2(ui.available_width(), total_h), Sense::click());
        let p = ui.painter_at(gr);
        for d in 0..n {
            let x = gr.min.x + TIME_GUTTER + col_w * d as f32;
            let day = start + Duration::days(d);
            let weekend = matches!(day.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun);
            for slot in 0..48 {
                let y = gr.min.y + slot as f32 * SLOT_H;
                let hour = slot / 2;
                let work = !weekend && hour >= app.settings.work_start && hour < app.settings.work_end;
                p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(col_w, SLOT_H)), 0.0, if work { t.work_hours } else { t.off_hours });
                let c = if slot % 2 == 0 { t.grid_line } else { t.grid_half };
                p.line_segment([pos2(x, y), pos2(x + col_w, y)], Stroke::new(1.0, c));
            }
            p.line_segment([pos2(x, gr.min.y), pos2(x, gr.max.y)], Stroke::new(1.0, t.grid_line));
        }
        for h in 0..24 {
            let y = gr.min.y + h as f32 * SLOT_H * 2.0;
            p.text(pos2(gr.min.x + TIME_GUTTER - 8.0, y + 2.0), Align2::RIGHT_TOP, format!("{h:02}:00"), regular(11.0), t.text_dim);
        }
        // Double-click an empty slot: a new appointment there.
        if bg.double_clicked() {
            if let Some(pos) = bg.interact_pointer_pos() {
                let d = ((pos.x - gr.min.x - TIME_GUTTER) / col_w).floor();
                if d >= 0.0 && (d as i64) < n {
                    let slot = ((pos.y - gr.min.y) / SLOT_H).floor().clamp(0.0, 47.0) as u32;
                    let day = start + Duration::days(d as i64);
                    let ts = local_ts(day, slot / 2, (slot % 2) * 30);
                    app.new_event(false, Some(ts));
                }
            }
        }
        if bg.clicked() {
            app.cal.selected = None;
        }
        // Events.
        for d in 0..n {
            let day = start + Duration::days(d);
            let (d0, d1) = (local_ts(day, 0, 0), local_ts(day + Duration::days(1), 0, 0));
            let todays: Vec<&EventRow> = timed.iter().filter(|e| e.start < d1 && e.end > d0).collect();
            let lay = columns(&todays);
            let x = gr.min.x + TIME_GUTTER + col_w * d as f32;
            for (i, col, cols) in lay {
                let e = todays[i];
                let s = (e.start.max(d0) - d0) as f32 / 1800.0 * SLOT_H;
                let en = ((e.end.min(d1) - d0) as f32 / 1800.0 * SLOT_H).max(s + SLOT_H * 0.9);
                let w = (col_w - 8.0) / cols as f32;
                let r = Rect::from_min_max(pos2(x + 2.0 + w * col as f32, gr.min.y + s + 1.0), pos2(x + 2.0 + w * (col as f32 + 1.0) - 2.0, gr.min.y + en - 1.0));
                event_chip(app, ui, r, e, t, false);
            }
        }
        // Now.
        let now = Local::now();
        let nd = now.date_naive();
        if nd >= start && nd < start + Duration::days(n) {
            let d = (nd - start).num_days();
            let x = gr.min.x + TIME_GUTTER + col_w * d as f32;
            let y = gr.min.y + (now.timestamp() - local_ts(nd, 0, 0)) as f32 / 1800.0 * SLOT_H;
            ui.painter().line_segment([pos2(x, y), pos2(x + col_w, y)], Stroke::new(2.0, t.now_line));
            ui.painter().circle_filled(pos2(x, y), 4.0, t.now_line);
        }
    });
}

fn covers_day(e: &EventRow, day: NaiveDate) -> bool {
    if e.all_day {
        let s = chrono::DateTime::from_timestamp(e.start, 0).map(|t| t.date_naive()).unwrap_or_default();
        let en = chrono::DateTime::from_timestamp(e.end, 0).map(|t| t.date_naive()).unwrap_or_default();
        day >= s && day < en.max(s + Duration::days(1))
    } else {
        let (d0, d1) = (local_ts(day, 0, 0), local_ts(day + Duration::days(1), 0, 0));
        e.start < d1 && e.end > d0
    }
}

fn event_chip(app: &mut MailApp, ui: &mut Ui, r: Rect, e: &EventRow, t: &Tokens, compact: bool) {
    let id = egui::Id::new(("event", &e.id, r.min.x as i32, r.min.y as i32));
    let resp = ui.interact(r, id, Sense::click());
    let base = event_color(e, &app.cal.calendars, t);
    let selected = app.cal.selected.as_deref() == Some(e.id.as_str());
    let (fill, text) = match e.show_as.as_str() {
        "free" => (if t.dark { t.reading } else { Color32::WHITE }, t.text),
        "oof" => (tint(Color32::from_rgb(0x88, 0x17, 0x98), t, 0.35), t.text),
        _ => (tint(base, t, if selected { 0.55 } else { 0.25 }), t.text),
    };
    let p = ui.painter_at(r);
    p.rect_filled(r, 3.0, fill);
    p.rect_stroke(r, 3.0, Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { t.text } else { tint(base, t, 0.6) }), egui::StrokeKind::Inside);
    let bar = Rect::from_min_size(r.min, vec2(4.0, r.height()));
    if e.show_as == "tentative" || e.response == "notResponded" {
        // Hatched: not yet decided.
        p.rect_filled(bar, 1.0, Color32::WHITE);
        let mut y = r.min.y;
        while y < r.max.y {
            p.line_segment([pos2(bar.min.x, y + 4.0), pos2(bar.max.x, y)], Stroke::new(1.5, base));
            y += 4.0;
        }
    } else {
        p.rect_filled(bar, 1.0, base);
    }
    let mut x = r.min.x + 8.0;
    if e.kind == "occurrence" || e.kind == "exception" {
        crate::icons::paint(&p, Rect::from_min_size(pos2(r.max.x - 16.0, r.min.y + 3.0), vec2(12.0, 12.0)), "recurring", t.text_dim, t.text_dim);
    }
    if !e.join_url.is_empty() && r.height() > 30.0 {
        crate::icons::paint(&p, Rect::from_min_size(pos2(r.max.x - 16.0, r.max.y - 16.0), vec2(12.0, 12.0)), "video", t.text_dim, t.accent);
    }
    let avail = r.width() - 12.0;
    if compact || r.height() < 30.0 {
        let label = if compact || e.all_day { e.subject.clone() } else { format!("{} {}", crate::list::local(e.start).format("%H:%M"), e.subject) };
        p.text(pos2(x, r.center().y), Align2::LEFT_CENTER, crate::list::elide(&p, &label, medium(11.5), avail), medium(11.5), text);
    } else {
        p.text(pos2(x, r.min.y + 4.0), Align2::LEFT_TOP, crate::list::elide(&p, &e.subject, semibold(11.5), avail - 14.0), semibold(11.5), text);
        let mut y = r.min.y + 19.0;
        if !e.location.is_empty() && y + 12.0 < r.max.y {
            p.text(pos2(x, y), Align2::LEFT_TOP, crate::list::elide(&p, &e.location, regular(11.0), avail), regular(11.0), t.text_dim);
            y += 14.0;
        }
        if e.organizer.address.is_empty() || e.is_organizer || y + 12.0 >= r.max.y {
        } else {
            p.text(pos2(x, y), Align2::LEFT_TOP, crate::list::elide(&p, e.organizer.label(), regular(11.0), avail), regular(11.0), t.text_dim);
        }
        x += 0.0;
        let _ = x;
    }
    let tip = format!("{}\n{}{}", e.subject, when_text(e), if e.location.is_empty() { String::new() } else { format!("\n{}", e.location) });
    let resp = resp.on_hover_text(tip);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &e.subject));
    if resp.clicked() {
        app.cal.selected = Some(e.id.clone());
    }
    if resp.double_clicked() {
        app.open_event(&e.id);
    }
    let ev = e.clone();
    resp.context_menu(|ui| {
        ui.set_min_width(200.0);
        if widgets::menu_item(ui, "open", "Open", "").clicked() {
            app.open_event(&ev.id);
            ui.close();
        }
        if !ev.join_url.is_empty() && widgets::menu_item(ui, "video", "Join Online Meeting", "").clicked() {
            let _ = open::that_detached(&ev.join_url);
            ui.close();
        }
        if !ev.is_organizer && !ev.local {
            ui.separator();
            for (icon, label, r) in [("accept", "Accept", "accept"), ("tentative", "Tentative", "tentative"), ("decline", "Decline", "decline")] {
                if widgets::menu_item(ui, icon, label, "").clicked() {
                    app.act_for_event(&ev.id, Action::RespondEvent { id: ev.id.clone(), response: r.into(), comment: String::new(), send: true });
                    ui.close();
                }
            }
        }
        ui.separator();
        if widgets::menu_item(ui, "delete", if ev.is_organizer && !ev.attendees.is_empty() { "Cancel Meeting" } else { "Delete" }, "Del").clicked() {
            app.delete_event(&ev.id);
            ui.close();
        }
    });
}

fn month(app: &mut MailApp, ui: &mut Ui, t: &Tokens) {
    let (start, n) = app.cal.days(&app.settings);
    let weeks = (n / 7).max(1);
    let area = ui.available_rect_before_wrap();
    let head_h = 24.0;
    let cw = area.width() / 7.0;
    let ch = ((area.height() - head_h) / weeks as f32).max(60.0);
    let (r, _) = ui.allocate_exact_size(area.size(), Sense::hover());
    for i in 0..7 {
        let d = start + Duration::days(i);
        ui.painter().text(pos2(r.min.x + cw * i as f32 + 8.0, r.min.y + head_h / 2.0), Align2::LEFT_CENTER, d.format("%A").to_string(), regular(11.5), t.text_dim);
    }
    let month_no = app.cal.anchor.month();
    for w in 0..weeks {
        for i in 0..7 {
            let day = start + Duration::days(w * 7 + i);
            let cell = Rect::from_min_size(pos2(r.min.x + cw * i as f32, r.min.y + head_h + ch * w as f32), vec2(cw, ch));
            let in_month = day.month() == month_no;
            ui.painter().rect_filled(cell, 0.0, if in_month { t.work_hours } else { t.off_hours });
            ui.painter().rect_stroke(cell, 0.0, Stroke::new(1.0, t.grid_line), egui::StrokeKind::Inside);
            let resp = ui.interact(cell, ui.id().with(("mday", day)), Sense::click());
            if resp.double_clicked() {
                app.new_event(false, Some(local_ts(day, app.settings.work_start, 0)));
            }
            let label = if day.day() == 1 { day.format("%-d %b").to_string() } else { day.day().to_string() };
            if day == today() {
                let tw = ui.painter().layout_no_wrap(label.clone(), semibold(12.0), t.on_accent).size().x;
                ui.painter().rect_filled(Rect::from_min_size(cell.min + vec2(4.0, 3.0), vec2(tw + 10.0, 18.0)), 9.0, t.today);
                ui.painter().text(cell.min + vec2(9.0, 12.0), Align2::LEFT_CENTER, label, semibold(12.0), t.on_accent);
            } else {
                ui.painter().text(cell.min + vec2(8.0, 12.0), Align2::LEFT_CENTER, label, regular(12.0), if in_month { t.text } else { t.text_disabled });
            }
            let todays: Vec<EventRow> = app.cal.events.iter().filter(|e| covers_day(e, day)).cloned().collect();
            let room = ((ch - 26.0) / 20.0).floor().max(0.0) as usize;
            for (k, e) in todays.iter().enumerate() {
                if k >= room.saturating_sub(if todays.len() > room { 1 } else { 0 }) {
                    let more = todays.len() - k;
                    let mr = Rect::from_min_size(pos2(cell.min.x + 6.0, cell.min.y + 24.0 + k as f32 * 20.0), vec2(cw - 12.0, 18.0));
                    let m = ui.interact(mr, ui.id().with(("more", day)), Sense::click());
                    ui.painter().text(mr.left_center(), Align2::LEFT_CENTER, format!("{more} more\u{2026}"), medium(11.0), t.accent_text);
                    if m.clicked() {
                        app.cal.anchor = day;
                        app.cal.set_view(CalendarView::Day);
                    }
                    break;
                }
                let er = Rect::from_min_size(pos2(cell.min.x + 3.0, cell.min.y + 24.0 + k as f32 * 20.0), vec2(cw - 6.0, 18.0));
                event_chip(app, ui, er, e, t, false);
            }
        }
    }
}

impl MailApp {
    pub fn delete_event(&mut self, id: &str) {
        let owner = self.session_for_event(id);
        let Some(e) = self.cal.events.iter().find(|e| e.id == id).cloned().or_else(|| owner.and_then(|i| self.sessions[i].store.event(id).ok().flatten())) else { return };
        let a = if e.is_organizer && !e.attendees.is_empty() {
            Action::CancelEvent { id: id.to_string(), comment: String::new() }
        } else if !e.is_organizer && e.response != "declined" && !e.organizer.address.is_empty() {
            Action::RespondEvent { id: id.to_string(), response: "decline".into(), comment: String::new(), send: true }
        } else {
            Action::DeleteEvent { id: id.to_string() }
        };
        self.act_for_event(id, a);
        self.cal.selected = None;
        self.cal.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_events_share_the_column() {
        let ev = |s: i64, e: i64| EventRow { start: s * 60, end: e * 60, ..Default::default() };
        let a = ev(540, 600);
        let b = ev(570, 630);
        let c = ev(600, 660);
        let d = ev(720, 780);
        let list = vec![&a, &b, &c, &d];
        let cols = columns(&list);
        assert_eq!((cols[0].1, cols[0].2), (0, 2));
        assert_eq!((cols[1].1, cols[1].2), (1, 2));
        assert_eq!((cols[2].1, cols[2].2), (0, 2), "c fits back in column 0 once a ends");
        assert_eq!((cols[3].1, cols[3].2), (0, 1));
    }

    #[test]
    fn ranges_and_titles() {
        let s = Settings::default();
        let mut c = CalState::new(CalendarView::WorkWeek);
        c.anchor = NaiveDate::from_ymd_opt(2026, 10, 22).unwrap();
        let (start, n) = c.days(&s);
        assert_eq!((start, n), (NaiveDate::from_ymd_opt(2026, 10, 19).unwrap(), 5));
        assert_eq!(c.range_title(&s), "19\u{2013}23 October 2026");
        c.set_view(CalendarView::Month);
        let (start, n) = c.days(&s);
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 9, 28).unwrap());
        assert_eq!(n, 35);
        assert_eq!(c.range_title(&s), "October 2026");
        c.step(1);
        assert_eq!(c.anchor, NaiveDate::from_ymd_opt(2026, 11, 22).unwrap());
        assert_eq!(add_months(NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(), 1), NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
    }
}

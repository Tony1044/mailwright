//! Writing mail: new messages, replies and forwards, each in its own window.

use base64::Engine as _;
use egui::{Context, Ui, vec2};
use mailcraft_store::{Action, Addr, Compose, MessageRow, OutAttachment};

use crate::app::MailApp;
use crate::theme::{Tokens, regular, semibold};
use crate::widgets;

pub struct ComposeWin {
    pub wid: u64,
    /// The account it's sent from (its address).
    pub account: String,
    pub local_id: String,
    pub draft_id: Option<String>,
    pub reply_to: Option<(String, String)>,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub show_bcc: bool,
    pub subject: String,
    pub body: String,
    /// The original, for replies and forwards (HTML, with its header).
    pub quoted: Option<String>,
    pub quoted_doc: Option<mailcraft_html::Document>,
    pub show_quoted: bool,
    pub attachments: Vec<OutAttachment>,
    /// Attachments of a forwarded message, which go along with it.
    pub forwarded: Vec<String>,
    pub importance: String,
    pub read_receipt: bool,
    pub dirty: bool,
    pub open: bool,
    pub asking_close: bool,
    pub asking_no_subject: bool,
    pub error: Option<String>,
    pub focus: Option<&'static str>,
    pub people_query: String,
    /// The local id of the last save, until the server has it.
    pub pending_local: Option<String>,
}

impl ComposeWin {
    pub fn title(&self) -> String {
        let s = if self.subject.trim().is_empty() { "Untitled".to_string() } else { self.subject.trim().to_string() };
        format!("{s} \u{2013} Message (HTML)")
    }
}

pub fn safe_file_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "attachment".into() } else { s.chars().take(150).collect() }
}

/// "Ada Lindqvist <ada@x>; raj@x" to addresses. Errors name what doesn't look like an address.
pub fn parse_recipients(text: &str) -> Result<Vec<Addr>, String> {
    let mut out = Vec::new();
    for part in text.split([';', ',']) {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        let (name, addr) = match (p.rfind('<'), p.rfind('>')) {
            (Some(a), Some(b)) if b > a => (p[..a].trim().trim_matches('"').to_string(), p[a + 1..b].trim().to_string()),
            _ => (String::new(), p.trim_matches(['<', '>']).to_string()),
        };
        let ok = addr.contains('@') && !addr.starts_with('@') && !addr.ends_with('@') && addr.rsplit('@').next().is_some_and(|d| d.contains('.')) && !addr.contains(char::is_whitespace);
        if !ok {
            return Err(format!("\u{201c}{p}\u{201d} isn't an email address."));
        }
        if !out.iter().any(|a: &Addr| a.address.eq_ignore_ascii_case(&addr)) {
            out.push(Addr::new(&name, &addr));
        }
    }
    Ok(out)
}

fn addr_list(list: &[Addr]) -> String {
    let mut s = list.iter().map(|a| a.full()).collect::<Vec<_>>().join("; ");
    if !s.is_empty() {
        s.push_str("; ");
    }
    s
}

fn strip_prefixes(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_ascii_lowercase();
        let cut = ["re:", "fw:", "fwd:", "aw:", "sv:"].iter().find(|p| lower.starts_with(**p)).map(|p| p.len());
        match cut {
            Some(n) => s = s[n..].trim_start(),
            None => return s.to_string(),
        }
    }
}

/// The header Outlook puts above a quoted message.
fn quote_header(m: &MessageRow) -> String {
    let esc = mailcraft_html::escape;
    let list = |l: &[Addr]| l.iter().map(|a| esc(&a.full())).collect::<Vec<_>>().join("; ");
    let when = crate::list::local(if m.received > 0 { m.received } else { m.sent }).format("%A, %-d %B %Y %H:%M").to_string();
    let mut h = format!(
        "<div id=\"appendonsend\"></div><hr style=\"display:inline-block;width:98%\" tabindex=\"-1\"><div id=\"divRplyFwdMsg\" dir=\"ltr\"><font face=\"Calibri, sans-serif\" style=\"font-size:11pt\" color=\"#000000\"><b>From:</b> {}<br><b>Sent:</b> {when}<br><b>To:</b> {}<br>",
        esc(&m.from.full()),
        list(&m.to)
    );
    if !m.cc.is_empty() {
        h.push_str(&format!("<b>Cc:</b> {}<br>", list(&m.cc)));
    }
    h.push_str(&format!("<b>Subject:</b> {}</font><div>&nbsp;</div></div>", esc(&m.subject)));
    h
}

impl MailApp {
    fn blank_compose(&mut self) -> ComposeWin {
        let wid = self.next_id();
        let account = self.session().map(|s| s.label().to_string()).unwrap_or_default();
        ComposeWin {
            wid,
            account,
            local_id: format!("local:{}-{wid}", chrono::Utc::now().timestamp_millis()),
            draft_id: None,
            reply_to: None,
            to: String::new(),
            cc: String::new(),
            bcc: String::new(),
            show_bcc: self.settings.show_bcc,
            subject: String::new(),
            body: String::new(),
            quoted: None,
            quoted_doc: None,
            show_quoted: true,
            attachments: Vec::new(),
            forwarded: Vec::new(),
            importance: "normal".into(),
            read_receipt: false,
            dirty: false,
            open: true,
            asking_close: false,
            asking_no_subject: false,
            error: None,
            focus: Some("to"),
            people_query: String::new(),
            pending_local: None,
        }
    }

    fn signature_text(&self, reply: bool) -> String {
        if self.settings.signature.trim().is_empty() || (reply && !self.settings.signature_on_replies) {
            String::new()
        } else {
            format!("\n\n{}", self.settings.signature.trim_end())
        }
    }

    pub fn new_mail(&mut self) {
        let mut c = self.blank_compose();
        c.body = self.signature_text(false);
        self.composes.push(c);
    }

    pub fn new_mail_to(&mut self, address: &str) {
        let mut c = self.blank_compose();
        c.to = format!("{address}; ");
        c.body = self.signature_text(false);
        c.focus = Some("subject");
        self.composes.push(c);
    }

    /// Reply, reply all or forward the selected message.
    pub fn reply(&mut self, kind: &str) {
        let Some(id) = self.mail.selected.first().cloned() else { return };
        let Some(m) = self.session().and_then(|s| s.store.message(&id).ok().flatten()) else { return };
        let attachments = self.session().map(|s| s.store.attachments(&id).unwrap_or_default()).unwrap_or_default();
        let me = self.session().map(|s| s.address.to_ascii_lowercase()).unwrap_or_default();
        let mut c = self.blank_compose();
        let base = strip_prefixes(&m.subject);
        match kind {
            "forward" => {
                c.subject = format!("FW: {base}");
                c.forwarded = attachments.iter().filter(|a| !a.inline).map(|a| a.name.clone()).collect();
                c.focus = Some("to");
            }
            _ => {
                c.subject = format!("RE: {base}");
                let mut to = vec![m.from.clone()];
                let mut cc = Vec::new();
                if kind == "replyAll" {
                    to.extend(m.to.iter().filter(|a| a.address.to_ascii_lowercase() != me).cloned());
                    cc.extend(m.cc.iter().filter(|a| a.address.to_ascii_lowercase() != me).cloned());
                }
                to.retain(|a| !a.address.is_empty());
                c.to = addr_list(&to);
                c.cc = addr_list(&cc);
                c.focus = Some("body");
            }
        }
        c.body = self.signature_text(true);
        let original = m.body.clone().unwrap_or_else(|| mailcraft_store::text_to_html(&m.preview));
        let quoted = format!("{}{}{}", mailcraft_html::head_styles(&original), quote_header(&m), mailcraft_html::body_inner(&original));
        c.quoted_doc = Some(mailcraft_html::parse(&quoted));
        c.quoted = Some(quoted);
        c.reply_to = Some((m.id.clone(), kind.to_string()));
        self.composes.push(c);
    }

    /// Open a draft (or an unsent Outbox message) to carry on writing it.
    pub fn edit_draft(&mut self, m: &MessageRow) {
        if self.composes.iter().any(|c| c.draft_id.as_deref() == Some(m.id.as_str()) || c.local_id == m.id) {
            return;
        }
        let mut c = self.blank_compose();
        if m.id.starts_with("local:") {
            c.local_id = m.id.clone();
        } else {
            c.draft_id = Some(m.id.clone());
        }
        c.to = addr_list(&m.to);
        c.cc = addr_list(&m.cc);
        c.bcc = addr_list(&m.bcc);
        c.show_bcc |= !m.bcc.is_empty();
        c.subject = m.subject.clone();
        c.importance = if m.importance.is_empty() { "normal".into() } else { m.importance.clone() };
        c.body = m.body.as_deref().map(mailcraft_html::to_text).unwrap_or_else(|| m.preview.clone());
        c.focus = Some("body");
        self.composes.push(c);
    }

    fn build(&self, c: &ComposeWin) -> Result<Compose, String> {
        let to = parse_recipients(&c.to)?;
        let cc = parse_recipients(&c.cc)?;
        let bcc = parse_recipients(&c.bcc)?;
        let mut html = mailcraft_html::text_to_html(&c.body, &self.settings.compose_font, self.settings.compose_size);
        if let Some(q) = &c.quoted {
            html.push_str(q);
        }
        let html = format!("<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\"></head><body>{html}</body></html>");
        Ok(Compose {
            local_id: c.local_id.clone(),
            draft_id: c.draft_id.clone(),
            reply_to: c.reply_to.clone(),
            subject: c.subject.trim().to_string(),
            to,
            cc,
            bcc,
            html,
            importance: c.importance.clone(),
            attachments: c.attachments.clone(),
            request_read_receipt: c.read_receipt,
        })
    }

    fn send_compose(&mut self, i: usize) {
        let built = self.build(&self.composes[i]);
        match built {
            Ok(compose) => {
                if compose.to.is_empty() && compose.cc.is_empty() && compose.bcc.is_empty() {
                    self.composes[i].error = Some("Add at least one name in To, Cc or Bcc.".into());
                    self.composes[i].focus = Some("to");
                    return;
                }
                if compose.subject.is_empty() && !self.composes[i].asking_no_subject {
                    self.composes[i].asking_no_subject = true;
                    return;
                }
                let subject = compose.subject.clone();
                let owner = self.session_by_address(&self.composes[i].account).unwrap_or(self.active);
                self.send_to(owner, mailcraft_sync::Command::Do(Action::Send { compose }));
                self.composes[i].open = false;
                self.status = format!("Sending \u{201c}{subject}\u{201d}\u{2026}");
            }
            Err(e) => {
                self.composes[i].error = Some(e);
                self.composes[i].asking_no_subject = false;
            }
        }
    }

    fn save_compose(&mut self, i: usize) {
        match self.build(&self.composes[i]) {
            Ok(compose) => {
                let owner = self.session_by_address(&self.composes[i].account).unwrap_or(self.active);
                self.send_to(owner, mailcraft_sync::Command::Do(Action::SaveDraft { compose }));
                // Once the server has it, `DraftSaved` points this window at it; meanwhile a new
                // local id keeps the next save apart.
                let c = &mut self.composes[i];
                c.dirty = false;
                c.attachments.clear();
                c.status_saved();
                self.status = "Draft saved".into();
            }
            Err(e) => self.composes[i].error = Some(e),
        }
    }

    pub fn draft_saved(&mut self, local_id: &str, draft_id: &str) {
        for c in &mut self.composes {
            if c.local_id == local_id || c.pending_local.as_deref() == Some(local_id) {
                c.draft_id = Some(draft_id.to_string());
                c.reply_to = None;
            }
        }
    }

    fn attach_files(&mut self, i: usize, paths: Vec<std::path::PathBuf>) {
        for p in paths {
            match std::fs::metadata(&p) {
                Ok(md) if md.len() > 150 * 1024 * 1024 => {
                    self.composes[i].error = Some(format!("{} is larger than 150 MB, the most Exchange Online accepts.", p.display()));
                    continue;
                }
                Err(e) => {
                    self.composes[i].error = Some(format!("{}: {e}", p.display()));
                    continue;
                }
                _ => {}
            }
            match std::fs::read(&p) {
                Ok(bytes) => {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "attachment".into());
                    let ct = mime_guess::from_path(&p).first_or_octet_stream().essence_str().to_string();
                    self.composes[i].attachments.push(OutAttachment { name, content_type: ct, data: base64::engine::general_purpose::STANDARD.encode(&bytes), content_id: None });
                    self.composes[i].dirty = true;
                }
                Err(e) => self.composes[i].error = Some(format!("{}: {e}", p.display())),
            }
        }
    }
}

impl ComposeWin {
    fn status_saved(&mut self) {
        self.pending_local = Some(self.local_id.clone());
        self.local_id = format!("{}-s", self.local_id);
    }
}

enum Do {
    Send,
    Save,
    Attach,
    Close,
    Discard,
    People(String),
}

pub fn windows(app: &mut MailApp, ctx: &Context) {
    let mut i = 0;
    while i < app.composes.len() {
        if !app.composes[i].open {
            app.composes.remove(i);
            continue;
        }
        let wid = app.composes[i].wid;
        let title = app.composes[i].title();
        let mut todo: Vec<Do> = Vec::new();
        let builder = egui::ViewportBuilder::default().with_title(title).with_inner_size([860.0, 700.0]).with_min_inner_size([520.0, 360.0]).with_drag_and_drop(true);
        let people = app.people.clone();
        let accounts: Vec<String> = app.sessions.iter().map(|s| s.label().to_string()).collect();
        let tokens_dark = Tokens::get(ctx).dark;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(("compose", wid)), builder, |ui, class| {
            let c = &mut app.composes[i];
            let t = if tokens_dark { Tokens::dark() } else { Tokens::get(ui.ctx()) };
            if class == egui::ViewportClass::EmbeddedWindow {
                ui.set_min_size(vec2(700.0, 520.0));
            }
            let close_requested = ui.ctx().input(|inp| inp.viewport().close_requested());
            if close_requested {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                todo.push(Do::Close);
            }
            let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|inp| inp.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
            if !dropped.is_empty() {
                todo.push(Do::Attach);
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(("dropped", wid)), dropped));
            }
            if ui.ctx().input_mut(|inp| inp.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Enter))) {
                todo.push(Do::Send);
            }
            if ui.ctx().input_mut(|inp| inp.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S))) {
                todo.push(Do::Save);
            }
            egui::Panel::top("compose_ribbon")
                .frame(egui::Frame::new().fill(t.ribbon).inner_margin(egui::Margin::symmetric(6, 4)).stroke(egui::Stroke::new(1.0, t.border)))
                .show(ui, |ui| compose_ribbon(ui, c, &mut todo));
            egui::CentralPanel::default().frame(egui::Frame::new().fill(t.reading).inner_margin(egui::Margin::symmetric(12, 10))).show(ui, |ui| {
                compose_body(ui, c, &t, &people, &accounts, &mut todo);
            });
            if c.asking_close {
                egui::Modal::new(egui::Id::new(("ask_close", wid))).show(ui.ctx(), |ui| {
                    ui.set_width(360.0);
                    ui.label(egui::RichText::new("Do you want to save changes?").font(semibold(15.0)));
                    ui.add_space(6.0);
                    ui.label("Your message will be kept in Drafts.");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if widgets::primary(ui, "Save").clicked() {
                            todo.push(Do::Save);
                            todo.push(Do::Discard);
                        }
                        if widgets::secondary(ui, "Don't Save").clicked() {
                            todo.push(Do::Discard);
                        }
                        if widgets::secondary(ui, "Cancel").clicked() {
                            c.asking_close = false;
                        }
                    });
                });
            }
            if c.asking_no_subject {
                egui::Modal::new(egui::Id::new(("ask_subject", wid))).show(ui.ctx(), |ui| {
                    ui.set_width(380.0);
                    ui.label(egui::RichText::new("Send this message without a subject?").font(semibold(15.0)));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if widgets::primary(ui, "Send Anyway").clicked() {
                            todo.push(Do::Send);
                        }
                        if widgets::secondary(ui, "Don't Send").clicked() {
                            c.asking_no_subject = false;
                            c.focus = Some("subject");
                        }
                    });
                });
            }
        });
        for d in todo {
            match d {
                Do::Send => app.send_compose(i),
                Do::Save => app.save_compose(i),
                Do::Attach => {
                    let dropped: Option<Vec<std::path::PathBuf>> = ctx.data_mut(|dd| dd.remove_temp(egui::Id::new(("dropped", wid))));
                    let files = match dropped {
                        Some(f) => f,
                        None => rfd::FileDialog::new().set_title("Insert File").pick_files().unwrap_or_default(),
                    };
                    app.attach_files(i, files);
                }
                Do::Close => {
                    let c = &mut app.composes[i];
                    if c.dirty && !c.body.trim().is_empty() || (c.dirty && (!c.to.trim().is_empty() || !c.subject.trim().is_empty())) {
                        c.asking_close = true;
                    } else {
                        c.open = false;
                    }
                }
                Do::Discard => app.composes[i].open = false,
                Do::People(q) => {
                    if app.people.0 != q {
                        let owner = app.session_by_address(&app.composes[i].account).unwrap_or(app.active);
                        app.send_to(owner, mailcraft_sync::Command::People(q));
                    }
                }
            }
        }
        i += 1;
    }
}

fn compose_ribbon(ui: &mut Ui, c: &mut ComposeWin, todo: &mut Vec<Do>) {
    ui.horizontal(|ui| {
        widgets::group(ui, "Clipboard", |ui| {
            widgets::stack(ui, |ui| {
                if widgets::small(ui, "save", "Save Draft", true, false, false).on_hover_text("Ctrl+S").clicked() {
                    todo.push(Do::Save);
                }
                if widgets::small(ui, "delete", "Discard", true, false, false).clicked() {
                    todo.push(Do::Discard);
                }
            });
        });
        widgets::group(ui, "Include", |ui| {
            if widgets::big(ui, "attach", "Attach\nFile", true, false, false).on_hover_text("Attach a file (or drop files on this window)").clicked() {
                todo.push(Do::Attach);
            }
        });
        widgets::group(ui, "Tags", |ui| {
            widgets::stack(ui, |ui| {
                let high = c.importance == "high";
                if widgets::small(ui, "important", "High Importance", true, high, false).clicked() {
                    c.importance = if high { "normal".into() } else { "high".into() };
                    c.dirty = true;
                }
                let low = c.importance == "low";
                if widgets::small(ui, "low", "Low Importance", true, low, false).clicked() {
                    c.importance = if low { "normal".into() } else { "low".into() };
                    c.dirty = true;
                }
                if widgets::small(ui, "check", "Request a Read Receipt", true, c.read_receipt, false).clicked() {
                    c.read_receipt = !c.read_receipt;
                }
            });
        });
        widgets::group(ui, "Show Fields", |ui| {
            if widgets::big(ui, "people", "Bcc", true, c.show_bcc, false).clicked() {
                c.show_bcc = !c.show_bcc;
            }
        });
    });
}

fn compose_body(ui: &mut Ui, c: &mut ComposeWin, t: &Tokens, people: &(String, Vec<Addr>), accounts: &[String], todo: &mut Vec<Do>) {
    if let Some(e) = c.error.clone() {
        if let Some(r) = widgets::info_bar(ui, "info", &e, true, Some("Dismiss")) {
            if r.clicked() {
                c.error = None;
            }
        }
        ui.add_space(4.0);
    }
    ui.horizontal_top(|ui| {
        // The big Send button, to the left of the address lines.
        let rows = if c.show_bcc { 3.0 } else { 2.0 } + if accounts.len() > 1 { 1.0 } else { 0.0 };
        let (r, resp) = ui.allocate_exact_size(vec2(64.0, rows * 30.0 - 4.0), egui::Sense::click());
        let fill = if resp.hovered() { t.accent_text } else { t.accent };
        ui.painter().rect_filled(r, 4.0, fill);
        crate::icons::paint(ui.painter(), egui::Rect::from_center_size(r.center() - vec2(0.0, 9.0), vec2(22.0, 22.0)), "send", t.on_accent, t.on_accent);
        ui.painter().text(r.center() + vec2(0.0, 14.0), egui::Align2::CENTER_CENTER, "Send", semibold(12.5), t.on_accent);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Send"));
        if resp.on_hover_text("Send (Ctrl+Enter)").clicked() {
            todo.push(Do::Send);
        }
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            if accounts.len() > 1 && c.draft_id.is_none() && c.reply_to.is_none() {
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(40.0, 24.0), egui::Label::new(egui::RichText::new("From").color(t.text_dim)));
                    egui::ComboBox::from_id_salt(("from", c.wid)).selected_text(c.account.clone()).width(ui.available_width().min(420.0)).show_ui(ui, |ui| {
                        for a in accounts {
                            ui.selectable_value(&mut c.account, a.clone(), a);
                        }
                    });
                });
            } else if accounts.len() > 1 {
                ui.label(egui::RichText::new(format!("From: {}", c.account)).color(t.text_dim));
            }
            address_line(ui, c, "To", "to", t, people, todo);
            address_line(ui, c, "Cc", "cc", t, people, todo);
            if c.show_bcc {
                address_line(ui, c, "Bcc", "bcc", t, people, todo);
            }
        });
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_sized(vec2(70.0, 24.0), egui::Label::new(egui::RichText::new("Subject").color(t.text_dim)));
        let id = egui::Id::new(("subject", c.wid));
        let r = ui.add(egui::TextEdit::singleline(&mut c.subject).id(id).desired_width(ui.available_width()).margin(vec2(6.0, 4.0)));
        if c.focus == Some("subject") {
            r.request_focus();
            c.focus = None;
        }
        if r.changed() {
            c.dirty = true;
        }
    });
    if !c.attachments.is_empty() || !c.forwarded.is_empty() {
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let mut remove = None;
            for (k, a) in c.attachments.iter().enumerate() {
                let size = a.data.len() as i64 * 3 / 4;
                let (icon, col) = crate::icons::file_icon(&a.name);
                egui::Frame::new().stroke(egui::Stroke::new(1.0, t.border_strong)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                        crate::icons::paint(ui.painter(), r, icon, col, col);
                        ui.label(egui::RichText::new(&a.name).font(regular(12.0)));
                        ui.label(egui::RichText::new(widgets::size_text(size)).color(t.text_dim).font(regular(11.0)));
                        if ui.small_button("\u{2715}").on_hover_text("Remove attachment").clicked() {
                            remove = Some(k);
                        }
                    });
                });
            }
            for f in &c.forwarded {
                ui.label(egui::RichText::new(format!("\u{1F4CE} {f} (forwarded)")).color(t.text_dim).font(regular(12.0)));
            }
            if let Some(k) = remove {
                c.attachments.remove(k);
                c.dirty = true;
            }
        });
        let total: usize = c.attachments.iter().map(|a| a.data.len() * 3 / 4).sum();
        if total > 25 * 1024 * 1024 {
            ui.label(egui::RichText::new(format!("These attachments total {}. Some mail systems don't accept messages this large.", widgets::size_text(total as i64))).color(t.high).font(regular(11.5)));
        }
    }
    ui.add_space(8.0);
    // The message, then the original below it.
    let body_id = egui::Id::new(("body", c.wid));
    egui::ScrollArea::vertical().id_salt(("compose_scroll", c.wid)).auto_shrink([false, false]).show(ui, |ui| {
        let min_h = if c.quoted.is_some() { 160.0 } else { ui.available_height().max(200.0) };
        let r = ui.add(
            egui::TextEdit::multiline(&mut c.body)
                .id(body_id)
                .desired_width(f32::INFINITY)
                .desired_rows(8)
                .min_size(vec2(0.0, min_h))
                .font(egui::FontId::proportional(14.5))
                .frame(egui::Frame::NONE)
                .hint_text("Type your message here"),
        );
        if c.focus == Some("body") {
            r.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), body_id) {
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(0))));
                state.store(ui.ctx(), body_id);
            }
            c.focus = None;
        }
        if r.changed() {
            c.dirty = true;
        }
        if let Some(doc) = &c.quoted_doc {
            ui.add_space(6.0);
            let label = if c.show_quoted { "Hide the original message" } else { "Show the original message" };
            if ui.link(label).clicked() {
                c.show_quoted = !c.show_quoted;
            }
            if c.show_quoted {
                let textures = std::collections::HashMap::new();
                let remote = std::collections::HashMap::new();
                let rc = crate::render::RenderCtx { message_id: "", textures: &textures, remote: &remote, allow_remote: false, tokens: *t, dark: t.dark };
                egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 6)).show(ui, |ui| {
                    ui.disable();
                    let _ = crate::render::document(ui, doc, &rc);
                });
            }
        }
    });
}

fn address_line(ui: &mut Ui, c: &mut ComposeWin, label: &str, key: &'static str, t: &Tokens, people: &(String, Vec<Addr>), todo: &mut Vec<Do>) {
    ui.horizontal(|ui| {
        ui.add_sized(vec2(40.0, 24.0), egui::Label::new(egui::RichText::new(label).color(t.text_dim)));
        let id = egui::Id::new((key, c.wid));
        let text = match key {
            "to" => &mut c.to,
            "cc" => &mut c.cc,
            _ => &mut c.bcc,
        };
        let resp = ui.add(egui::TextEdit::singleline(text).id(id).desired_width(ui.available_width()).margin(vec2(6.0, 4.0)));
        if c.focus == Some(key) {
            resp.request_focus();
            c.focus = None;
        }
        if resp.changed() {
            c.dirty = true;
        }
        // Completion: the part after the last separator.
        let typed = text.rsplit([';', ',']).next().unwrap_or("").trim().to_string();
        if resp.has_focus() && typed.chars().count() >= 2 && !typed.contains('<') {
            if c.people_query != typed {
                c.people_query = typed.clone();
                todo.push(Do::People(typed.clone()));
            }
            let lower = typed.to_lowercase();
            let matches: Vec<&Addr> = people.1.iter().filter(|a| a.name.to_lowercase().contains(&lower) || a.address.to_lowercase().contains(&lower)).take(8).collect();
            if !matches.is_empty() {
                let mut pick = None;
                egui::Area::new(id.with("suggest")).order(egui::Order::Foreground).fixed_pos(resp.rect.left_bottom() + vec2(0.0, 2.0)).show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(resp.rect.width().min(420.0));
                        for a in matches {
                            let r = ui.add(egui::Button::new(egui::RichText::new(a.full()).font(regular(12.5))).frame(false).min_size(vec2(ui.available_width(), 24.0)));
                            if r.clicked() {
                                pick = Some(a.clone());
                            }
                        }
                    });
                });
                let enter = ui.input(|i| i.key_pressed(egui::Key::Tab));
                if pick.is_none() && enter {
                    pick = people.1.iter().find(|a| a.name.to_lowercase().contains(&lower) || a.address.to_lowercase().contains(&lower)).cloned();
                }
                if let Some(a) = pick {
                    let cut = text.rfind([';', ',']).map(|p| p + 1).unwrap_or(0);
                    text.truncate(cut);
                    if !text.is_empty() && !text.ends_with(' ') {
                        text.push(' ');
                    }
                    text.push_str(&a.full());
                    text.push_str("; ");
                    c.dirty = true;
                    c.people_query.clear();
                    resp.request_focus();
                    if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), id) {
                        let end = text.chars().count();
                        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(end))));
                        st.store(ui.ctx(), id);
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipients_parse_and_dedupe() {
        let r = parse_recipients("Ada Lindqvist <ada@x.example>; raj@x.example, \"Mei T\" <mei@x.example>; ADA@x.example;").unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0], Addr::new("Ada Lindqvist", "ada@x.example"));
        assert_eq!(r[2].name, "Mei T");
        assert!(parse_recipients("not an address").is_err());
        assert!(parse_recipients("a@b").is_err());
        assert!(parse_recipients("").unwrap().is_empty());
    }

    #[test]
    fn subjects_keep_one_prefix() {
        assert_eq!(strip_prefixes("RE: Re: FW: Lunch"), "Lunch");
        assert_eq!(strip_prefixes("Fwd: news"), "news");
        assert_eq!(strip_prefixes("Rebate"), "Rebate");
    }

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(safe_file_name("a/b:c?.pdf"), "a_b_c_.pdf");
        assert_eq!(safe_file_name(".."), "attachment");
    }
}

impl MailApp {
    /// Send compose window `i` (what its Send button does).
    #[doc(hidden)]
    pub fn test_send_compose(&mut self, i: usize) {
        self.send_compose(i);
    }
}

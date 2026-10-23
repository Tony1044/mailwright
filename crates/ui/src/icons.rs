//! MailCraft's icons, drawn in code on a 20×20 grid (original artwork, no icon assets). Line work
//! is in `c`; `a` is the accent.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    c: Color32,
    a: Color32,
    w: f32,
}

impl Pen<'_> {
    fn s(&self) -> f32 {
        self.r.width().min(self.r.height()) / 20.0
    }
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        let s = self.s();
        let o = self.r.center() - vec2(10.0 * s, 10.0 * s);
        pos2(o.x + x * s, o.y + y * s)
    }
    fn stroke(&self, c: Color32) -> Stroke {
        Stroke::new((self.w * self.s()).max(1.0), c)
    }
    fn line(&self, pts: &[(f32, f32)]) {
        self.line_c(pts, self.c);
    }
    fn line_c(&self, pts: &[(f32, f32)], c: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pt(x, y)).collect();
        self.p.add(Shape::line(v, self.stroke(c)));
    }
    fn closed(&self, pts: &[(f32, f32)], c: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pt(x, y)).collect();
        self.p.add(Shape::closed_line(v, self.stroke(c)));
    }
    fn fill(&self, pts: &[(f32, f32)], c: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pt(x, y)).collect();
        self.p.add(Shape::convex_polygon(v, c, Stroke::NONE));
    }
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: Color32) {
        self.p.rect_stroke(Rect::from_min_max(self.pt(x0, y0), self.pt(x1, y1)), 1.5 * self.s(), self.stroke(c), egui::StrokeKind::Middle);
    }
    fn frect(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: Color32) {
        self.p.rect_filled(Rect::from_min_max(self.pt(x0, y0), self.pt(x1, y1)), 0.8 * self.s(), c);
    }
    fn circle(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.p.circle_stroke(self.pt(x, y), r * self.s(), self.stroke(c));
    }
    fn fcircle(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.p.circle_filled(self.pt(x, y), r * self.s(), c);
    }
    /// An arc from `a0` to `a1` degrees (0 = east, clockwise on screen).
    fn arc(&self, x: f32, y: f32, r: f32, a0: f32, a1: f32, c: Color32) {
        let n = ((a1 - a0).abs() / 10.0).ceil().max(2.0) as usize;
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                (x + r * a.cos(), y + r * a.sin())
            })
            .collect();
        self.line_c(&pts, c);
    }
    fn text(&self, x: f32, y: f32, size: f32, t: &str, c: Color32) {
        self.p.text(self.pt(x, y), egui::Align2::CENTER_CENTER, t, crate::theme::semibold(size * self.s()), c);
    }
    fn envelope(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: Color32) {
        self.rect(x0, y0, x1, y1, c);
        let mx = (x0 + x1) / 2.0;
        self.line_c(&[(x0 + 0.5, y0 + 0.8), (mx, y0 + (y1 - y0) * 0.55), (x1 - 0.5, y0 + 0.8)], c);
    }
    fn folder(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: Color32) {
        self.closed(&[(x0, y0), (x0 + 5.0, y0), (x0 + 6.5, y0 + 1.8), (x1, y0 + 1.8), (x1, y1), (x0, y1)], c);
        self.line_c(&[(x0, y0 + 4.0), (x1, y0 + 4.0)], c);
    }
    fn plus(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.fcircle(x, y, r, c);
        let k = r * 0.55;
        let white = Color32::WHITE;
        self.line_c(&[(x - k, y), (x + k, y)], white);
        self.line_c(&[(x, y - k), (x, y + k)], white);
    }
    fn arrow_head(&self, tip: (f32, f32), dir: (f32, f32), size: f32, c: Color32) {
        let (dx, dy) = dir;
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        let (ux, uy) = (dx / len, dy / len);
        let (px, py) = (-uy, ux);
        let b = (tip.0 - ux * size, tip.1 - uy * size);
        self.fill(&[tip, (b.0 + px * size * 0.6, b.1 + py * size * 0.6), (b.0 - px * size * 0.6, b.1 - py * size * 0.6)], c);
    }
    fn calendar(&self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.rect(x0, y0, x1, y1, self.c);
        self.frect(x0, y0, x1, y0 + 3.0, self.a);
        self.line(&[(x0 + 3.5, y0 - 1.5), (x0 + 3.5, y0 + 1.0)]);
        self.line(&[(x1 - 3.5, y0 - 1.5), (x1 - 3.5, y0 + 1.0)]);
    }
    fn trash(&self, c: Color32) {
        self.line_c(&[(4.0, 5.0), (16.0, 5.0)], c);
        self.closed(&[(5.5, 5.0), (6.5, 17.0), (13.5, 17.0), (14.5, 5.0)], c);
        self.line_c(&[(8.0, 5.0), (8.5, 3.0), (11.5, 3.0), (12.0, 5.0)], c);
        self.line_c(&[(8.5, 8.0), (8.8, 14.5)], c);
        self.line_c(&[(11.5, 8.0), (11.2, 14.5)], c);
    }
    fn flag(&self, c: Color32, fill: Color32) {
        self.line_c(&[(5.0, 17.5), (5.0, 3.0)], c);
        self.fill(&[(5.0, 3.5), (15.5, 3.5), (12.5, 7.0), (15.5, 10.5), (5.0, 10.5)], fill);
    }
    fn paper_plane(&self, c: Color32, a: Color32) {
        self.closed(&[(2.5, 9.5), (17.5, 3.0), (13.0, 17.0), (9.5, 11.0)], c);
        self.line_c(&[(9.5, 11.0), (17.5, 3.0)], a);
    }
    fn person(&self, x: f32, y: f32, k: f32, c: Color32) {
        self.circle(x, y - 3.2 * k, 2.6 * k, c);
        self.arc(x, y + 4.5 * k, 4.8 * k, 200.0, 340.0, c);
    }
}

/// Paint icon `name` into `r`.
pub fn paint(p: &Painter, r: Rect, name: &str, c: Color32, a: Color32) {
    let pen = Pen { p, r, c, a, w: 1.3 };
    draw(&pen, name);
}

fn draw(pen: &Pen<'_>, name: &str) {
    let (c, a) = (pen.c, pen.a);
    match name {
        "mail" | "envelope" => pen.envelope(2.5, 5.0, 17.5, 15.5, c),
        "new_mail" => {
            pen.envelope(1.5, 4.0, 15.5, 14.0, c);
            pen.plus(15.0, 14.5, 4.0, a);
        }
        "new_items" => {
            pen.rect(3.0, 3.0, 13.5, 13.0, c);
            pen.line(&[(5.5, 6.5), (11.0, 6.5)]);
            pen.line(&[(5.5, 9.5), (11.0, 9.5)]);
            pen.plus(15.0, 15.0, 4.0, a);
        }
        "unread" => {
            pen.envelope(2.5, 5.0, 17.5, 15.5, c);
            pen.fcircle(16.5, 5.0, 2.6, a);
        }
        "read" => {
            pen.closed(&[(2.5, 9.0), (10.0, 3.5), (17.5, 9.0), (17.5, 17.0), (2.5, 17.0)], c);
            pen.line(&[(3.0, 9.5), (10.0, 14.0), (17.0, 9.5)]);
        }
        "delete" | "deleted" => pen.trash(c),
        "empty" => {
            pen.trash(c);
            pen.line_c(&[(13.0, 2.0), (17.5, 2.0)], a);
        }
        "archive" => {
            pen.rect(2.5, 3.5, 17.5, 7.0, c);
            pen.closed(&[(3.5, 7.0), (3.5, 16.5), (16.5, 16.5), (16.5, 7.0)], c);
            pen.line_c(&[(8.0, 10.0), (12.0, 10.0)], a);
        }
        "reply" => {
            pen.arc(10.0, 15.5, 7.0, 270.0, 360.0, c);
            pen.line(&[(10.0, 8.5), (5.0, 8.5)]);
            pen.arrow_head((2.0, 8.5), (-1.0, 0.0), 4.0, a);
        }
        "reply_all" => {
            pen.arc(11.5, 15.5, 6.5, 270.0, 360.0, c);
            pen.line(&[(11.5, 9.0), (7.5, 9.0)]);
            pen.arrow_head((4.8, 9.0), (-1.0, 0.0), 3.6, a);
            pen.line_c(&[(5.0, 5.5), (1.5, 9.0), (5.0, 12.5)], a);
        }
        "forward" => {
            pen.arc(10.0, 15.5, 7.0, 180.0, 270.0, c);
            pen.line(&[(10.0, 8.5), (15.0, 8.5)]);
            pen.arrow_head((18.0, 8.5), (1.0, 0.0), 4.0, a);
        }
        "move" => {
            pen.folder(1.5, 4.0, 14.0, 15.5, c);
            pen.line_c(&[(10.0, 12.0), (17.0, 12.0)], a);
            pen.arrow_head((19.0, 12.0), (1.0, 0.0), 3.5, a);
        }
        "flag" => pen.flag(c, a),
        "flag_outline" => {
            pen.line(&[(5.0, 17.5), (5.0, 3.0)]);
            pen.closed(&[(5.0, 3.5), (15.5, 3.5), (12.5, 7.0), (15.5, 10.5), (5.0, 10.5)], c);
        }
        "flag_complete" | "check" => pen.line_c(&[(3.5, 10.5), (8.0, 15.0), (16.5, 5.0)], if name == "check" { c } else { a }),
        "categorize" => {
            pen.frect(3.0, 3.0, 9.0, 9.0, Color32::from_rgb(0xE7, 0x48, 0x56));
            pen.frect(11.0, 3.0, 17.0, 9.0, Color32::from_rgb(0x00, 0x78, 0xD4));
            pen.frect(3.0, 11.0, 9.0, 17.0, Color32::from_rgb(0x16, 0xC6, 0x0C));
            pen.frect(11.0, 11.0, 17.0, 17.0, Color32::from_rgb(0xFF, 0xB9, 0x00));
        }
        "send_receive" | "sync" | "refresh" => {
            pen.arc(10.0, 10.0, 6.5, 200.0, 340.0, c);
            pen.arrow_head((16.3, 7.2), (0.3, 1.0), 3.5, a);
            pen.arc(10.0, 10.0, 6.5, 20.0, 160.0, c);
            pen.arrow_head((3.7, 12.8), (-0.3, -1.0), 3.5, a);
        }
        "update_folder" => {
            pen.folder(1.5, 3.5, 13.5, 14.0, c);
            pen.arc(14.5, 14.5, 3.5, 200.0, 470.0, a);
            pen.arrow_head((11.2, 13.4), (-0.2, -1.0), 2.8, a);
        }
        "send" | "sent" | "send_all" => pen.paper_plane(c, a),
        "offline" => {
            pen.arc(10.0, 11.0, 5.0, 180.0, 360.0, c);
            pen.line(&[(3.0, 11.0), (3.0, 15.0), (17.0, 15.0), (17.0, 11.0)]);
            pen.line_c(&[(3.0, 3.0), (17.0, 17.0)], a);
        }
        "online" => {
            pen.arc(10.0, 11.0, 5.0, 180.0, 360.0, c);
            pen.line(&[(3.0, 11.0), (3.0, 15.0), (17.0, 15.0), (17.0, 11.0)]);
        }
        "folder" => pen.folder(2.0, 4.0, 18.0, 16.0, c),
        "folder_new" => {
            pen.folder(1.5, 3.5, 15.5, 14.5, c);
            pen.plus(15.0, 15.0, 4.0, a);
        }
        "rename" => {
            pen.closed(&[(4.0, 13.0), (13.0, 4.0), (16.0, 7.0), (7.0, 16.0), (3.5, 16.5)], c);
            pen.line_c(&[(11.5, 5.5), (14.5, 8.5)], a);
        }
        "folder_delete" => {
            pen.folder(1.5, 3.5, 15.5, 14.5, c);
            pen.fcircle(15.0, 15.0, 4.0, a);
            pen.line_c(&[(13.0, 13.0), (17.0, 17.0)], Color32::WHITE);
            pen.line_c(&[(17.0, 13.0), (13.0, 17.0)], Color32::WHITE);
        }
        "mark_all_read" => {
            pen.closed(&[(2.5, 9.0), (10.0, 3.5), (17.5, 9.0), (17.5, 17.0), (2.5, 17.0)], c);
            pen.line_c(&[(6.5, 12.0), (9.0, 14.5), (14.0, 9.5)], a);
        }
        "inbox" => {
            pen.closed(&[(2.5, 11.0), (5.0, 4.0), (15.0, 4.0), (17.5, 11.0), (17.5, 16.0), (2.5, 16.0)], c);
            pen.line(&[(2.5, 11.0), (7.0, 11.0), (8.0, 13.0), (12.0, 13.0), (13.0, 11.0), (17.5, 11.0)]);
        }
        "outbox" => {
            pen.line(&[(2.5, 11.0), (7.0, 11.0), (8.0, 13.0), (12.0, 13.0), (13.0, 11.0), (17.5, 11.0), (17.5, 16.0), (2.5, 16.0), (2.5, 11.0)]);
            pen.line_c(&[(10.0, 9.5), (10.0, 2.5)], a);
            pen.arrow_head((10.0, 1.5), (0.0, -1.0), 3.0, a);
        }
        "drafts" => {
            pen.closed(&[(4.0, 2.5), (12.5, 2.5), (16.0, 6.0), (16.0, 17.5), (4.0, 17.5)], c);
            pen.line_c(&[(7.0, 14.0), (13.0, 8.0)], a);
        }
        "junk" => {
            pen.closed(&[(10.0, 2.5), (16.5, 5.0), (16.0, 11.0), (10.0, 17.5), (4.0, 11.0), (3.5, 5.0)], c);
            pen.line_c(&[(7.5, 7.5), (12.5, 12.5)], a);
            pen.line_c(&[(12.5, 7.5), (7.5, 12.5)], a);
        }
        "history" => {
            pen.circle(10.0, 10.0, 7.0, c);
            pen.line(&[(10.0, 5.5), (10.0, 10.0), (13.5, 12.0)]);
        }
        "calendar" => {
            pen.calendar(2.5, 4.0, 17.5, 17.0);
            for (x, y) in [(6.0, 10.0), (10.0, 10.0), (14.0, 10.0), (6.0, 13.5), (10.0, 13.5)] {
                pen.fcircle(x, y, 0.9, c);
            }
        }
        "today" => {
            pen.calendar(2.5, 4.0, 17.5, 17.0);
            pen.text(10.0, 11.8, 7.5, "1", c);
        }
        "next7" => {
            pen.calendar(2.5, 4.0, 17.5, 17.0);
            pen.text(10.0, 11.8, 7.5, "7", c);
        }
        "day" => {
            pen.calendar(2.5, 4.0, 17.5, 17.0);
            pen.frect(7.5, 9.0, 12.5, 15.0, a);
        }
        "work_week" => {
            pen.calendar(2.5, 4.0, 17.5, 17.0);
            for i in 0..5 {
                let x = 4.5 + i as f32 * 2.4;
                pen.frect(x, 9.0, x + 1.6, 15.0, a);
            }
        }
        "week" => {
            pen.calendar(2.5, 4.0, 17.5, 17.0);
            for i in 0..7 {
                let x = 3.8 + i as f32 * 1.8;
                pen.frect(x, 9.0, x + 1.1, 15.0, a);
            }
        }
        "month" => {
            pen.calendar(2.5, 4.0, 17.5, 17.0);
            for row in 0..3 {
                for col in 0..4 {
                    pen.frect(4.5 + col as f32 * 3.0, 8.5 + row as f32 * 2.7, 6.3 + col as f32 * 3.0, 10.0 + row as f32 * 2.7, c);
                }
            }
        }
        "new_appointment" => {
            pen.calendar(1.5, 3.5, 15.5, 15.5);
            pen.plus(15.0, 15.0, 4.0, a);
        }
        "new_meeting" | "people" => {
            pen.person(7.0, 9.5, 1.0, c);
            pen.person(14.0, 8.5, 0.85, a);
            if name == "new_meeting" {
                pen.plus(15.5, 15.5, 3.5, a);
            }
        }
        "person" => pen.person(10.0, 9.5, 1.2, c),
        "accept" => {
            pen.circle(10.0, 10.0, 7.5, pen.a);
            pen.line_c(&[(6.5, 10.5), (9.0, 13.0), (13.5, 7.5)], pen.a);
        }
        "tentative" => {
            pen.circle(10.0, 10.0, 7.5, c);
            pen.text(10.0, 10.2, 10.0, "?", c);
        }
        "decline" => {
            pen.circle(10.0, 10.0, 7.5, a);
            pen.line_c(&[(7.0, 7.0), (13.0, 13.0)], a);
            pen.line_c(&[(13.0, 7.0), (7.0, 13.0)], a);
        }
        "attach" | "paperclip" => {
            pen.line(&[(12.5, 6.0), (12.5, 14.0)]);
            pen.arc(10.0, 14.0, 2.5, 0.0, 180.0, c);
            pen.line(&[(7.5, 14.0), (7.5, 4.5)]);
            pen.arc(10.0, 4.5, 2.5, 180.0, 360.0, c);
            pen.line(&[(12.5, 4.5), (12.5, 5.0)]);
            pen.line(&[(10.0, 6.5), (10.0, 13.0)]);
        }
        "important" => {
            pen.line_c(&[(10.0, 3.0), (10.0, 12.5)], a);
            pen.fcircle(10.0, 16.0, 1.4, a);
        }
        "low" => {
            pen.line_c(&[(10.0, 3.0), (10.0, 15.0)], a);
            pen.arrow_head((10.0, 17.5), (0.0, 1.0), 4.0, a);
        }
        "search" => {
            pen.circle(8.5, 8.5, 5.0, c);
            pen.line(&[(12.2, 12.2), (17.0, 17.0)]);
        }
        "filter" => pen.closed(&[(3.0, 4.0), (17.0, 4.0), (11.5, 10.5), (11.5, 16.0), (8.5, 14.5), (8.5, 10.5)], c),
        "sort" => {
            pen.line(&[(6.0, 3.0), (6.0, 16.0)]);
            pen.arrow_head((6.0, 17.5), (0.0, 1.0), 3.2, c);
            pen.line_c(&[(14.0, 17.0), (14.0, 4.0)], a);
            pen.arrow_head((14.0, 2.5), (0.0, -1.0), 3.2, a);
        }
        "reading_pane" => {
            pen.rect(2.5, 3.5, 17.5, 16.5, c);
            pen.line(&[(8.5, 3.5), (8.5, 16.5)]);
            pen.frect(9.5, 4.5, 16.5, 15.5, a);
        }
        "reading_bottom" => {
            pen.rect(2.5, 3.5, 17.5, 16.5, c);
            pen.line(&[(2.5, 9.5), (17.5, 9.5)]);
            pen.frect(3.5, 10.5, 16.5, 15.5, a);
        }
        "folder_pane" => {
            pen.rect(2.5, 3.5, 17.5, 16.5, c);
            pen.frect(3.5, 4.5, 7.5, 15.5, a);
        }
        "focused" => {
            pen.circle(10.0, 10.0, 7.0, c);
            pen.circle(10.0, 10.0, 3.5, c);
            pen.fcircle(10.0, 10.0, 1.4, a);
        }
        "theme" => {
            pen.circle(10.0, 10.0, 7.0, c);
            pen.fill(&[(10.0, 3.0), (13.5, 4.0), (16.0, 6.5), (17.0, 10.0), (16.0, 13.5), (13.5, 16.0), (10.0, 17.0)], c);
        }
        "location" => {
            pen.arc(10.0, 8.0, 5.0, 150.0, 390.0, c);
            pen.line(&[(5.7, 10.5), (10.0, 17.5), (14.3, 10.5)]);
            pen.fcircle(10.0, 8.0, 1.6, a);
        }
        "clock" => {
            pen.circle(10.0, 10.0, 7.5, c);
            pen.line_c(&[(10.0, 5.5), (10.0, 10.0), (13.0, 12.0)], a);
        }
        "bell" | "reminder" => {
            pen.line(&[(5.0, 14.0), (5.5, 8.5)]);
            pen.arc(10.0, 8.5, 4.5, 180.0, 360.0, c);
            pen.line(&[(14.5, 8.5), (15.0, 14.0), (5.0, 14.0)]);
            pen.line(&[(3.5, 14.0), (16.5, 14.0)]);
            pen.arc(10.0, 15.0, 2.0, 0.0, 180.0, a);
        }
        "video" | "online_meeting" => {
            pen.rect(2.5, 6.0, 13.0, 14.5, c);
            pen.closed(&[(13.0, 9.0), (17.5, 6.5), (17.5, 14.0), (13.0, 11.5)], a);
        }
        "link" => {
            pen.arc(7.0, 13.0, 3.2, 45.0, 315.0, c);
            pen.arc(13.0, 7.0, 3.2, 225.0, 495.0, c);
            pen.line_c(&[(8.0, 12.0), (12.0, 8.0)], a);
        }
        "save" => {
            pen.closed(&[(3.0, 3.0), (14.5, 3.0), (17.0, 5.5), (17.0, 17.0), (3.0, 17.0)], c);
            pen.rect(6.0, 3.0, 13.0, 7.5, c);
            pen.frect(6.0, 11.0, 14.0, 17.0, a);
        }
        "discard" | "close" => {
            pen.line(&[(5.0, 5.0), (15.0, 15.0)]);
            pen.line(&[(15.0, 5.0), (5.0, 15.0)]);
        }
        "image" | "picture" => {
            pen.rect(2.5, 4.0, 17.5, 16.0, c);
            pen.fcircle(7.0, 8.0, 1.6, a);
            pen.line_c(&[(3.0, 15.0), (8.0, 10.5), (11.0, 13.0), (13.5, 10.0), (17.0, 14.0)], a);
        }
        "chevron_left" => pen.line(&[(12.5, 4.0), (6.5, 10.0), (12.5, 16.0)]),
        "chevron_right" => pen.line(&[(7.5, 4.0), (13.5, 10.0), (7.5, 16.0)]),
        "chevron_down" | "expand" => pen.fill(&[(6.0, 8.0), (14.0, 8.0), (10.0, 12.5)], c),
        "chevron_up" => pen.line(&[(4.0, 12.5), (10.0, 6.5), (16.0, 12.5)]),
        "collapsed" => pen.fill(&[(8.0, 5.5), (13.0, 10.0), (8.0, 14.5)], c),
        "dropdown" => pen.fill(&[(4.0, 7.0), (16.0, 7.0), (10.0, 13.5)], c),
        "plus" => {
            pen.line(&[(10.0, 4.0), (10.0, 16.0)]);
            pen.line(&[(4.0, 10.0), (16.0, 10.0)]);
        }
        "settings" | "options" => {
            pen.circle(10.0, 10.0, 2.8, c);
            for i in 0..8 {
                let ang = (i as f32 * 45.0).to_radians();
                let (s, co) = ang.sin_cos();
                pen.line(&[(10.0 + 5.0 * co, 10.0 + 5.0 * s), (10.0 + 7.5 * co, 10.0 + 7.5 * s)]);
            }
            pen.circle(10.0, 10.0, 5.2, c);
        }
        "sign_out" => {
            pen.line(&[(11.0, 3.0), (4.0, 3.0), (4.0, 17.0), (11.0, 17.0)]);
            pen.line_c(&[(8.0, 10.0), (16.0, 10.0)], a);
            pen.arrow_head((18.0, 10.0), (1.0, 0.0), 3.5, a);
        }
        "info" | "about" => {
            pen.circle(10.0, 10.0, 7.5, c);
            pen.line_c(&[(10.0, 9.0), (10.0, 14.5)], a);
            pen.fcircle(10.0, 6.2, 1.1, a);
        }
        "star" | "favorites" => {
            let pts: Vec<(f32, f32)> = (0..10)
                .map(|i| {
                    let r = if i % 2 == 0 { 7.5 } else { 3.2 };
                    let ang = (-90.0 + i as f32 * 36.0f32).to_radians();
                    (10.0 + r * ang.cos(), 10.5 + r * ang.sin())
                })
                .collect();
            pen.closed(&pts, c);
        }
        "more" => {
            for x in [5.0, 10.0, 15.0] {
                pen.fcircle(x, 10.0, 1.4, c);
            }
        }
        "recurring" => {
            pen.arc(10.0, 10.0, 6.0, 200.0, 340.0, c);
            pen.arc(10.0, 10.0, 6.0, 20.0, 160.0, c);
            pen.arrow_head((15.6, 8.0), (0.3, 1.0), 2.8, c);
            pen.arrow_head((4.4, 12.0), (-0.3, -1.0), 2.8, c);
        }
        "print" => {
            pen.rect(5.0, 2.5, 15.0, 7.0, c);
            pen.rect(2.5, 7.0, 17.5, 14.0, c);
            pen.frect(5.0, 12.0, 15.0, 17.5, a);
        }
        "account" => {
            pen.circle(10.0, 10.0, 8.0, c);
            pen.person(10.0, 11.0, 0.9, c);
        }
        "cloud" => {
            pen.arc(7.0, 11.0, 3.5, 90.0, 270.0, c);
            pen.arc(11.0, 8.5, 4.5, 190.0, 340.0, c);
            pen.arc(14.5, 11.5, 3.0, 270.0, 450.0, c);
            pen.line(&[(7.0, 14.5), (14.5, 14.5)]);
        }
        "file" | "attachment_file" => {
            pen.closed(&[(4.5, 2.5), (12.0, 2.5), (15.5, 6.0), (15.5, 17.5), (4.5, 17.5)], c);
            pen.line(&[(12.0, 2.5), (12.0, 6.0), (15.5, 6.0)]);
        }
        "open" => {
            pen.line(&[(8.0, 3.5), (3.5, 3.5), (3.5, 16.5), (16.5, 16.5), (16.5, 12.0)]);
            pen.line_c(&[(10.0, 10.0), (16.5, 3.5)], a);
            pen.line_c(&[(11.5, 3.5), (16.5, 3.5), (16.5, 8.5)], a);
        }
        "download" => {
            pen.line_c(&[(10.0, 3.0), (10.0, 12.0)], a);
            pen.arrow_head((10.0, 14.0), (0.0, 1.0), 4.0, a);
            pen.line(&[(3.5, 13.5), (3.5, 17.0), (16.5, 17.0), (16.5, 13.5)]);
        }
        "help" => {
            pen.circle(10.0, 10.0, 7.5, c);
            pen.text(10.0, 10.2, 10.0, "?", a);
        }
        "conversation" => {
            pen.rect(2.5, 3.5, 13.0, 11.0, c);
            pen.closed(&[(7.0, 11.0), (7.0, 16.0), (17.5, 16.0), (17.5, 7.0), (13.0, 7.0)], a);
        }
        "lock" => {
            pen.rect(4.5, 9.0, 15.5, 17.0, c);
            pen.arc(10.0, 9.0, 3.5, 180.0, 360.0, c);
        }
        _ => {
            pen.rect(4.0, 4.0, 16.0, 16.0, c);
        }
    }
}

/// The icon for a folder, by its well-known name.
pub fn folder_icon(well_known: Option<&str>) -> &'static str {
    match well_known {
        Some("inbox") => "inbox",
        Some("drafts") => "drafts",
        Some("sentitems") => "sent",
        Some("deleteditems") => "deleted",
        Some("junkemail") => "junk",
        Some("archive") => "archive",
        Some("outbox") => "outbox",
        Some("conversationhistory") => "history",
        _ => "folder",
    }
}

/// The icon for an attachment, by name and type.
pub fn file_icon(name: &str) -> (&'static str, Color32) {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "heic" => ("image", Color32::from_rgb(0x88, 0x17, 0x98)),
        "pdf" => ("file", Color32::from_rgb(0xC5, 0x0F, 0x1F)),
        "doc" | "docx" | "rtf" | "odt" => ("file", Color32::from_rgb(0x18, 0x5A, 0xBD)),
        "xls" | "xlsx" | "csv" | "ods" => ("file", Color32::from_rgb(0x10, 0x7C, 0x41)),
        "ppt" | "pptx" | "odp" => ("file", Color32::from_rgb(0xC4, 0x3E, 0x1C)),
        "zip" | "7z" | "rar" | "gz" => ("file", Color32::from_rgb(0x8E, 0x56, 0x2E)),
        "ics" => ("calendar", Color32::from_rgb(0x0F, 0x6C, 0xBD)),
        _ => ("file", Color32::from_rgb(0x61, 0x61, 0x61)),
    }
}

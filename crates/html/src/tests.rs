use super::*;

fn texts(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Text { text, .. } => s.push_str(text),
            Inline::Break => s.push('|'),
            Inline::Image(_) => s.push_str("[img]"),
        }
    }
    s
}

#[test]
fn paragraphs_whitespace_and_styles() {
    let d = parse("<html><head><style>p{color:red}</style><title>x</title></head><body><p>Hello   <b>bold</b>\n world</p><p style=\"color:#c50f1f;font-size:14pt\">Red <i>and</i> big</p><script>alert(1)</script></body></html>");
    assert_eq!(d.blocks.len(), 2);
    let Block::Para { inlines, .. } = &d.blocks[0] else { panic!() };
    assert_eq!(texts(inlines), "Hello bold world");
    assert!(matches!(&inlines[1], Inline::Text { style, .. } if style.bold));
    let Block::Para { inlines, .. } = &d.blocks[1] else { panic!() };
    assert!(matches!(&inlines[0], Inline::Text { style, .. } if style.color == Some([0xc5, 0x0f, 0x1f]) && style.size == Some(14.0)));
    assert!(matches!(&inlines[1], Inline::Text { style, .. } if style.italic));
}

#[test]
fn hidden_preheaders_and_tracking_pixels_are_dropped() {
    let d = parse("<div style=\"display:none;max-height:0\">Preheader text</div><p>Visible<img src=\"https://t.example/p.gif\" width=\"1\" height=\"1\"></p><img src=\"https://cdn.example/hero.png\" width=\"600\">");
    let all = to_text("<div style=\"display:none\">Preheader</div><p>Visible</p>");
    assert_eq!(all, "Visible");
    assert_eq!(d.remote_images, 1, "the 1×1 pixel isn't counted, the hero is");
    assert!(matches!(d.images()[0].source, ImageSource::Remote(ref u) if u.ends_with("hero.png")));
}

#[test]
fn links_are_kept_safe() {
    let d = parse("<p><a href=\"https://example.com/x\">site</a> <a href=\"javascript:alert(1)\">bad</a> <a href=\"mailto:a@b.example\">mail</a></p>");
    let Block::Para { inlines, .. } = &d.blocks[0] else { panic!() };
    let links: Vec<Option<String>> = inlines.iter().filter_map(|i| if let Inline::Text { link, text, .. } = i { (!text.trim().is_empty()).then(|| link.clone()) } else { None }).collect();
    assert_eq!(links, vec![Some("https://example.com/x".into()), None, Some("mailto:a@b.example".into())]);
}

#[test]
fn lists_quotes_headings_rules_and_pre() {
    let d = parse("<h2>Title</h2><ol><li>One</li><li>Two<ul><li>Nested</li></ul></li></ol><blockquote><p>Quoted</p></blockquote><hr><pre>  a\n  b</pre>");
    assert!(matches!(&d.blocks[0], Block::Heading { level: 2, .. }));
    let items: Vec<(String, u8, String)> = d
        .blocks
        .iter()
        .filter_map(|b| if let Block::ListItem { marker, depth, inlines } = b { Some((marker.clone(), *depth, texts(inlines))) } else { None })
        .collect();
    assert_eq!(items, vec![("1.".into(), 0, "One".into()), ("2.".into(), 0, "Two".into()), ("\u{25e6}".into(), 1, "Nested".into())]);
    assert!(d.blocks.iter().any(|b| matches!(b, Block::Quote(inner) if inner.len() == 1)));
    assert!(d.blocks.iter().any(|b| matches!(b, Block::Rule)));
    assert!(d.blocks.iter().any(|b| matches!(b, Block::Pre(t) if t == "  a\n  b")));
}

#[test]
fn data_tables_stay_tables_and_layout_tables_flatten() {
    let d = parse(
        "<table width=\"100%\"><tr><td bgcolor=\"#f6f8fa\"><h3>Booking</h3><p>Ref ABC</p></td></tr></table>\
         <table border=\"1\"><tr><th>Stage</th><th>When</th></tr><tr><td>Strip-out</td><td colspan=\"1\">w/c 2 Nov</td></tr></table>",
    );
    assert!(matches!(&d.blocks[0], Block::Heading { level: 3, .. }));
    assert!(matches!(&d.blocks[1], Block::Para { background: Some([0xf6, 0xf8, 0xfa]), .. }));
    let Block::Table { rows, border, .. } = &d.blocks[2] else { panic!("{:?}", d.blocks) };
    assert!(*border);
    assert_eq!(rows.len(), 2);
    assert!(rows[0][0].header);
    assert_eq!(rows[1].len(), 2);
}

#[test]
fn cid_and_data_images() {
    let d = parse("<p><img src=\"cid:banner001\" alt=\"Banner\" width=\"560\" height=\"90\"> <img src=\"data:image/png;base64,iVBORw0KGgo=\"></p>");
    let imgs = d.images();
    assert_eq!(imgs.len(), 2);
    assert_eq!(imgs[0].source, ImageSource::Cid("banner001".into()));
    assert_eq!(imgs[0].width, Some(560.0));
    assert!(matches!(&imgs[1].source, ImageSource::Data { mime, bytes } if mime == "image/png" && bytes.starts_with(&[0x89, b'P'])));
    assert_eq!(d.remote_images, 0);
}

#[test]
fn breaks_and_plain_text() {
    let d = parse("<div>Line one<br>Line two<br><br></div><div>Next</div>");
    let Block::Para { inlines, .. } = &d.blocks[0] else { panic!() };
    assert_eq!(texts(inlines), "Line one|Line two");
    assert_eq!(to_text("<p>Hi <a href=\"https://x.example\">there</a></p><blockquote><p>old</p></blockquote>"), "Hi there <https://x.example>\n\n> old");
}

#[test]
fn typed_text_becomes_safe_html() {
    let h = text_to_html("Hi <Raj> & co,\n\nSee https://example.com/a. Or mail a@b.example\n  indented", "Calibri", 11.0);
    assert!(h.contains("Hi &lt;Raj&gt; &amp; co,"));
    assert!(h.contains("<div><br></div>"));
    assert!(h.contains("<a href=\"https://example.com/a\">https://example.com/a</a>."));
    assert!(h.contains("<a href=\"mailto:a@b.example\">"));
    assert_eq!(to_text(&h), "Hi <Raj> & co,\n\nSee https://example.com/a. Or mail a@b.example\n\nindented");
}

#[test]
fn body_inner_and_styles() {
    let html = "<html><head><style>.a{}</style></head><body class=x><p>In</p></body></html>";
    assert_eq!(body_inner(html), "<p>In</p>");
    assert_eq!(head_styles(html), "<style>.a{}</style>");
    assert_eq!(body_inner("<p>bare</p>"), "<p>bare</p>");
}

#[test]
fn colours() {
    assert_eq!(parse_color("#fff"), Some([255, 255, 255]));
    assert_eq!(parse_color("rgb(1, 2, 3)"), Some([1, 2, 3]));
    assert_eq!(parse_color("Navy"), Some([0, 0, 128]));
    assert_eq!(parse_color("#12345"), None);
}

#[test]
fn transparent_colours_are_not_opaque_black() {
    // A fully transparent colour is "no colour", never solid black.
    assert_eq!(parse_color("rgba(0,0,0,0)"), None);
    assert_eq!(parse_color("rgba(0, 0, 0, 0)"), None);
    assert_eq!(parse_color("rgba(255,255,255,0)"), None);
    assert_eq!(parse_color("#00000000"), None);
    assert_eq!(parse_color("#0000"), None);
    // Any non-zero alpha keeps the colour.
    assert_eq!(parse_color("rgba(249,66,58,1)"), Some([249, 66, 58]));
    assert_eq!(parse_color("rgba(10, 20, 30, 0.5)"), Some([10, 20, 30]));
    assert_eq!(parse_color("rgba(10, 20, 30)"), Some([10, 20, 30]));
    assert_eq!(parse_color("#000000ff"), Some([0, 0, 0]));

    // A transparent background must not become a black box behind the text.
    let d = parse("<div style=\"background-color:rgba(0,0,0,0)\">Hi</div>");
    let Block::Para { background, .. } = &d.blocks[0] else { panic!() };
    assert_eq!(*background, None);
}

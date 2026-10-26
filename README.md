# Mailwright

A classic desktop mail and calendar client for Microsoft 365 – Exchange Online through Microsoft Graph – written in Rust with no `unsafe` code. One standalone `.exe` for Windows; one `.deb` for Ubuntu.

- **Mail:** folder tree with Favorites, Focused/Other, a message list grouped by date, a reading pane (right, bottom or off), HTML mail drawn safely (pictures from the web blocked until you allow them), attachments (open, save, save all), new mail, reply, reply all and forward with attachments, drafts, flags, categories, read/unread, move (menu or drag to a folder), delete, archive, search (offline, or on the server), new/rename/delete folders, empty Deleted Items.
- **Calendar:** Day, Work Week, Week and Month, a date navigator, new appointments and meetings (with attendees and an online meeting link), editing, cancelling, and Accept/Tentative/Decline – from the calendar or from the meeting request in your Inbox.
- **Offline:** the last three months of mail (configurable) are kept in a local SQLite database with full-text search, kept in step with Graph delta queries. Everything you do offline is queued and sent when you're back online.
- **Sign-in:** Microsoft's own page in your browser (authorization code + PKCE, so MFA and conditional access work), or a device code. The refresh token lives in Windows Credential Manager or the desktop keyring.

There is nothing to set up first: type your email address and sign in, as in Outlook. See [docs/SETUP.md](docs/SETUP.md) for details, including what an IT administrator may need to approve. To look around without an account, choose **Try Mailwright with a sample mailbox** on the first screen.

## Layout

| Crate | What it does |
| --- | --- |
| `crates/graph` | Graph client: OAuth sign-in, tokens, mail, attachments (upload sessions over 3 MB), calendar, people; paging, throttling and token refresh |
| `crates/store` | The offline cache (SQLite + FTS5) and the queue of changes waiting for the server |
| `crates/sync` | The sync engine thread: delta rounds, sending queued changes, body prefetch |
| `crates/html` | HTML mail to a safe block model (and to text) |
| `crates/ui` | The egui interface |
| `apps/mailwright` | The desktop app |
| `tools/mockgraph` | A stand-in for Entra ID and Graph with a sample mailbox (tests and the sample mailbox) |

## Building

```sh
cargo test --workspace
cargo build --release -p mailwright                 # Linux
packaging/linux/package.sh target/release/mailwright dist
cargo build --release -p mailwright --target x86_64-pc-windows-gnu   # Windows, from Linux with mingw-w64
```

`cargo run -p mailwright-ui --example ui_shot -- shots demo shot:inbox` takes headless screenshots.

## Licence

MIT or Apache 2.0. Fonts (Inter, Source Sans 3, Source Serif 4, JetBrains Mono) are under the SIL Open Font License. Mailwright is not made or endorsed by Microsoft.

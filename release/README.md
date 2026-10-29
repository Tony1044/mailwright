# Mailwright 0.4.0 binaries

| File | For |
| --- | --- |
| `mailwright.exe` | Windows 10 and 11 (64-bit). A single file: no installer, no extra DLLs. |
| `mailwright_0.4.0-1_amd64.deb` | Ubuntu 24.04 or later (64-bit). Install with `sudo apt install ./mailwright_0.4.0-1_amd64.deb` (it replaces MailCraft if that's installed). |

## What's new in 0.4.0

- IMAP, POP3, and SMTP: Mailwright now works with any mailbox, not only Microsoft 365. Choose **Set up an IMAP or POP account** on the first screen, type your address and password, and Mailwright fills in the servers for common providers (Gmail, Yahoo, iCloud, Outlook.com, Fastmail, GMX, AOL) or guesses them from your domain.
  - IMAP keeps mail on the server and in step across devices; POP3 downloads it. Sending is over SMTP, with a copy filed in your Sent folder (IMAP).
  - Reading, flagging, moving, deleting, searching, writing, replying, forwarding, attachments, and signatures all work the same. Calendar, contacts, and automatic replies stay Microsoft 365 features.
  - All connections are encrypted (SSL/TLS or STARTTLS), using the same Rust TLS stack as the rest of Mailwright, so the Windows build is still a single file with no extra DLLs.

## In 0.3.0

- Address suggestions when composing can now be clicked, and you can move through them with the arrow keys and accept with Enter or Tab.
- Signatures: keep several named signatures (**File › Options › Signatures**), choose one for new messages and one for replies and forwards, and switch signature in a message from the **Signature** button. An earlier single signature is kept as "My signature".
- Automatic Replies (out-of-office): **File › Info › Automatic Replies**, with an optional time range and separate messages for inside and outside your organisation. This needs one extra permission, so the first time you may be asked to sign in again.
- Opening an older folder now shows its mail even when it's beyond the "keep mail offline" window, so archive folders look right rather than near-empty.

## In 0.2.2

- Fixed: reading a message could blank its subject, sender, and date, so it dropped out of view (Outlook.com sends only what changed, such as the read state, and Mailwright saved that over the whole message). Updates now change only what they carry, and messages stay in the folder they were synced from.
- Repair: on first start, copies an earlier version damaged are removed and the folders sync again from scratch, so everything comes back.
- The Inbox check now compares details (folder, date, subject, Focused or Other), not just whether a message is there.
- The taskbar envelope goes once the new mail is read, or once you've looked at your Inbox for a couple of seconds.

## In 0.2.1

- Fixed: a message you'd read could disappear from Mailwright (Outlook.com reports some changes as "removed"). Mailwright now checks with the server before taking a recent message out, and every minute compares the newest Inbox messages with the server and adds anything missing.
- Windows: new mail now shows Mailwright's own Desktop Alert in the bottom-right corner (as classic Outlook does), plays the Windows new-mail sound, and puts an amber envelope on the taskbar button – also when Mailwright is pinned – until you've read the new mail.
- File › Options › Test shows what an alert looks and sounds like.

## In 0.2.0

- MailCraft is now Mailwright. Settings, offline mail, and saved sign-ins move across on first start.
- New mail: the system's new-mail sound, a desktop alert, and an envelope on the taskbar icon until you've seen it (a count on the dock icon on Linux). See **File › Options › When new messages arrive**.
- Receiving no longer stops: a dropped connection (sleep, Wi-Fi, VPN) times out instead of waiting for ever, and a change the server keeps refusing no longer holds up incoming mail.
- Mail that couldn't be saved locally is fetched again rather than skipped.
- Linux: the window no longer freezes after being hidden or left on another workspace, and big folders draw faster.
- Focused Inbox: unread mail on the other tab shows as a count.

See [docs/SETUP.md](../docs/SETUP.md) for getting started.

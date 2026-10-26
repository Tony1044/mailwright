# Mailwright 0.2.1 binaries

| File | For |
| --- | --- |
| `mailwright.exe` | Windows 10 and 11 (64-bit). A single file: no installer, no extra DLLs. |
| `mailwright_0.2.1-1_amd64.deb` | Ubuntu 24.04 or later (64-bit). Install with `sudo apt install ./mailwright_0.2.1-1_amd64.deb` (it replaces MailCraft if that's installed). |

## What's new in 0.2.1

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

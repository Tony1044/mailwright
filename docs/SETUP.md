# Getting started with Mailwright

Mailwright works with Microsoft 365 work and school accounts (Exchange Online) and personal Outlook.com accounts through Microsoft Graph, and with any other mailbox over IMAP or POP3 for receiving and SMTP for sending (Gmail, Yahoo, iCloud, Fastmail, your own server, and so on). There's nothing to set up first.

## Adding a Microsoft 365 or Outlook.com account

1. Start Mailwright and type your email address. Select **Connect**.
2. Mailwright works out what kind of account it is, as Outlook does: it asks Microsoft about your address, and checks where your domain's mail is delivered (its MX records). It tells you what it found – a Microsoft 365 account for your organisation, an Outlook.com account, or mail that's hosted somewhere else (Google, for example), which Mailwright can't open.
3. Select **Sign in**. Microsoft's own sign-in page opens in a Mailwright window, so multi-factor authentication, your organisation's own sign-in page, and conditional access all work as they do for Outlook. If you'd rather, choose **Use my browser instead**, or **Sign in on another device with a code**.
4. The first time, Microsoft asks you to let Mailwright read and send your mail and manage your calendar. Accept, and your mail starts to arrive.

To add more accounts, select **Add Account** at the foot of the folder pane, or **File › Info › Add Account**. Each account has its own folder tree; new mail has a **From** box to choose the account; and the calendar shows every account's calendars together.

## Adding an IMAP or POP account

For any other mailbox, choose **Set up an IMAP or POP account** on the first screen (under the email box).

1. Type your email address and password. For most providers you can leave the servers as Mailwright suggests; it fills them in for common ones (Gmail, Yahoo, iCloud, Outlook.com, Fastmail, GMX, AOL) and otherwise guesses `imap.` and `smtp.` on your domain.
2. Choose **IMAP** (which keeps your mail on the server and in step across devices, recommended) or **POP3** (which downloads your mail). Check the incoming and outgoing server names, ports, and security if you need to.
3. Select **Add account**. Mailwright checks the servers and your sign-in, then starts bringing in your mail.

A few provider notes:

- **Gmail, Yahoo, iCloud, AOL** need an **app password**, not your normal password, and Gmail needs IMAP turned on in its settings. Mailwright shows a reminder for the provider it recognises.
- IMAP connections use SSL/TLS (port 993); SMTP uses SSL/TLS (465) or STARTTLS (587), filled in for you.
- IMAP and POP accounts don't have a calendar, contacts, or automatic replies (those are Microsoft 365 features). Everything else – reading, flagging, moving, deleting, searching, writing, replying, forwarding, attachments, and signatures – works the same.
- With POP3, mail is downloaded and there are no server folders, so read and flag changes are kept on this computer only, and there's just an Inbox.

### If your organisation asks for approval

Some organisations don't let people approve apps themselves. You'll see *Need admin approval* on Microsoft's page. Your IT administrator can approve Mailwright once for everyone, by signing in to Mailwright as an administrator and ticking *Consent on behalf of your organisation*, or in the Microsoft Entra admin centre under **Enterprise applications**. This is the same for Thunderbird and other independent mail apps.

## New mail

When new mail reaches your Inbox, Mailwright plays the system's new-mail sound, shows a Desktop Alert with the sender, subject, and the start of the message, and puts an amber envelope on its taskbar button until you've read the new mail. Clicking an alert opens the message. On Windows the alert is Mailwright's own, in the bottom-right corner, as in classic Outlook; on Linux it's the desktop's notification, and docks that show counts (Ubuntu Dock, Dash to Dock, KDE) also show how many unread messages are waiting. Turn any of these off under **File › Options › When new messages arrive**, where **Test** shows what an alert looks and sounds like.

If you use Focused Inbox, unread mail on the tab you're not looking at shows as a count next to the tab's name.

## Signatures

Set up signatures under **File › Options › Signatures**. You can keep several, each with its own name, and choose one to add to new messages and one to add to replies and forwards. While writing a message, the **Signature** button on the ribbon swaps to a different one. (An earlier single signature is kept as "My signature".)

## Automatic Replies

**File › Info › Automatic Replies** sets your out-of-office message on the server, so it works whether or not Mailwright is running. You can turn it on and off, limit it to a time range, and write separate messages for people inside and outside your organisation. Automatic replies need one extra permission, so the first time you may be asked to sign in again and approve it.

## What Mailwright can do with your account

| Permission | What it's used for |
| --- | --- |
| Sign you in and read your profile | Your name and address |
| Read and write your mail | Reading, flagging, moving, and deleting mail; drafts |
| Send mail as you | Sending mail |
| Read and write your calendars | Appointments, meetings, and responses |
| Read your relevant people list | Suggesting names as you type addresses |
| Read and change your mailbox settings | Automatic replies (out-of-office) |
| Keep access to data you've given it | Staying signed in |

Mailwright can only ever see and change what you yourself can. It never sees your password: it keeps a sign-in token in the operating system's credential store (Windows Credential Manager; the GNOME keyring or KWallet on Linux).

## What's kept on your computer

Mailwright keeps the last three months of mail (change this under **Send / Receive › Keep Mail Offline** or **File › Options**) in a local database, and keeps it in step with Exchange using Graph *delta queries*: only what changed is downloaded. You can read, search, flag, move, and delete offline, and write mail; everything you do offline is sent when you're back online.

- Windows: `%LOCALAPPDATA%\Mailwright\`
- Linux: `~/.local/share/Mailwright/`

Mailwright used to be called MailCraft. The first time Mailwright starts, it moves MailCraft's settings, offline mail, and saved sign-ins across, so there's nothing to set up again.

**File › Info › Remove Account** removes an account's local copy and its saved sign-in; nothing changes on the server.

## Trying it without an account

Choose **Try Mailwright with a sample mailbox** on the first screen (or start it with `--sample-mailbox`). A small stand-in for Exchange runs inside Mailwright with a made-up mailbox and calendar, so you can try everything without touching real mail.

## For administrators

Mailwright signs in with its own multi-tenant app registration (Application ID `6b43e75b-9b99-4d73-8575-390390a86137`), using the authorization code flow with PKCE and a loopback redirect, or the device code flow. It requests only delegated permissions: `User.Read`, `Mail.ReadWrite`, `Mail.Send`, `Calendars.ReadWrite`, `People.Read`, `MailboxSettings.ReadWrite` (for automatic replies), `offline_access`, `openid`, `profile`, and `email`. An organisation that would rather use its own registration can enter its Application (client) ID under **Advanced** on the add-account page.

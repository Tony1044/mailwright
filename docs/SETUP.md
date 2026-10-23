# Getting started with MailCraft

MailCraft works with Microsoft 365 work and school accounts (Exchange Online) and personal Outlook.com accounts, through Microsoft Graph. There's nothing to set up first.

## Adding an account

1. Start MailCraft and type your email address. Select **Connect**.
2. MailCraft works out what kind of account it is, as Outlook does: it asks Microsoft about your address, and checks where your domain's mail is delivered (its MX records). It tells you what it found – a Microsoft 365 account for your organisation, an Outlook.com account, or mail that's hosted somewhere else (Google, for example), which MailCraft can't open.
3. Select **Sign in**. Microsoft's own sign-in page opens in a MailCraft window, so multi-factor authentication, your organisation's own sign-in page, and conditional access all work as they do for Outlook. If you'd rather, choose **Use my browser instead**, or **Sign in on another device with a code**.
4. The first time, Microsoft asks you to let MailCraft read and send your mail and manage your calendar. Accept, and your mail starts to arrive.

To add more accounts, select **Add Account** at the foot of the folder pane, or **File › Info › Add Account**. Each account has its own folder tree; new mail has a **From** box to choose the account; and the calendar shows every account's calendars together.

### If your organisation asks for approval

Some organisations don't let people approve apps themselves. You'll see *Need admin approval* on Microsoft's page. Your IT administrator can approve MailCraft once for everyone, by signing in to MailCraft as an administrator and ticking *Consent on behalf of your organisation*, or in the Microsoft Entra admin centre under **Enterprise applications**. This is the same for Thunderbird and other independent mail apps.

## What MailCraft can do with your account

| Permission | What it's used for |
| --- | --- |
| Sign you in and read your profile | Your name and address |
| Read and write your mail | Reading, flagging, moving, and deleting mail; drafts |
| Send mail as you | Sending mail |
| Read and write your calendars | Appointments, meetings, and responses |
| Read your relevant people list | Suggesting names as you type addresses |
| Keep access to data you've given it | Staying signed in |

MailCraft can only ever see and change what you yourself can. It never sees your password: it keeps a sign-in token in the operating system's credential store (Windows Credential Manager; the GNOME keyring or KWallet on Linux).

## What's kept on your computer

MailCraft keeps the last three months of mail (change this under **Send / Receive › Keep Mail Offline** or **File › Options**) in a local database, and keeps it in step with Exchange using Graph *delta queries*: only what changed is downloaded. You can read, search, flag, move, and delete offline, and write mail; everything you do offline is sent when you're back online.

- Windows: `%LOCALAPPDATA%\MailCraft\`
- Linux: `~/.local/share/MailCraft/`

**File › Info › Remove Account** removes an account's local copy and its saved sign-in; nothing changes on the server.

## Trying it without an account

Choose **Try MailCraft with a sample mailbox** on the first screen (or start it with `--sample-mailbox`). A small stand-in for Exchange runs inside MailCraft with a made-up mailbox and calendar, so you can try everything without touching real mail.

## For administrators

MailCraft signs in with its own multi-tenant app registration (Application ID `6b43e75b-9b99-4d73-8575-390390a86137`), using the authorization code flow with PKCE and a loopback redirect, or the device code flow. It requests only delegated permissions: `User.Read`, `Mail.ReadWrite`, `Mail.Send`, `Calendars.ReadWrite`, `People.Read`, `offline_access`, `openid`, `profile`, and `email`. An organisation that would rather use its own registration can enter its Application (client) ID under **Advanced** on the add-account page.

# Setting up MailCraft for Exchange Online

MailCraft talks to Exchange Online through Microsoft Graph. Like any app that signs in with a work or school account, it needs an *app registration* in your organisation's Microsoft Entra ID. You create it once – it takes about five minutes – and everyone in the organisation can then use the same Application (client) ID.

You'll need an account that can register applications (by default every user can; some organisations restrict it to administrators).

## 1. Register the app

1. Go to the Microsoft Entra admin centre, <https://entra.microsoft.com>, and sign in.
2. Open **Identity › Applications › App registrations**, and choose **New registration**.
3. **Name:** MailCraft.
4. **Supported account types:** *Accounts in this organizational directory only* (single tenant) is the usual choice. Choose *Accounts in any organizational directory* if you'll use MailCraft with more than one organisation.
5. **Redirect URI:** choose the platform **Public client/native (mobile & desktop)** and enter `http://localhost`.
6. Select **Register**.

On the Overview page, copy the **Application (client) ID** and the **Directory (tenant) ID**.

## 2. Allow sign-in on another device (optional, recommended)

Open **Authentication**, and under **Advanced settings** set **Allow public client flows** to **Yes**, then **Save**. This is what lets MailCraft's *Use a code instead* option work (signing in from a phone or another computer when the browser on this one can't).

## 3. Add the permissions

Open **API permissions › Add a permission › Microsoft Graph › Delegated permissions** and add:

| Permission | What MailCraft uses it for |
| --- | --- |
| `User.Read` | Your name and address |
| `Mail.ReadWrite` | Reading, flagging, moving, and deleting mail; drafts |
| `Mail.Send` | Sending mail |
| `Calendars.ReadWrite` | Your calendar: appointments, meetings, and responses |
| `People.Read` | Suggesting names as you type addresses |
| `offline_access` | Staying signed in |

All of these are delegated: MailCraft can only ever see and change what you yourself can. If your organisation requires admin consent for apps, an administrator selects **Grant admin consent for …** on this page; otherwise each person agrees the first time they sign in.

## 4. Sign in

Start MailCraft, enter the **Application (client) ID**, and for **Tenant** enter your domain (for example `contoso.com`), the Directory (tenant) ID, or leave `organizations`. Select **Sign in**: Microsoft's own sign-in page opens in your browser, so multi-factor authentication, conditional access, and single sign-on work as they do for Outlook. When the page says you can close it, go back to MailCraft.

MailCraft never sees your password. It keeps a refresh token in the operating system's credential store (Windows Credential Manager; the GNOME keyring or KWallet on Linux) so you stay signed in.

## What's kept on your computer

MailCraft keeps the last three months of mail (change this under **Send / Receive › Keep Mail Offline** or **File › Options**) in a local database, and keeps it in step with Exchange using Graph *delta queries*: only what changed is downloaded. You can read, search, flag, move, and delete offline, and write mail; everything you do offline is sent when you're back online.

- Windows: `%LOCALAPPDATA%\MailCraft\`
- Linux: `~/.local/share/MailCraft/`

Settings are in `%APPDATA%\MailCraft\settings.json` (Windows) or `~/.config/MailCraft/settings.json` (Linux). **File › Info › Sign out** removes the local copy and the saved sign-in; nothing changes on the server.

## Trying it without a tenant

Choose **Try MailCraft with a sample mailbox** on the first screen (or start it with `--sample-mailbox`). A small stand-in for Exchange runs inside MailCraft with a made-up mailbox and calendar, so you can try everything – sending, replying, meeting responses, offline working – without touching real mail.

## Troubleshooting

| Message | What to do |
| --- | --- |
| *AADSTS700016: application not found* | Check the client ID, and that the tenant is the one the app is registered in. |
| *AADSTS65001 / consent required* | An administrator needs to grant consent (step 3). |
| *AADSTS50011: redirect URI mismatch* | Add `http://localhost` under the *Mobile and desktop applications* platform (step 1.5). |
| *AADSTS7000218* with *Use a code instead* | Turn on *Allow public client flows* (step 2). |
| *Can't reach graph.microsoft.com* | Check the network. MailCraft uses your system's certificates, and honours `HTTPS_PROXY`. |

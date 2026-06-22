# Portal

**Self-hosted file transfer for film & video post-production, backed by Frame io.**

Portal gives you branded **upload** and **download** links that let clients and external
collaborators share files in and out of specific Frame.io destinations without requiring
a Frame.io account or adding a paid seat to yours.

**Files never flow through the Portal server.** 

Uploads go browser-direct to Frame io's storage via presigned URLs; downloads are browser redirects to
short-lived signed URLs.

## Features

- **Upload links** (`/u/{token}`) — branded landing page; anyone can drag-drop files (folder
  structure preserved, resumable) into a configured Frame.io project/folder. Per-link expiry,
  password, size cap, allowed extensions, uploader-info fields, optional email verification, and
  an optional target subfolder + path template. Email notification on completion.
- **Download links** (`/d/{token}`) — branded page to hand a file, a selection, or a whole folder
  to clients or collaborators, downloaded straight from Frame.io via signed URLs. Optional preview
  thumbnails, password, max-downloads, viewer fields.
- **Sync rules** — watch a Frame io folder and auto-download new files to a local/NAS path, with
  path templating, conflict policy, parallel ranged downloads (flat memory on huge files),
  retries/dead-lettering, and a reconcile cron. Triggered immediately by completed Portal uploads.
- **In-Portal Frame.io explorer** — browse accounts/workspaces/projects/folders; multi-select
  download/copy/move/delete; create projects/folders; create native Frame.io share links.
- **Secure admin** — passkey (WebAuthn) passwordless login, optional TOTP 2FA, signed/httponly
  session cookies, rate-limited public endpoints.
- **Fully brandable** — upload your logo and accent color; shown on every public page and login.

## Quickstart

**One-click (standalone, automated TLS)** — on a clean host with a domain pointed at it:

```bash
curl -fsSL https://raw.githubusercontent.com/farbhaus/portal/main/install.sh | bash -s -- portal.example.com
```

This installs Docker if needed, writes `.env`, opens the firewall, pulls the image, and brings
Portal up on `https://portal.example.com` — printing a generated admin password at the end. Behind
an existing host proxy instead? Append `--behind-proxy portal.example.com`.

**From a checkout** — for either mode, or to tweak `.env` first:

```bash
git clone https://github.com/farbhaus/portal.git
cd portal
./deploy.sh portal.example.com                 # standalone, container provisions TLS
./deploy.sh --behind-proxy portal.example.com  # loopback only; you point a host proxy at it
```
## Adobe Developer Console setup for Frame.io OAuth

Portal connects to Frame.io through Adobe's identity system (Adobe IMS). To allow it, you
create an **OAuth Web App** credential in the Adobe Developer Console and paste two values
(`client_id`, `client_secret`) into Portal's **setting** page. 
This is a one-time setup. Your Frame.io account needs to be linked to an Adobe account.

**1. Create a project + Frame io API**

1. Go to https://developer.adobe.com/console and sign in with your Adobe ID.
2. **Create new project** (or open an existing one).
3. **Add API** → choose the **Frame io** API.
4. When prompted for the credential type, choose **OAuth Web App credential** (a server-side
   app that keeps a client secret). Do **not** choose Single-Page App or Server-to-Server.

**2. Set the scopes**

Ensure the credential requests these scopes (Portal's default `FRAMEIO_SCOPES`):

```
openid, email, profile, offline_access, additional_info.roles
```

`offline_access` is essential — it's what lets Portal receive a **refresh token** and stay
connected without you re-authorizing every day.

**3. Register the redirect URI(s)**

The redirect URI is where Adobe sends the browser back after you approve. 
Please paste the exact URL listed in **Settings** → **Frame io section** → **Redirect URI**

**4. Enter the credentials in Portal**

From the credential's overview page, copy the **Client ID** and **Client Secret**. 
In Portal, log in as admin and go to **Settings** → **Frame.io section** and paste them 
in the respective **Client ID** and **Client Secret** boxes.

**5. Connect**

In **Settings**, click **Connect Frame.io**. You'll be sent to Adobe to sign in and approve, then
returned to Portal showing the connected account.

## Documentation

- [DEPLOY.md](DEPLOY.md) — full deployment runbook
- [CLAUDE.md](CLAUDE.md) — architecture overview, including how Portal talks to Frame.io (storage abstraction + Adobe IMS OAuth)
- [CONTRIBUTING.md](CONTRIBUTING.md) — local dev setup and how to contribute
- [SECURITY.md](SECURITY.md) — reporting a vulnerability

## License

Portal is licensed under the **GNU Affero General Public License v3.0** ([LICENSE](LICENSE)).
The AGPL's network clause means anyone who runs a modified Portal as a service must make their
source available — self-hosting and modification are fully permitted, closed-source SaaS forks
are not.

Copyright © 2026 FARBHAUS.

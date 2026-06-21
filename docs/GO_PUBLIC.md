# Going public — maintainer checklist

Portal was prepared for an open-source release while the repo was still **private** on a free
plan, which blocks a few GitHub features. This is the one-pass checklist to run when flipping
`farbhaus/portal` to public. None of these can be done from the codebase alone — they need repo
admin / external accounts.

## 1. Re-enable the gated CI workflows

These were disabled because they fail on a private free-plan repo; they're free on public repos.

- **`.github/workflows/codeql.yml`** — uncomment the original `push` / `pull_request` / `schedule`
  triggers (currently `workflow_dispatch:` only).
- **`.github/workflows/image-scan.yml`** — same: uncomment the original triggers. (The build
  matrix already targets the single `farbhaus/portal` image.)
- **`.github/workflows/dependabot-automerge.yml`** — set `AUTOMERGE_ENABLED: 'true'`.

## 2. GitHub repository settings (UI — unlock on public)

- **Settings → General → Pull Requests → "Allow auto-merge"**: enable (required for the
  Dependabot auto-merge workflow).
- **Settings → Branches → branch protection on `main`**: require the CI status checks to pass
  before merge (this is what `gh pr merge --auto` waits on).
- **Settings → Code security → Private vulnerability reporting**: enable (backs
  [SECURITY.md](../SECURITY.md)).

## 3. Docker Hub publishing

The release workflow (`.github/workflows/docker-image.yml`) builds the all-in-one image on every
branch push (validation) and **pushes on a `v*` tag**. To make a publish work:

1. Create the **`farbhaus/portal`** repository on Docker Hub.
2. Add repo secrets **`DOCKERHUB_USERNAME`** and **`DOCKERHUB_TOKEN`** (a Docker Hub access token)
   under Settings → Secrets and variables → Actions.
3. Cut the first release: `git tag v1.0.0 && git push origin v1.0.0`. The workflow builds and
   pushes `farbhaus/portal:1.0.0` + `:latest`.

## 4. Deploy / redeploy from the published image

Once `farbhaus/portal` is published, deploy per [DEPLOY.md](DEPLOY.md). A fresh install needs no
migration — the container sets up its own database on first boot. Set `PORTAL_VERSION` in `.env`
to pin a release.

## 5. Final pre-flip scan

- Confirm `.env` (and any real secrets) are gitignored and **not** in the working tree of
  what you're publishing.
- Skim the git history for anything sensitive that predates this prep.
- Confirm `Briefing.md` / `To-Do.md` are gitignored if you don't intend to publish internal notes.

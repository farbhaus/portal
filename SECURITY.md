# Security Policy

## Reporting a vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Report them privately through GitHub's **["Report a vulnerability"](https://github.com/farbhaus/portal/security/advisories/new)**
button (Security → Advisories on the repository). This opens a private advisory visible only to
you and the maintainers.

Please include:

- a description of the issue and its impact,
- steps to reproduce (or a proof of concept),
- affected version / commit, and
- any suggested remediation if you have one.

We'll acknowledge your report, work with you on a fix, and credit you in the advisory unless you'd
prefer to remain anonymous. Please give us a reasonable window to release a fix before any public
disclosure.

## Supported versions

Portal is pre-1.0 and ships from the latest release. Security fixes target the most recent
published image (`farbhaus/portal:latest`) and the `main` branch. Please make sure you can
reproduce an issue against the latest version before reporting.

## Scope notes

- Portal is designed so file bytes never transit the server (browser-direct to Frame.io storage),
  and so it runs behind a host reverse proxy that terminates TLS. Reports about the reference
  topology should assume that proxy is present.
- Secrets are provided via environment / generated into the data volume; the OAuth and SMTP
  credentials at rest are encrypted with a key from the deployment. Findings about secret handling
  are in scope.

<!--
  Privacy & cookie notice for link RECIPIENTS (clients/collaborators who open an upload or download
  link). It's linked only from client-facing pages (upload, download, error) — not the admin login
  (operators know what Portal stores). Portal sets only strictly-functional cookies and runs no
  analytics, so no consent banner is required; this page is disclosure.

  OPERATORS: this is an accurate baseline for a stock Portal deployment. Review it and add your
  organisation's contact details before relying on it, and adjust if you customise data handling.
-->
<script lang="ts">
  import { PoweredByPortal } from "$lib/components";
</script>

<svelte:head><title>Privacy &amp; cookies · Portal</title></svelte:head>

<div class="mx-auto min-h-screen max-w-2xl px-4 py-12">
  <h1 class="text-2xl font-semibold tracking-tight">Privacy &amp; cookies</h1>
  <p class="mt-2 text-sm text-muted">
    This notice explains what happens to your information when you use an upload or download link
    from this Portal instance. It's operated by the organisation that shared the link with you —
    they're responsible for your data (the data controller).
  </p>

  <div class="mt-8 space-y-8 text-sm leading-relaxed text-muted">
    <section class="space-y-2">
      <h2 class="font-medium text-text">What we collect when you use a link</h2>
      <ul class="list-disc space-y-1.5 pl-5">
        <li>
          <span class="text-text">The details the link asks for</span> — some combination of your
          name, email address, and a message. Only the fields shown on the link are collected.
        </li>
        <li>
          For downloads, the <span class="text-text">IP address</span> of your download session, kept
          as an access record.
        </li>
        <li>
          If the link requires email verification, we email you a <span class="text-text">one-time
          code</span> and remember on your device that you verified (see cookies below).
        </li>
      </ul>
      <p class="pt-1">
        <span class="text-text">Your files don't pass through this server.</span> Uploads go straight
        from your browser to Frame.io storage, and downloads are served directly from it — this site
        only handles the link, its branding, and access checks.
      </p>
    </section>

    <section class="space-y-2">
      <h2 class="font-medium text-text">Cookies</h2>
      <p>
        This site uses only strictly-functional cookies — no advertising, no analytics, no
        third-party tracking — so there's no cookie-consent banner. As someone using a link, the only
        cookie you might get is:
      </p>
      <ul class="list-disc space-y-1.5 pl-5">
        <li>
          <code class="text-text">portal_verified</code> — set only on links that require email
          verification, so your device is remembered and you aren't asked for a new code every visit.
        </li>
      </ul>
    </section>

    <section class="space-y-2">
      <h2 class="font-medium text-text">How long it's kept</h2>
      <p>
        Records of transfers are pruned automatically after a retention period the operator
        configures.
      </p>
    </section>

    <section class="space-y-2">
      <h2 class="font-medium text-text">Your rights</h2>
      <p>
        Depending on where you live, you may have the right to access, correct, or request deletion
        of the personal data held about you, or to object to its processing. To exercise those
        rights, contact the operator of this Portal instance using the address they used to share the
        link with you.
      </p>
    </section>
  </div>

  <PoweredByPortal class="mt-10" />
</div>

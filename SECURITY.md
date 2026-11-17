# Security

Grok Monitor reads the session in each configured CLI home and sends that
bearer only to `https://cli-chat-proxy.grok.com`. The default homes are
`~/.grok` and `~/.grok-personal`. The panel process does not take `GROK_HOME`
from its own environment. Sign-in, sign-out, and a quiet session refresh run
`grok` with `GROK_HOME` set on that child only. The refresh command is
`grok models`, used when that home's access token is near `expires_at` or
already past it. It is not `grok login`.

The device-code URL and user code are shown in the icon popup so the sign-in
can be finished in an Incognito window. They are not written to logs. The
applet does not log session-file contents or tokens.

Grok Monitor does not read Grok Bot profiles to decide which icons to show.
The retired Grok Bot Monitor slot does not poll, and opening it does not read
Grok Bot profile directories, session files, or the keyring. Grok API Monitor reads a
Management API key from `~/.config/grok-mon-api/credentials.json` and/or
`XAI_MANAGEMENT_API_KEY` and calls xAI’s Management API billing endpoints.
That prepaid path is separate from the weekly usage chips.

The applet does not write session files itself. `grok login --device-code`
and `grok logout` write or clear the session in the selected CLI home.
`grok models` is the command the CLI uses to refresh an access token; the
applet runs it only for the home that is near expiry, discards its output,
and stops it after 60 seconds. A failed refresh backs off so the panel does
not spawn `grok` on every poll. Tokens and email addresses are not logged.
The Authorization header is marked sensitive so HTTP debug traces redact it.
In-memory secrets are zeroized when dropped.

## Reporting a vulnerability

If you find a token leak, credential logging issue, or other vulnerability,
**do not** open a public GitHub issue and **do not** attach `auth.json`,
`sand-secrets.json`, `credentials.json`, keyring dumps, or tokens.

Use GitHub’s private reporting form (enable **Private vulnerability
reporting** in the repository settings if that page is missing):

https://github.com/simple-systems-se/grok-mon/security/advisories/new

If the form is unavailable, email russoj88@proton.me with `[SECURITY]` in
the subject. Do not attach credentials.

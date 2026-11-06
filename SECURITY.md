# Security

Grok Monitor reads the Grok CLI session under `~/.grok` and sends that bearer
only to `https://cli-chat-proxy.grok.com`. It may also decrypt Grok Bot
`sand-secrets.json` (under `~/.config/Grok Bot*` and optional
`GROK_BOT_CONFIG_DIRS`) far enough to read which account is the work profile
and which is personal. Those tokens are dropped and are not sent to Cursor.
The retired Grok Bot Monitor slot does not poll. Grok API Monitor reads a
Management API key from `~/.config/grok-mon-api/credentials.json` and/or
`XAI_MANAGEMENT_API_KEY` and calls xAI’s Management API billing endpoints.
That prepaid path is unchanged.

None of the applets write credentials. Tokens are not logged. The Authorization
header is marked sensitive so HTTP debug traces redact it. In-memory secrets
are zeroized when dropped.

## Reporting a vulnerability

If you find a token leak, credential logging issue, or other vulnerability,
**do not** open a public GitHub issue and **do not** attach `auth.json`,
`sand-secrets.json`, `credentials.json`, keyring dumps, or tokens.

Use GitHub’s private reporting form (enable **Private vulnerability
reporting** in the repository settings if that page is missing):

https://github.com/simple-systems-se/grok-mon/security/advisories/new

If the form is unavailable, email russoj88@proton.me with `[SECURITY]` in
the subject. Do not attach credentials.

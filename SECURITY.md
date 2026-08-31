# Security

Grok Monitor reads the Grok CLI session under `~/.grok`. Grok Bot Monitor
decrypts the Grok Bot desktop app’s local session (`sand-secrets.json` plus the
login keyring item for `application=Grok Bot`) and calls Cursor usage endpoints
with that token. Grok API Monitor reads a Management API key from
`~/.config/grok-mon-api/credentials.json` and/or `XAI_MANAGEMENT_API_KEY` and
calls xAI’s Management API billing endpoints.

None of the applets write credentials. Tokens are not logged. The Authorization
header and Bot checksum header are marked sensitive so HTTP debug traces redact
them. In-memory secrets are zeroized when dropped.

## Reporting a vulnerability

If you find a token leak, credential logging issue, or other vulnerability,
**do not** open a public GitHub issue and **do not** attach `auth.json`,
`sand-secrets.json`, `credentials.json`, keyring dumps, or tokens.

Use GitHub’s private reporting form (enable **Private vulnerability
reporting** in the repository settings if that page is missing):

https://github.com/simple-systems-se/grok-mon/security/advisories/new

If the form is unavailable, email russoj88@proton.me with `[SECURITY]` in
the subject. Do not attach credentials.

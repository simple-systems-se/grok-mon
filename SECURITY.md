# Security

Grok Monitor reads the Grok CLI session under `~/.grok`. Grok Bot Monitor
decrypts the Grok Bot desktop app’s local session (`sand-secrets.json` plus the
login keyring item for `application=Grok Bot`) and calls Cursor usage endpoints
with that token.

Neither applet writes credentials. Tokens are not logged. The Authorization
header and Bot checksum header are marked sensitive so HTTP debug traces redact
them. In-memory secrets are zeroized when dropped.

## Reporting a vulnerability

If you find a token leak, credential logging issue, or other vulnerability,
**do not** open a public GitHub issue and **do not** attach `auth.json`,
`sand-secrets.json`, keyring dumps, or tokens.

Use GitHub’s private reporting form:

https://github.com/simple-systems-se/grok-mon/security/advisories/new

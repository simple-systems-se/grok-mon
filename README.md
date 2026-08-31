# Grok Monitor

A panel applet for the [COSMIC™](https://github.com/pop-os/cosmic-epoch) desktop that shows current
Grok Build credit usage. It uses the same billing endpoint as `/usage` in the
Grok CLI. A second applet, **Grok Bot Monitor**, shows Grok Bot weekly usage
from the Grok Bot desktop app. A third applet, **Grok API Monitor**, shows
remaining prepaid dollars for xAI API tokens.

This is an unofficial community project. It is not affiliated with, endorsed by,
or supported by xAI or Cursor.

## Requirements

Shared:

- The COSMIC™ desktop
- Rust 1.88 or newer (edition 2024), via [rustup](https://rustup.rs)
- `pkg-config`, a C toolchain, and the libxkbcommon development headers
- [`just`](https://github.com/casey/just) (optional; wraps the Cargo and install
  commands below)

Grok Monitor (Build):

- A signed-in [Grok CLI](https://grok.com) session (`grok login`)

Grok Bot Monitor:

- The Grok Bot desktop app, signed in
- An unlocked login keyring (Secret Service). The first poll may prompt to
  unlock Grok Bot’s Safe Storage item
- A launchable Grok Bot desktop file (`sand.desktop` or `grok-bot.desktop`) if
  you want **Open Grok Bot** to work

Grok API Monitor:

- A [Management API key](https://docs.x.ai/developers/management-api-guide)
  from the [xAI Console](https://console.x.ai) (Settings → Management Keys)
  with billing read access. Inference keys (`XAI_API_KEY`) do not work.

On Debian, Ubuntu, and Pop!_OS:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev \
  libwayland-dev libexpat1-dev libfontconfig-dev libfreetype6-dev
```

Other distributions need the equivalent packages (`libxkbcommon-devel`,
`wayland-devel`, `expat-devel`, `fontconfig-devel`, `freetype-devel` on Fedora).

## Install

```sh
git clone https://github.com/simple-systems-se/grok-mon.git
cd grok-mon
just install-user
```

Without `just`:

```sh
cargo build --release

install -Dm0755 target/release/cosmic-ext-applet-grok-monitor \
  ~/.local/bin/cosmic-ext-applet-grok-monitor

mkdir -p ~/.local/share/applications
sed "s|^Exec=.*|Exec=$HOME/.local/bin/cosmic-ext-applet-grok-monitor|" \
  res/io.github.simple-systems-se.grok-mon.desktop \
  > ~/.local/share/applications/io.github.simple-systems-se.grok-mon.desktop

install -Dm0644 res/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon.svg
install -Dm0644 res/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-symbolic.svg
install -Dm0644 res/io.github.simple-systems-se.grok-mon.metainfo.xml \
  ~/.local/share/metainfo/io.github.simple-systems-se.grok-mon.metainfo.xml

sed "s|^Exec=.*|Exec=$HOME/.local/bin/cosmic-ext-applet-grok-monitor --product=bot|" \
  res/io.github.simple-systems-se.grok-mon-bot.desktop \
  > ~/.local/share/applications/io.github.simple-systems-se.grok-mon-bot.desktop

install -Dm0644 res/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-bot.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-bot.svg
install -Dm0644 res/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-bot.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-bot-symbolic.svg
install -Dm0644 res/io.github.simple-systems-se.grok-mon-bot.metainfo.xml \
  ~/.local/share/metainfo/io.github.simple-systems-se.grok-mon-bot.metainfo.xml

sed "s|^Exec=.*|Exec=$HOME/.local/bin/cosmic-ext-applet-grok-monitor --product=api|" \
  res/io.github.simple-systems-se.grok-mon-api.desktop \
  > ~/.local/share/applications/io.github.simple-systems-se.grok-mon-api.desktop

install -Dm0644 res/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-api.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-api.svg
install -Dm0644 res/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-api.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-api-symbolic.svg
install -Dm0644 res/io.github.simple-systems-se.grok-mon-api.metainfo.xml \
  ~/.local/share/metainfo/io.github.simple-systems-se.grok-mon-api.metainfo.xml
```

Add **Grok Monitor**, **Grok Bot Monitor**, or **Grok API Monitor** in
**COSMIC Settings → Desktop → Panel → Applets**. If they do not appear, restart
the panel:

```sh
pkill cosmic-panel
```

## Usage

The panel chip shows a circular usage ring (the same shape Minimon uses for
memory) with a small **hammer** (Build), **robot** (Bot), or **key** (API) in
the center. By default Build and Bot also show current usage as a whole
percent; API shows remaining prepaid dollars. Settings can hide the number or
(for Build and Bot) switch the number and ring fill to percent remaining.
Color follows percent **used**, stepped each whole percent: green 0–50%, yellow
50–80%, orange 80–90%, red 90%+. For API, that percent used is remaining
prepaid mapped against a $50 full wallet (green above $25, yellow through $10,
orange through $5, red under $5).

Click the chip for a popup with:

- Plan name and account email
- Usage bar and percent
- Weekly or monthly reset time
- How long ago usage was fetched
- Live Grok CLI session count (from `~/.grok/active_sessions.json`)
- **Open usage** (opens [grok.com usage](https://grok.com/?_s=usage))
- **Copy** (copies the percent)
- **Settings**

Panel states:

| Chip | Meaning |
|------|---------|
| `…` | Fetching |
| `—` | Not signed in, or the session has expired |
| `?` | Request failed, and there is no previous reading |
| dimmed percent | Last good reading; the latest poll failed |

If you see `—` on Grok Monitor, run `grok login` and wait for the next poll.
On Grok API Monitor, add a Management API key (see below).

## Grok Bot Monitor

A second applet, launched as `cosmic-ext-applet-grok-monitor --product=bot`.
It is independent of Grok Monitor: separate panel chip, settings, and account.

The chip shows Grok Bot weekly usage as a whole percent (or `n/a` on an
enterprise pool). Click for reset time, optional on-demand spend, whether the
Grok Bot app is running, and up to three recently active bots.

Auth comes from the Grok Bot desktop app (`~/.config/Grok Bot/sand-secrets.json`
plus the login keyring). Tokens are read-only. The applet uses the active
account in `cursor-accounts` and still accepts the older top-level
`cursor-access-token` field. If you see `—`, open Grok Bot and sign in, then
wait for the next poll.

This is not Grok Chat. The Grok CLI billing `productUsage` list has GrokBuild
and GrokChat; Grok Bot usage is a Cursor Sand ledger.

## Grok API Monitor

A third applet, launched as `cosmic-ext-applet-grok-monitor --product=api`.
It is independent of the others: separate panel chip, settings, and credentials.

The chip shows remaining **prepaid API dollars** (the same remaining-credit
figure as [console.x.ai](https://console.x.ai) billing). Click for optional
spend this billing period, up to three API key names on the team, and
**Open console**.

Auth is a **Management API key**, not an inference API token. Create one in the
xAI Console under **Settings → Management Keys** with billing read access, then
write it to `~/.config/grok-mon-api/credentials.json` (`chmod 600` recommended):

```json
{
  "management_key": "xai-...",
  "team_id": "optional-uuid"
}
```

`team_id` is optional for a team-scoped key (the applet reads it from key
validation). Organization-scoped keys need `team_id` or `XAI_TEAM_ID`.
Environment variables `XAI_MANAGEMENT_API_KEY` (or `XAI_MANAGEMENT_KEY`) and
`XAI_TEAM_ID` override the file when the panel process has them.

The applet prefers live remaining from the invoice preview
(`/v1/billing/teams/{team}/postpaid/invoice/preview`) and falls back to the
posted prepaid ledger (`/v1/billing/teams/{team}/prepaid/balance`). Ledger
amounts are inverted USD cents (`"-1000"` is $10 remaining). The applet never
treats a missing total as $0.00.

If you see `—`, the Management API key is missing or rejected. Inference keys
(`XAI_API_KEY`) are not accepted.

## Settings

Open the popup and choose **Settings**.

| Setting | Options | Default |
|---------|---------|---------|
| Poll interval | 30s, 60s, 5m | 60s |
| Sparkline on panel | on / off | off |
| Percent on panel | on / off | on |
| Amount on panel (API) | on / off | on |
| Panel number | used / remaining | used |

The ring fill matches the panel number (used or remaining). Color is always
based on percent used: green through 50%, yellow through 80%, orange through
90%, red above that, with a distinct shade at each whole percent. Settings are
stored in COSMIC config under app id
`io.github.simple-systems-se.grok-mon` (Build),
`io.github.simple-systems-se.grok-mon-bot` (Bot), or
`io.github.simple-systems-se.grok-mon-api` (API).

API has no used/remaining toggle: the chip is remaining dollars. The ring is
full at $50 remaining.

## Authentication

Grok Monitor reads the Grok CLI’s OIDC bearer from `~/.grok/auth.json` (or
`$GROK_HOME/auth.json` if that is set). It prefers the `https://auth.x.ai::`
entry written by `grok login`.

Grok Bot Monitor decrypts the Cursor access token stored by the Grok Bot app
in `~/.config/Grok Bot/sand-secrets.json` using the login keyring item
`application=Grok Bot` (preferring `xdg:schema=chrome_libsecret_os_crypt_password_v2`).
The system may prompt to unlock that item; the prompt is labeled by Grok Bot,
not by this applet. It prefers the active `cursor-accounts` entry and falls
back to a top-level `cursor-access-token` if that older layout is still
present. It does not refresh tokens. When Grok Bot refreshes them, the next
poll picks up the new file contents.

Bot mode is unofficial. It reuses the local Grok Bot session to call Cursor’s
private usage endpoints (`api2.cursor.sh` DashboardService). Those APIs are
unsupported and can change or break without notice. This project is not
affiliated with, endorsed by, or supported by Cursor or xAI.

Grok API Monitor reads a Management API key from
`~/.config/grok-mon-api/credentials.json` and/or `XAI_MANAGEMENT_API_KEY`
(`XAI_MANAGEMENT_KEY`) plus optional `XAI_TEAM_ID`. It does not write that
file. Inference keys are rejected.

None of the applets write credentials.

## Privacy

Grok Monitor:

- Reads `~/.grok/auth.json` (or `$GROK_HOME/auth.json`) and
  `~/.grok/active_sessions.json`.
- Sends the Grok CLI bearer only to `https://cli-chat-proxy.grok.com`
  (`/v1/billing`, `/v1/settings`).

Grok Bot Monitor:

- Reads `~/.config/Grok Bot/sand-secrets.json`,
  `sand-session-marker.json`, and `sand-client-persistence` (bot names and
  unread counts).
- Reads the login keyring item for Grok Bot Safe Storage (decrypt only).
- Sends the decrypted Cursor bearer only to `https://api2.cursor.sh`
  DashboardService usage methods (`GetSandUsageStatus`,
  `GetCurrentPeriodUsage`), plus `x-cursor-checksum` (derived from the local
  machine id), `x-cursor-client-version`, and a random `x-request-id`.

Grok API Monitor:

- Reads `~/.config/grok-mon-api/credentials.json` and the
  `XAI_MANAGEMENT_API_KEY` / `XAI_MANAGEMENT_KEY` / `XAI_TEAM_ID` environment
  variables when present.
- Sends the Management API key only to `https://management-api.x.ai`
  (`/auth/management-keys/validation`, `/v1/billing/teams/{team}/prepaid/balance`,
  `/v1/billing/teams/{team}/postpaid/invoice/preview`,
  `/auth/teams/{team}/api-keys`).

None of the applets:

- Writes credentials back to disk.
- Logs tokens. The Authorization header and Bot checksum header are marked
  sensitive so HTTP debug traces redact them. In-memory secrets are zeroized
  when dropped.

## Uninstall

```sh
just uninstall-user
```

Without `just`:

```sh
rm -f ~/.local/bin/cosmic-ext-applet-grok-monitor \
  ~/.local/share/applications/io.github.simple-systems-se.grok-mon.desktop \
  ~/.local/share/applications/io.github.simple-systems-se.grok-mon-bot.desktop \
  ~/.local/share/applications/io.github.simple-systems-se.grok-mon-api.desktop \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-symbolic.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-bot.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-bot-symbolic.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-api.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-api-symbolic.svg \
  ~/.local/share/metainfo/io.github.simple-systems-se.grok-mon.metainfo.xml \
  ~/.local/share/metainfo/io.github.simple-systems-se.grok-mon-bot.metainfo.xml \
  ~/.local/share/metainfo/io.github.simple-systems-se.grok-mon-api.metainfo.xml
```

Then remove **Grok Monitor**, **Grok Bot Monitor**, or **Grok API Monitor** from
the panel if they are still listed.

Settings stay in COSMIC config until you purge them:

```sh
just purge-user
```

That also removes:

```
~/.config/cosmic/io.github.simple-systems-se.grok-mon/
~/.config/cosmic/io.github.simple-systems-se.grok-mon-bot/
~/.config/cosmic/io.github.simple-systems-se.grok-mon-api/
```

It does not delete `~/.config/grok-mon-api/credentials.json`.

## Development

```sh
just test
just check
just build-release
just run
just run -- --product=bot
just run -- --product=api
```

Or with Cargo:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
RUST_LOG=cosmic_ext_applet_grok_monitor=info cargo run --release
RUST_LOG=cosmic_ext_applet_grok_monitor=info cargo run --release -- --product=bot
RUST_LOG=cosmic_ext_applet_grok_monitor=info cargo run --release -- --product=api
```

## License

This is free and unencumbered software released into the public domain.
See [LICENSE](LICENSE) (Unlicense).

[libcosmic](https://github.com/pop-os/libcosmic) is MPL-2.0. Other crates keep
their own licenses; see `Cargo.lock`.

Grok is a trademark of xAI. Cursor is a trademark of Anysphere. COSMIC is a
trademark of System76. The names are used here only to identify Grok Build,
Grok Bot, the COSMIC™ desktop, and the usage endpoints those apps already
call. This is an unofficial community project.

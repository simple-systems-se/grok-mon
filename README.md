# Grok Monitor

A panel applet for the [COSMIC™](https://github.com/pop-os/cosmic-epoch) desktop that shows current
Grok Build credit usage. It uses the same billing endpoint as `/usage` in the
Grok CLI. A second applet, **Grok Bot Monitor**, shows Grok Bot weekly usage
from the Grok Bot desktop app.

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
```

Add **Grok Monitor** or **Grok Bot Monitor** in **COSMIC Settings → Desktop →
Panel → Applets**. If they do not appear, restart the panel:

```sh
pkill cosmic-panel
```

## Usage

The panel chip shows a circular usage ring (the same shape Minimon uses for
memory) with a small **hammer** (Build) or **robot** (Bot) in the center. By
default it also shows current usage as a whole percent. Settings can hide the
number or switch the number and ring fill to percent remaining. Color follows
percent **used**, stepped each whole percent: green 0–50%, yellow 50–80%,
orange 80–90%, red 90%+.

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

If you see `—`, run `grok login` and wait for the next poll.

## Grok Bot Monitor

A second applet, launched as `cosmic-ext-applet-grok-monitor --product=bot`.
It is independent of Grok Monitor: separate panel chip, settings, and account.

The chip shows Grok Bot weekly usage as a whole percent (or `n/a` on an
enterprise pool). Click for reset time, optional on-demand spend, whether the
Grok Bot app is running, and up to three recently active bots.

Auth comes from the Grok Bot desktop app (`~/.config/Grok Bot/sand-secrets.json`
plus the login keyring). Tokens are read-only. If you see `—`, open Grok Bot and
sign in, then wait for the next poll.

This is not Grok Chat. The Grok CLI billing `productUsage` list has GrokBuild
and GrokChat; Grok Bot usage is a Cursor Sand ledger.

## Settings

Open the popup and choose **Settings**.

| Setting | Options | Default |
|---------|---------|---------|
| Poll interval | 30s, 60s, 5m | 60s |
| Sparkline on panel | on / off | off |
| Percent on panel | on / off | on |
| Panel number | used / remaining | used |

The ring fill matches the panel number (used or remaining). Color is always
based on percent used: green through 50%, yellow through 80%, orange through
90%, red above that, with a distinct shade at each whole percent. Settings are
stored in COSMIC config under app id
`io.github.simple-systems-se.grok-mon` (Build) or
`io.github.simple-systems-se.grok-mon-bot` (Bot).

## Authentication

Grok Monitor reads the Grok CLI’s OIDC bearer from `~/.grok/auth.json` (or
`$GROK_HOME/auth.json` if that is set). It prefers the `https://auth.x.ai::`
entry written by `grok login`.

Grok Bot Monitor decrypts the Cursor access token stored by the Grok Bot app
in `~/.config/Grok Bot/sand-secrets.json` using the login keyring item
`application=Grok Bot` (preferring `xdg:schema=chrome_libsecret_os_crypt_password_v2`).
The system may prompt to unlock that item; the prompt is labeled by Grok Bot,
not by this applet. It does not refresh tokens. When Grok Bot refreshes them,
the next poll picks up the new file contents.

Bot mode is unofficial. It reuses the local Grok Bot session to call Cursor’s
private usage endpoints (`api2.cursor.sh` DashboardService). Those APIs are
unsupported and can change or break without notice. This project is not
affiliated with, endorsed by, or supported by Cursor or xAI.

Neither applet writes credentials.

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

Neither applet:

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
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-symbolic.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-bot.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-bot-symbolic.svg \
  ~/.local/share/metainfo/io.github.simple-systems-se.grok-mon.metainfo.xml \
  ~/.local/share/metainfo/io.github.simple-systems-se.grok-mon-bot.metainfo.xml
```

Then remove **Grok Monitor** or **Grok Bot Monitor** from the panel if they are
still listed.

Settings stay in COSMIC config until you purge them:

```sh
just purge-user
```

That also removes:

```
~/.config/cosmic/io.github.simple-systems-se.grok-mon/
~/.config/cosmic/io.github.simple-systems-se.grok-mon-bot/
```

## Development

```sh
just test
just check
just build-release
just run
just run -- --product=bot
```

Or with Cargo:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
RUST_LOG=cosmic_ext_applet_grok_monitor=info cargo run --release
RUST_LOG=cosmic_ext_applet_grok_monitor=info cargo run --release -- --product=bot
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

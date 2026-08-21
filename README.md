# Grok Monitor

A [COSMIC](https://github.com/pop-os/cosmic-epoch) panel applet that shows current
Grok Build credit usage. It uses the same billing endpoint as `/usage` in the
Grok CLI.

This is an unofficial community project. It is not affiliated with, endorsed by,
or supported by xAI.

## Requirements

- The COSMIC desktop
- A signed-in [Grok CLI](https://grok.com) session (`grok login`)
- Rust 1.88 or newer (edition 2024), via [rustup](https://rustup.rs)
- `pkg-config`, a C toolchain, and the libxkbcommon development headers
- [`just`](https://github.com/casey/just) (optional; wraps the Cargo and install
  commands below)

On Debian, Ubuntu, and Pop!_OS:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev
```

Other distributions need the equivalent packages (`libxkbcommon-devel` on Fedora).

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
```

Add **Grok Monitor** in **COSMIC Settings → Desktop → Panel → Applets**. If it
does not appear, restart the panel:

```sh
pkill cosmic-panel
```

## Usage

The panel chip shows a **G** icon and current usage as a whole percent. Color
follows the COSMIC theme: normal text below 70%, warning at 70%, destructive at
90%.

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

## Settings

Open the popup and choose **Settings**.

| Setting | Options | Default |
|---------|---------|---------|
| Poll interval | 30s, 60s, 5m | 60s |
| Sparkline on panel | on / off | off |

Warning and critical thresholds are currently 70% and 90%. Settings are stored
in COSMIC config under app id `io.github.simple-systems-se.grok-mon`.

## Authentication

The applet reads the Grok CLI’s OIDC bearer from `~/.grok/auth.json` (or
`$GROK_HOME/auth.json` if that is set). It prefers the `https://auth.x.ai::`
entry written by `grok login`.

It does not refresh tokens. When the CLI refreshes them, the next poll picks up
the new file contents.

## Privacy

- The bearer token is used only to call `https://cli-chat-proxy.grok.com`.
- Tokens are never written back to disk.
- Tokens are never logged. The Authorization header is marked sensitive so HTTP
  debug traces redact it. The in-memory bearer is cleared when the request
  finishes.

## Uninstall

```sh
just uninstall-user
```

Without `just`:

```sh
rm -f ~/.local/bin/cosmic-ext-applet-grok-monitor \
  ~/.local/share/applications/io.github.simple-systems-se.grok-mon.desktop \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.simple-systems-se.grok-mon-symbolic.svg \
  ~/.local/share/metainfo/io.github.simple-systems-se.grok-mon.metainfo.xml
```

Then remove **Grok Monitor** from the panel if it is still listed.

## Development

```sh
just test
just build-release
just run
```

Or with Cargo:

```sh
cargo test
cargo build --release
RUST_LOG=cosmic_ext_applet_grok_monitor=info cargo run --release
```

## License

This is free and unencumbered software released into the public domain.
See [LICENSE](LICENSE) (Unlicense).

Grok is a trademark of xAI. The name is used here only to identify Grok Build
usage.

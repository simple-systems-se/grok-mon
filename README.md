# Grok Monitor

A panel applet for the [COSMIC™](https://github.com/pop-os/cosmic-epoch) desktop that shows the
shared weekly Grok usage pool **once per account**. Grok Build and Grok Bot draw
from that pool on current xAI plans, so a second Bot meter would count the same
usage again. The chip uses the same billing endpoint as `/usage` in the Grok
CLI (`creditUsagePercent`). A separate applet, **Grok API Monitor**, still shows
remaining prepaid dollars for xAI API tokens.

The work account is labeled **Grok (sim sys)**. The personal account is labeled **Grok (pers)**.

**Grok Bot Monitor** is a retired panel slot. It no longer polls usage. Remove
it from the panel after updating (see below). That step is done in COSMIC
Settings; the package does not edit the panel layout.

This is an unofficial community project. It is not affiliated with, endorsed by,
or supported by xAI or Cursor.

## Requirements

Shared:

- The COSMIC™ desktop
- Rust 1.88 or newer (edition 2024), via [rustup](https://rustup.rs)
- `pkg-config`, a C toolchain, and the libxkbcommon development headers
- [`just`](https://github.com/casey/just) (optional; wraps the Cargo and install
  commands below)

Grok Monitor:

- A signed-in [Grok CLI](https://grok.com) session for each account (`grok login`)
- Optional: Grok Bot userData dirs (`Grok Bot` and `Grok Bot Work`) so the
  applet can tell the personal profile from the work profile. Tokens from those
  dirs are not sent anywhere. v11 secrets need an unlocked login keyring;
  Chromium v10 secrets (typical on COSMIC) do not

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

On Pop!_OS 24 (or Ubuntu 24.04 amd64), install a release `.deb`:

```sh
sudo apt install ./cosmic-ext-applet-grok-monitor_*_amd64.deb
```

Download that package from
[GitHub Releases](https://github.com/simple-systems-se/grok-mon/releases).
Each release also has a stripped binary tarball if you prefer not to use `apt`.

To build from source and install for the current user:

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

Add **Grok Monitor** or **Grok API Monitor** in
**COSMIC Settings → Desktop → Panel → Applets**. Do not add **Grok Bot Monitor**.
If a new applet does not appear, restart the panel:

```sh
pkill cosmic-panel
```

`just install-user` installs only for the user running it (`~/.local`). It does
not change other users, and it does not remove an applet that is already pinned
to the panel.

## Update on Pop!_OS COSMIC

Grok Build and Grok Bot now share one weekly pool. After you install or upgrade:

1. Open **COSMIC Settings → Desktop → Panel → Applets**.
2. Remove **Grok Bot Monitor** if it is on the panel. This is a manual step.
   The package keeps the old desktop file so the slot does not go blank, and
   that slot now only tells you to remove it. It does not fetch usage.
3. Keep **Grok Monitor**. It shows one chip for the work account (**Grok (sim sys)**)
   and one for the personal account (**Grok (pers)**).
4. Keep **Grok API Monitor** if you watch prepaid API credits. That applet is
   unchanged.
5. If the updated applet does not show up, run `pkill cosmic-panel`.

Sign-in files are left in place: `~/.grok/auth.json`, `~/.config/Grok Bot*`,
and `~/.config/grok-mon-api/credentials.json`. Settings already stored for
Grok Monitor stay (poll interval, sparkline, used/remaining, pace). A saved
panel label of `work`, `sim sys`, or `bot (work)` becomes `Grok (sim sys)`, and
`personal`, `pers`, or `bot (pers)` becomes `Grok (pers)`. A saved label of
`build` is kept until that sign-in is explicitly joined to an account. Any
other custom label is kept. Labels that lived only on the Bot applet are copied
onto Grok Monitor when that chip does not already have one.

To install the upgrade for the current user without `sudo`, from a git checkout:

```sh
just install-user
```

Or, for a system package on Pop!_OS 24 / Ubuntu 24.04 amd64:

```sh
sudo apt install ./cosmic-ext-applet-grok-monitor_*_amd64.deb
```

`apt install` of a newer `.deb` replaces the binary. It still does not edit the
panel. Do the applet removal in Settings after the package is installed.

## Usage

The panel chip shows a circular usage ring (the same shape Minimon uses for
memory) with a small **hammer** (Grok) or **key** (API) in the center, plus a
label. Grok shows one chip per account. By default that chip shows current
usage of the shared pool as a whole percent. API shows remaining prepaid
dollars. Settings can hide the number or (for Grok) switch the number and ring
fill to percent remaining.
Color follows percent **used**, stepped each whole percent: green 0–50%, yellow
50–80%, orange 80–90%, red 90%+. For API, that percent used is remaining
prepaid mapped against a $50 full wallet (green above $25, yellow through $10,
orange through $5, red under $5).

Click a chip for a popup with:

- Plan name and account email
- Usage bar (0–100, colored like the ring) and percent
- **Shared weekly pool** when the account is on unified billing
- Weekly or monthly reset time
- Weekly pace: time until credits run out at the current burn, or how much is left at reset if they will last the week
- How long ago usage was fetched
- Live Grok CLI session count (from `~/.grok/active_sessions.json`)
- **Open usage** (opens [grok.com usage](https://grok.com/?_s=usage))
- **Copy** (copies the percent)
- **Settings**

Panel states:

| Chip | Meaning |
|------|---------|
| `…` | Fetching |
| `—` | No Grok CLI login is attached to this chip, or that CLI session expired |
| `?` | Request failed, and there is no previous reading |
| dimmed percent | Last good reading; the latest poll failed |

If you see `—` on Grok Monitor, that chip has no Grok CLI login attached.
A Grok Bot profile can still be present. The mark is not a sign-out check.
Join the Build sign-in under **Same weekly pool** when it belongs to that
profile. The weekly percent is the CLI reading for the joined profile only.
Run `grok login` when the CLI session itself is missing or expired.
On Grok API Monitor, add a Management API key (see below).

## Accounts

Grok Monitor keeps the work account and the personal account separate. It does
not add their percents together.

| Account | Panel label | How it is recognized |
|---------|-------------|----------------------|
| Work | `Grok (sim sys)` | Grok Bot profile directory `Grok Bot Work`, or a saved label `work` / `sim sys` / `bot (work)` |
| Personal | `Grok (pers)` | Grok Bot profile directory `Grok Bot`, or a saved label `personal` / `pers` / `bot (pers)` |

The same email signed in to both the Grok CLI and Grok Bot is **one** chip.
The percent is that account’s `creditUsagePercent` from
`GET https://cli-chat-proxy.grok.com/v1/billing?format=credits`. On a unified
plan that number is the shared weekly pool (Build, Bot, Chat, and the other
Grok products). `productUsage` rows are a breakdown of the same pool. They are
not added to the headline, and the old Cursor Sand / Grok Bot ledger is not
added either.

If the headline percent is missing on a unified plan, the chip does not invent
a total by summing product rows. A legacy plan that is not unified still uses
the Grok Build row only, when that is all the payload has.

An account that exists only as a Grok Bot profile, with no Grok CLI login, is
still listed so it is not dropped. Its chip stays at `—` until `grok login`
for that account. This build does not fall back to the old Bot percent.

When exactly two accounts are present and only one role is known, the other
becomes the partner label (`Grok (sim sys)` or `Grok (pers)`). A third account
is not guessed. A custom panel label other than the legacy work and personal
captions is left as you set it.

A Grok CLI sign-in whose email is missing, or different from the Grok Bot
profile, stays on its own chip. To join it, open that chip’s **Settings** and
choose **Grok (sim sys)** or **Grok (pers)** under **Same weekly pool**, or write
the same choice into COSMIC config. The entry is `account_links` for app id
`io.github.simple-systems-se.grok-mon`, version 1. Its value is a map from the
sign-in’s account id to `work` or `personal`. The join runs only when exactly
one discovered profile already has that role and the sign-in is not already
attached to the other role by email. It does not run from the panel caption.
An unmapped sign-in stays visible. The weekly percent is the
CLI login’s shared pool; a profile with no CLI login stays at `—`.

## Retired Grok Bot Monitor

`cosmic-ext-applet-grok-monitor --product=bot` no longer fetches usage. If the
old applet is still on the panel it shows a removal note instead of a second
meter. Remove it with the steps in **Update on Pop!_OS COSMIC**.

The applet can still read Grok Bot profile directories to match the work and
personal accounts to CLI logins. It does not call `api2.cursor.sh`.

### Weekly pace

The shared pool is weekly on current plans. The popup adds a short line about when the
remaining quota hits zero if the current burn continues:

- `Will run out in 2d 4h` when that time is **before** the next reset
- `Won't run out before reset · ~60% left at reset` when the quota lasts the week
- `Week just started` early in the week, while usage is still too small to trust
- `Out of credits` at 100% used, or `At limit` within half a percent of the cap
- `Pace unavailable` when the window is marked weekly but start and end are unusable

The burn rate is **constant from the period start**: used percent divided by
how long the period has been open. Time to exhaustion is the remaining percent
divided by that rate. The payload has no separate “since the last poll” rate,
so this is the rate the applet can actually support. `~N% left` is the unused
percent at reset under that same burn (`100 − used% / elapsed fraction`). A
reading that is exactly even (`used %` equals elapsed % of the week) lands on
`~0% left at reset`. An 80% used reading halfway through the week runs out in
about 21 hours, not at reset.

The line does not call a hot burn “on track.” The popup already has a separate
reset countdown, so the pace line leads with exhaustion time when credits will
not last the week.

The first ~1% of the week (and any moment before the period opens) shows
`Week just started` unless usage is already at least 5%. That avoids a huge or
jumpy ETA from a tiny sample. A fast start that has already used 5% or more
still reports `Will run out in …`.

Pace uses `currentPeriod.start` → `end` from the billing payload when present.
Pace is hidden when there is no reset time, or when the window is not weekly
(monthly/daily). API prepaid has no weekly quota window, so that applet has no
pace line. Settings can hide the line; it is on by default.

Profile discovery, used only to tell accounts apart:

- Auto-discovery: directories under `$XDG_CONFIG_HOME` (or `~/.config`) whose
  names start with `Grok Bot` and that contain `sand-secrets.json`. That
  includes the default `~/.config/Grok Bot` and extra profiles such as
  `~/.config/Grok Bot Work`.
- Extra roots: colon-separated paths in `GROK_BOT_CONFIG_DIRS` (a `~/` prefix
  is expanded). Use this only for dirs that do not match the `Grok Bot*` name.

Tokens are read-only. Chromium v10 blobs use OSCrypt’s built-in password;
v11 blobs use the login keyring item whose `application=` label matches that
directory’s name (`Grok Bot`, `Grok Bot Work`, …), then fall back
to `application=Grok Bot`. Discovery reads `cursor-accounts` (active first) and still accepts the older
top-level `cursor-access-token` field. The same email in more than one
directory is one account. The CLI login is the one used for the usage request.

If you see `—`, run `grok login` for that account and wait for the next poll.

## Grok API Monitor

The other applet, launched as `cosmic-ext-applet-grok-monitor --product=api`.
It is independent: separate panel chip, settings, and credentials. Prepaid
monitoring is unchanged.

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

The applet computes live remaining as the lower of two figures when both
exist: the posted prepaid ledger
(`/v1/billing/teams/{team}/prepaid/balance`) minus current-period spend, and
the invoice preview's `prepaidCredits`
(`/v1/billing/teams/{team}/postpaid/invoice/preview`). Spend is the largest
of `prepaidCreditsUsed`, `totalWithCorr`, and invoice line amounts. Mid-cycle,
preview `prepaidCredits` can lag live spend, so posted minus spend is lower.
At a billing-cycle change the previous invoice may not be posted to the
ledger yet, while preview `prepaidCredits` already includes that spend and
`prepaidCreditsUsed` is still zero, so the preview figure is lower. When the
ledger is missing, preview remaining is the fallback. Ledger amounts are
inverted USD cents (`"-1000"` is $10 remaining). The applet never treats a
missing total as $0.00.

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
| Pace in popup (Grok) | on / off | on |
| Panel label | text | `Grok (sim sys)`, `Grok (pers)`, account email, or API key name |
| Same weekly pool | separate / work / personal | separate |
| Panel number | used / remaining | used |

The ring fill matches the panel number (used or remaining). Color is always
based on percent used: green through 50%, yellow through 80%, orange through
90%, red above that, with a distinct shade at each whole percent. Settings are
stored in COSMIC config under app id
`io.github.simple-systems-se.grok-mon` (Grok) or
`io.github.simple-systems-se.grok-mon-api` (API).
The retired Bot id `io.github.simple-systems-se.grok-mon-bot` is left on disk
and is only read so its labels can be copied once. It is not overwritten.

API has no used/remaining toggle: the chip is remaining dollars. The ring is
full at $50 remaining.

## Authentication

Grok Monitor reads the Grok CLI’s OIDC bearer from `~/.grok/auth.json` (or
`$GROK_HOME/auth.json` if that is set). It prefers unexpired `https://auth.x.ai::`
entries written by `grok login`. Each unexpired entry is its own account.
The bearer is sent only to the Grok CLI billing and settings endpoints.

To tell `Grok (sim sys)` from `Grok (pers)`, the applet also reads Grok Bot profile
directories (`~/.config/Grok Bot*`, plus `GROK_BOT_CONFIG_DIRS`). It decrypts
`sand-secrets.json` only far enough to read the account email and id, then
drops the token. It does not refresh tokens, does not change which account
Grok Bot considers active, and does not call Cursor. Chromium v10 ciphertext
uses the built-in OSCrypt password (`peanuts`); v11 ciphertext uses the login
keyring item whose `application=` label is the directory name. On COSMIC,
Grok Bot typically writes v10. A v11 file can prompt to unlock the keyring;
that prompt is labeled by Grok Bot, not by this applet.

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

Grok Monitor also reads Grok Bot profile files when they are present, to match
accounts. It does not send those tokens off the machine. The retired Grok Bot
Monitor slot does not poll.

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
- Logs tokens. The Authorization header is marked sensitive so HTTP debug
  traces redact it. In-memory secrets are zeroized when dropped.

## Uninstall

If you installed the `.deb`:

```sh
sudo apt remove cosmic-ext-applet-grok-monitor
```

If you installed with `just install-user`:

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

Then remove **Grok Monitor**, **Grok Bot Monitor**, and **Grok API Monitor** from
the panel if they are still listed
(**COSMIC Settings → Desktop → Panel → Applets**).

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
just package-linux
just run
just run -- --product=api
```

Tagged `v*` pushes run `.github/workflows/release.yml` on `ubuntu-24.04`
and attach a `.deb` plus binary tarball to a GitHub Release. Use
**Actions → Release → Run workflow** to build artifacts without publishing.

Or with Cargo:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
RUST_LOG=cosmic_ext_applet_grok_monitor=info cargo run --release
RUST_LOG=cosmic_ext_applet_grok_monitor=info cargo run --release -- --product=api
```

## License

This is free and unencumbered software released into the public domain.
See [LICENSE](LICENSE) (Unlicense).

[libcosmic](https://github.com/pop-os/libcosmic) is MPL-2.0. Other crates keep
their own licenses; see `Cargo.lock`.

Grok is a trademark of xAI. Cursor is a trademark of Anysphere. COSMIC is a
trademark of System76. The names are used here only to identify the Grok
products, the COSMIC™ desktop, and the usage endpoint the Grok CLI already
calls. This is an unofficial community project.

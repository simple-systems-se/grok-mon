# Changelog

## 0.4.2

- The panel chip no longer stays on `…` after a successful device-code sign-in
  when the usage request then fails. An expired session shows `—`. Any other
  failure shows `?` and the popup error.
- A missing `grok` binary no longer surfaces a raw operating-system error
  number from the process spawn. The popup keeps the “grok CLI was not found”
  text.
- Usage logs no longer include the panel label.
- The retired Grok Bot Monitor slot only shows the removal note. Opening it
  does not read Grok Bot profile directories, decrypt secrets, or fetch a
  second usage ledger. The pre-0.4.0 scanner and its crypto dependencies are
  gone.
- Direct dependencies that remain are updated within their current major
  versions. libcosmic stays on the git revision this tree builds.
- The README covers install, accounts, every config key, sign-in, and
  troubleshooting, with illustration SVGs under `docs/`.

## 0.4.1

- Find `grok` when it is not on the panel `PATH`, including `~/.grok/bin` and
  nvm.
- Paint the ring, usage bar, and popup on opaque theme colors.

## 0.4.0

- Show only configured account icons, in config order.
- Sign in from each icon with `grok login --device-code` for that icon's CLI
  home.
- Default icons are Grok (sim sys) at `~/.grok` and Grok (pers) at
  `~/.grok-personal`.

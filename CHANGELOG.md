# Changelog

## 0.4.5

- The usage bar and the time bar in a signed-in popup are the same width.
  The label sits in a fixed column, so a longer label (`72% remaining`,
  `100% remaining`) does not shorten its bar relative to `3h 12m`. The same
  layout is used when the popup is only the two bars and when an error or
  sign-in control is also shown.

## 0.4.4

- A signed-in Grok popup that has a reading shows two bars and nothing else.
  The usage bar is percent remaining (`100 − used`, clamped to 0–100),
  labeled `72% remaining`. The bar under it is the fraction of the current
  window still left, labeled only with the compact time until reset
  (`3h 12m`, `2d 4h`).
- Reset sentences, the pace line, the plan, the email, and the other status
  lines are not shown in that state. Sign-in, errors, and loading stay.
- If the payload has a reset time but no positive window, the duration label
  is shown and the time bar is empty. A missing reset time uses `—` and an
  empty bar. The applet does not assume a 7-day week.

## 0.4.3

- When the access token in a chip's CLI home is within 5 minutes of
  `expires_at`, or already past it, the applet runs `grok models` once for
  that home so the CLI can refresh the session. The child is not a TTY,
  `GROK_HOME` is set only on that child, and the run is stopped after 60
  seconds. It is not `grok login` or `grok logout`.
- The applet re-reads `auth.json` after that command. A failure waits before
  the next try (5 minutes, doubling, capped at 6 hours) so a poll does not
  spawn `grok` in a loop. If the session is still expired, the chip stays on
  `—` and the popup keeps Sign in.
- Logs for that attempt name the chip index and the CLI home, and say whether
  it succeeded or failed (exit code or timeout). They do not include tokens
  or email addresses.

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

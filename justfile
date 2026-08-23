name := "cosmic-ext-applet-grok-monitor"
appid := "io.github.simple-systems-se.grok-mon"
botid := "io.github.simple-systems-se.grok-mon-bot"

home := env("HOME")
cargo-target-dir := env("CARGO_TARGET_DIR", "target")

bin-src := cargo-target-dir / "release" / name
bin-dst := home / ".local/bin" / name
desktop-src := "res" / appid + ".desktop"
desktop-dst := home / ".local/share/applications" / appid + ".desktop"
bot-desktop-src := "res" / botid + ".desktop"
bot-desktop-dst := home / ".local/share/applications" / botid + ".desktop"
icon-src := "res/icons/hicolor/scalable/apps" / appid + ".svg"
icon-dst := home / ".local/share/icons/hicolor/scalable/apps" / appid + ".svg"
icon-symbolic-dst := home / ".local/share/icons/hicolor/scalable/apps" / appid + "-symbolic.svg"
bot-icon-src := "res/icons/hicolor/scalable/apps" / botid + ".svg"
bot-icon-dst := home / ".local/share/icons/hicolor/scalable/apps" / botid + ".svg"
bot-icon-symbolic-dst := home / ".local/share/icons/hicolor/scalable/apps" / botid + "-symbolic.svg"
metainfo-src := "res" / appid + ".metainfo.xml"
metainfo-dst := home / ".local/share/metainfo" / appid + ".metainfo.xml"
bot-metainfo-src := "res" / botid + ".metainfo.xml"
bot-metainfo-dst := home / ".local/share/metainfo" / botid + ".metainfo.xml"
config-dst := home / ".config/cosmic" / appid
bot-config-dst := home / ".config/cosmic" / botid

default: build-release

build-debug *args:
    cargo build {{args}}

build-release *args: (build-debug "--release" args)

test *args:
    cargo test {{args}}

fmt:
    cargo fmt

check *args:
    cargo clippy --all-targets {{args}} -- -D warnings

run *args:
    env RUST_LOG=cosmic_ext_applet_grok_monitor=info RUST_BACKTRACE=1 cargo run --release -- {{args}}

install-user: build-release
    install -Dm0755 {{bin-src}} {{bin-dst}}
    mkdir -p {{home}}/.local/share/applications
    sed 's|^Exec=.*|Exec={{bin-dst}}|' {{desktop-src}} > {{desktop-dst}}
    sed 's|^Exec=.*|Exec={{bin-dst}} --product=bot|' {{bot-desktop-src}} > {{bot-desktop-dst}}
    install -Dm0644 {{icon-src}} {{icon-dst}}
    install -Dm0644 {{icon-src}} {{icon-symbolic-dst}}
    install -Dm0644 {{bot-icon-src}} {{bot-icon-dst}}
    install -Dm0644 {{bot-icon-src}} {{bot-icon-symbolic-dst}}
    install -Dm0644 {{metainfo-src}} {{metainfo-dst}}
    install -Dm0644 {{bot-metainfo-src}} {{bot-metainfo-dst}}
    @echo "Installed to ~/.local. Add “Grok Monitor” or “Grok Bot Monitor” in COSMIC Settings → Desktop → Panel → Applets."
    @echo "If they do not appear, run: pkill cosmic-panel"

uninstall-user:
    rm -f {{bin-dst}} {{desktop-dst}} {{icon-dst}} {{icon-symbolic-dst}} {{metainfo-dst}} \
      {{bot-desktop-dst}} {{bot-icon-dst}} {{bot-icon-symbolic-dst}} {{bot-metainfo-dst}}

purge-user: uninstall-user
    rm -rf {{config-dst}} {{bot-config-dst}}

clean:
    cargo clean

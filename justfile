name := "cosmic-ext-applet-grok-monitor"
appid := "io.github.simple-systems-se.grok-mon"

home := env("HOME")
cargo-target-dir := env("CARGO_TARGET_DIR", "target")

bin-src := cargo-target-dir / "release" / name
bin-dst := home / ".local/bin" / name
desktop-src := "res" / appid + ".desktop"
desktop-dst := home / ".local/share/applications" / appid + ".desktop"
icon-src := "res/icons/hicolor/scalable/apps" / appid + ".svg"
icon-dst := home / ".local/share/icons/hicolor/scalable/apps" / appid + ".svg"
icon-symbolic-dst := home / ".local/share/icons/hicolor/scalable/apps" / appid + "-symbolic.svg"
metainfo-src := "res" / appid + ".metainfo.xml"
metainfo-dst := home / ".local/share/metainfo" / appid + ".metainfo.xml"

default: build-release

build-debug *args:
    cargo build {{args}}

build-release *args: (build-debug "--release" args)

test *args:
    cargo test {{args}}

run *args:
    env RUST_LOG=cosmic_ext_applet_grok_monitor=info RUST_BACKTRACE=1 cargo run --release {{args}}

install-user: build-release
    install -Dm0755 {{bin-src}} {{bin-dst}}
    mkdir -p {{home}}/.local/share/applications
    sed 's|^Exec=.*|Exec={{bin-dst}}|' {{desktop-src}} > {{desktop-dst}}
    install -Dm0644 {{icon-src}} {{icon-dst}}
    install -Dm0644 {{icon-src}} {{icon-symbolic-dst}}
    install -Dm0644 {{metainfo-src}} {{metainfo-dst}}
    @echo "Installed to ~/.local. Add “Grok Monitor” in COSMIC Settings → Desktop → Panel → Applets."
    @echo "If it does not appear, run: pkill cosmic-panel"

uninstall-user:
    rm -f {{bin-dst}} {{desktop-dst}} {{icon-dst}} {{icon-symbolic-dst}} {{metainfo-dst}}

clean:
    cargo clean

#!/usr/bin/env bash
# Stage a Pop!_OS 24 / Ubuntu 24.04 amd64 .deb and a plain binary tarball
# from an existing cargo release build. Does not compile.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

name="cosmic-ext-applet-grok-monitor"
appid="io.github.simple-systems-se.grok-mon"
ids=("$appid" "${appid}-bot" "${appid}-api")
arch="${ARCH:-amd64}"
cargo_target_dir="${CARGO_TARGET_DIR:-target}"
bin_src="${BIN_SRC:-$cargo_target_dir/release/$name}"

if [[ -n "${VERSION:-}" ]]; then
  version="${VERSION#v}"
else
  version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
fi

if [[ -z "$version" ]]; then
  echo "could not determine package version (set VERSION= or fix Cargo.toml)" >&2
  exit 1
fi

if [[ ! -f "$bin_src" ]]; then
  echo "missing release binary: $bin_src" >&2
  echo "build first: cargo build --release --locked" >&2
  exit 1
fi

dist="$root/dist"
staging="$dist/staging"
deb_root="$staging/deb"
tar_name="${name}-${version}-linux-${arch}"
tar_root="$staging/$tar_name"
rm -rf "$dist"
mkdir -p "$deb_root" "$tar_root"

install -Dm0755 "$bin_src" "$deb_root/usr/bin/$name"
if command -v strip >/dev/null 2>&1; then
  strip --strip-unneeded "$deb_root/usr/bin/$name" || true
fi

for id in "${ids[@]}"; do
  install -Dm0644 "res/${id}.desktop" \
    "$deb_root/usr/share/applications/${id}.desktop"
  install -Dm0644 "res/${id}.metainfo.xml" \
    "$deb_root/usr/share/metainfo/${id}.metainfo.xml"
  install -Dm0644 "res/icons/hicolor/scalable/apps/${id}.svg" \
    "$deb_root/usr/share/icons/hicolor/scalable/apps/${id}.svg"
  install -Dm0644 "res/icons/hicolor/scalable/apps/${id}.svg" \
    "$deb_root/usr/share/icons/hicolor/scalable/apps/${id}-symbolic.svg"
done

install -Dm0644 LICENSE "$deb_root/usr/share/doc/$name/copyright"

# Desktop files already use Exec=cosmic-ext-applet-grok-monitor (PATH /usr/bin).
# User-local justfile install rewrites Exec to ~/.local/bin; system packages
# leave the upstream files as-is.

depends="libc6, libgcc-s1, libxkbcommon0, libwayland-client0, libwayland-cursor0, libwayland-egl1, libegl1, libfontconfig1, libfreetype6, libexpat1"
if command -v dpkg-shlibdeps >/dev/null 2>&1; then
  shlib_work="$(mktemp -d)"
  mkdir -p "$shlib_work/debian"
  printf 'Source: %s\n\nPackage: %s\n' "$name" "$name" >"$shlib_work/debian/control"
  if shlibs_line="$(
    cd "$shlib_work"
    dpkg-shlibdeps -O --ignore-missing-info "$deb_root/usr/bin/$name" 2>/dev/null
  )"; then
    shlibs_line="${shlibs_line#shlibs:Depends=}"
    if [[ -n "$shlibs_line" ]]; then
      depends="$shlibs_line"
    fi
  fi
  rm -rf "$shlib_work"
fi

installed_size="$(du -sk "$deb_root" | cut -f1)"
mkdir -p "$deb_root/DEBIAN"

cat >"$deb_root/DEBIAN/control" <<EOF
Package: $name
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Installed-Size: $installed_size
Maintainer: Simple Systems <russoj88@proton.me>
Homepage: https://github.com/simple-systems-se/grok-mon
Depends: $depends
Description: COSMIC panel applets for Grok Build, Bot, and xAI API usage
 Unofficial COSMIC panel applets that show Grok Build credit usage,
 Grok Bot weekly usage, and remaining xAI API prepaid dollars.
EOF

cat >"$deb_root/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
fi
EOF
chmod 0755 "$deb_root/DEBIAN/postinst"

cat >"$deb_root/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
fi
EOF
chmod 0755 "$deb_root/DEBIAN/postrm"

install -Dm0755 "$deb_root/usr/bin/$name" "$tar_root/$name"
cp -a LICENSE README.md "$tar_root/"
cp -a res "$tar_root/"

deb_file="$dist/${name}_${version}_${arch}.deb"
tar_file="$dist/${tar_name}.tar.gz"

dpkg-deb --build --root-owner-group "$deb_root" "$deb_file"
tar -C "$staging" -czf "$tar_file" "$tar_name"

rm -rf "$staging"

(
  cd "$dist"
  sha256sum "$(basename "$deb_file")" "$(basename "$tar_file")" >SHA256SUMS
)

echo "Wrote:"
ls -lh "$deb_file" "$tar_file" "$dist/SHA256SUMS"

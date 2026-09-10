#!/bin/sh
set -eu

archive=${1:-}
if [ -z "$archive" ] || [ ! -f "$archive" ]; then
  printf '%s\n' "usage: $0 ARCHIVE" >&2
  exit 2
fi

version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | sed -n '1p')
case $(basename "$archive") in
  "lazy-git-review-$version-"*.tar.gz) ;;
  *)
    printf '%s\n' "archive name does not contain Cargo version $version: $archive" >&2
    exit 1
    ;;
esac

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT INT TERM
tar -C "$stage" -xzf "$archive"
root=$(find "$stage" -mindepth 1 -maxdepth 1 -type d | sed -n '1p')
if [ -z "$root" ] || [ "$(find "$stage" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')" != "1" ]; then
  printf '%s\n' "archive must contain exactly one root directory" >&2
  exit 1
fi

test "$(sed -n '1p' "$root/VERSION")" = "$version"
test "$(sed -n '1p' "$root/share/lazy-git-review/VERSION")" = "$version"
for path in \
  bin/lgr \
  bin/lgr-gh-dash \
  bin/lgr-fetch-youtrack \
  install.sh \
  CHANGELOG.md \
  LICENSE \
  NOTICE \
  THIRD_PARTY_LICENSES.md \
  THIRD_PARTY_LICENSES.html \
  share/lazy-git-review/README.md \
  share/lazy-git-review/docs/installation.md \
  share/lazy-git-review/docs/configuration.md \
  share/lazy-git-review/docs/neovim.md \
  share/lazy-git-review/docs/troubleshooting.md \
  share/lazy-git-review/nvim/lua/lazy-git-review/init.lua \
  share/lazy-git-review/THIRD_PARTY_LICENSES.html; do
  test -e "$root/$path"
done
grep -q "European Union Public Licence" "$root/LICENSE"
grep -q "tokio" "$root/THIRD_PARTY_LICENSES.html"
grep -q "Apache License" "$root/THIRD_PARTY_LICENSES.html"
"$root/bin/lgr" --version | grep -q "$version"

install_home="$stage/home"
mkdir -p "$install_home"
HOME="$install_home" \
XDG_BIN_HOME="$install_home/bin" \
XDG_DATA_HOME="$install_home/share" \
"$root/install.sh"
"$install_home/bin/lgr" --version | grep -q "$version"
test -f "$install_home/.lgr/settings.json"
test -f "$install_home/share/lazy-git-review/LICENSE"
test -f "$install_home/share/lazy-git-review/CHANGELOG.md"
test -f "$install_home/share/lazy-git-review/THIRD_PARTY_LICENSES.html"

printf '%s\n' "package check passed: $(basename "$archive")"

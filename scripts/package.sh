#!/bin/sh
set -eu

target=${1:-}
if [ -z "$target" ]; then
  target=$(rustc -vV | sed -n 's/^host: //p')
fi
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | sed -n '1p')
if [ -z "$version" ]; then
  printf '%s\n' "could not read package version from Cargo.toml" >&2
  exit 1
fi

if [ "${LGR_SKIP_BUILD:-0}" != "1" ]; then
  cargo build --release --locked --target "$target"
fi
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT INT TERM
root="$stage/lazy-git-review-$version-$target"
mkdir -p "$root/bin" "$root/share/lazy-git-review"
cp "target/$target/release/lgr" "$root/bin/lgr"
cp scripts/lgr-gh-dash scripts/lgr-fetch-youtrack "$root/bin/"
chmod 0755 "$root/bin/lgr-gh-dash" "$root/bin/lgr-fetch-youtrack"
cp scripts/install.sh "$root/install.sh"
chmod 0755 "$root/install.sh"
cp -R skills nvim docs README.md CHANGELOG.md LICENSE NOTICE THIRD_PARTY_LICENSES.md "$root/share/lazy-git-review/"
if [ -n "${LGR_THIRD_PARTY_REPORT:-}" ]; then
  cp "$LGR_THIRD_PARTY_REPORT" "$root/share/lazy-git-review/THIRD_PARTY_LICENSES.html"
elif command -v cargo-about >/dev/null 2>&1; then
  cargo about generate about.hbs --locked --fail \
    --config about.toml \
    --output-file "$root/share/lazy-git-review/THIRD_PARTY_LICENSES.html"
else
  printf '%s\n' "cargo-about 0.9.2 is required to package third-party licence texts" >&2
  exit 1
fi
printf '%s\n' "$version" >"$root/share/lazy-git-review/VERSION"
cp LICENSE NOTICE THIRD_PARTY_LICENSES.md CHANGELOG.md "$root/"
cp "$root/share/lazy-git-review/THIRD_PARTY_LICENSES.html" "$root/"
printf '%s\n' "$version" >"$root/VERSION"
archive="lazy-git-review-$version-$target.tar.gz"
tar -C "$stage" -czf "$archive" "lazy-git-review-$version-$target"
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$archive" >"$archive.sha256"
else
  shasum -a 256 "$archive" >"$archive.sha256"
fi
printf '%s\n' "$archive"

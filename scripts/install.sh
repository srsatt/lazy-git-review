#!/bin/sh
set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
install_dir=${LGR_INSTALL_DIR:-${XDG_BIN_HOME:-"$HOME/.local/bin"}}
share_dir=${LGR_SHARE_DIR:-${XDG_DATA_HOME:-"$HOME/.local/share"}/lazy-git-review}

if [ -x "$script_dir/bin/lgr" ] && [ -d "$script_dir/share/lazy-git-review" ]; then
  project_root="$script_dir"
  binary="$project_root/bin/lgr"
  helper_dir="$project_root/bin"
  asset_dir="$project_root/share/lazy-git-review"
else
  project_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
  target=${LGR_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}
  binary="$project_root/target/$target/release/lgr"
  helper_dir="$project_root/scripts"
  asset_dir="$project_root"
  if [ "${LGR_SKIP_BUILD:-0}" != "1" ]; then
    cargo build --release --locked --manifest-path "$project_root/Cargo.toml" --target "$target"
  fi
fi

if [ ! -x "$binary" ]; then
  printf '%s\n' "release binary is missing: $binary" >&2
  exit 1
fi

mkdir -p "$install_dir" "$share_dir"
install -m 0755 "$binary" "$install_dir/lgr"
install -m 0755 "$helper_dir/lgr-gh-dash" "$install_dir/lgr-gh-dash"
install -m 0755 "$helper_dir/lgr-fetch-youtrack" "$install_dir/lgr-fetch-youtrack"
for item in skills nvim docs README.md CHANGELOG.md LICENSE NOTICE THIRD_PARTY_LICENSES.md THIRD_PARTY_LICENSES.html VERSION; do
  if [ -e "$asset_dir/$item" ]; then
    cp -R "$asset_dir/$item" "$share_dir/"
  fi
done
if [ "${LGR_SKIP_CONFIG_INIT:-0}" != "1" ]; then
  "$install_dir/lgr" config init >/dev/null
fi
printf '%s\n' "Installed lgr, lgr-gh-dash, and lgr-fetch-youtrack to $install_dir"
printf '%s\n' "Installed documentation and Neovim assets to $share_dir"

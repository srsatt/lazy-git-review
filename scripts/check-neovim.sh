#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
nvim_bin=${NVIM_BIN:-nvim}
version=$($nvim_bin --version | sed -n '1p')
case "$version" in
  "NVIM v0.12."*) ;;
  *)
    printf '%s\n' "Neovim 0.12 is required for acceptance checks; found: $version" >&2
    exit 1
    ;;
esac

run_fixture() {
  "$nvim_bin" --headless -u NONE \
    --cmd "set runtimepath+=$root/nvim" \
    -l "$root/tests/fixtures/$1"
}

LGR_TEST_ROOT="$root" run_fixture nvim_launch_acceptance.lua
LGR_FAKE_TUI="$root/tests/fixtures/fake_lgr_tui.sh" run_fixture nvim_tui_popup_acceptance.lua

stage=$(CDPATH='' cd -- "$(mktemp -d)" && pwd -P)
trap 'rm -rf "$stage"' EXIT INT TERM
mkdir -p "$stage/one/.git" "$stage/two/.git"
: >"$stage/one/synthetic"
: >"$stage/two/synthetic"
LGR_ROOT_ONE="$stage/one" LGR_ROOT_TWO="$stage/two" run_fixture nvim_gh_dash_acceptance.lua

config="$stage/diffview.lua"
printf '%s\n' \
  'return {' \
  '  opts = function()' \
  '    return { keymaps = require("lazy-git-review").diffview_keymaps({ view = {}, file_panel = {} }) }' \
  '  end,' \
  '}' >"$config"
LGR_DIFFVIEW_CONFIG="$config" run_fixture nvim_diffview_keymaps_acceptance.lua

printf '%s\n' "Neovim acceptance checks passed"

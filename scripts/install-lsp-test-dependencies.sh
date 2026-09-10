#!/bin/sh
set -eu

prefix=${1:-.ci-lsp}
NPM_CONFIG_CACHE=${NPM_CONFIG_CACHE:-"$prefix/.npm-cache"}
export NPM_CONFIG_CACHE
npm install --no-save --prefix "$prefix" \
  typescript@5.9.3 \
  typescript-language-server@6.0.0 \
  vscode-langservers-extracted@4.10.0

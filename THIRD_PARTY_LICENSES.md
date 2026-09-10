# Third-party licences

Project-authored source is licensed under EUPL-1.2. See `LICENSE` and `NOTICE`.

Rust dependencies are resolved exactly by `Cargo.lock` and retain their own licences and copyright notices. Release archives include `THIRD_PARTY_LICENSES.html`, generated with cargo-about from the locked dependency graph and SPDX licence data. Source builds retain corresponding licence files in the Cargo registry. This notice supplements those upstream terms; it does not replace them.

## Embedded parser assets

LGR links the following Tree-sitter crates and embeds their published highlight queries:

- `tree-sitter`, `tree-sitter-highlight` — MIT license
- `tree-sitter-javascript`, `tree-sitter-typescript` — MIT license
- `tree-sitter-html`, `tree-sitter-css` — MIT license

The project does not modify the embedded grammar or query assets.

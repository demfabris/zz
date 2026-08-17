# Showcase assets

The `icons/` directory is the canonical `gpui-component-assets` icon set from
`longbridge/gpui-component` revision `b004e595cf5de98a73b6b561394a559a94ae1e2a`,
vendored when `zz-ui` forked that crate. It is embedded in the showcase WASM so
the catalog does not depend on a CDN.

The upstream assets crate is distributed under Apache-2.0; see
`LICENSE-APACHE` in this directory. The icon artwork itself is now
[Iconoir](https://iconoir.com) regular, MIT, matching what `zz-ui`
embeds; see `icons/LICENSE-ICONOIR`.

The `fonts/inter/` directory contains the roman and italic variable TTFs from
the official Inter v4.1 release. They are embedded only in the showcase and are
distributed under the SIL Open Font License 1.1; see
`fonts/inter/LICENSE.txt`.

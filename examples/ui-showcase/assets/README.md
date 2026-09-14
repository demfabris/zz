# Showcase assets

The showcase renders icons from `zz-ui`'s own embedded set: it registers
`zz_ui::Assets`, which embeds `crates/zz-ui/assets/icons`. There is no separate
icon copy here, so the catalog cannot drift from the widgets it documents. The
artwork and its license live beside that set.

`LICENSE-APACHE` is upstream's, retained from the `gpui-component` fork at
revision `b004e595cf5de98a73b6b561394a559a94ae1e2a`.

The `fonts/inter/` directory contains the roman and italic variable TTFs from
the official Inter v4.1 release. They are embedded only in the showcase and are
distributed under the SIL Open Font License 1.1; see `fonts/inter/LICENSE.txt`.

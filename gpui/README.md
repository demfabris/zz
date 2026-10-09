# gpui

The GPU UI framework from [Zed](https://github.com/zed-industries/zed), as maintained for
[zz](https://github.com/demfabris/zz). It started as a patch branch on a Zed fork and drifted far
enough that rebasing stopped paying off, so it now lives here as its own repository.

It holds only the 22 crates zz builds against, plus the two font families `gpui` and `gpui_wgpu`
embed at build time. Crate names are unchanged, so `use gpui::...` keeps working.

## History

- The first commit is upstream `zed-industries/zed` at `933d8d9381`, limited to these crates.
- The next 89 commits are the zz patch set, replayed from `demfabris/zed` branch `zz-patches`
  (tip `5a00ac89a4`). Every crate tree here matches that tip.
- After that, this repository is the source of truth. Upstream fixes come in by hand.

## Using it

```toml
[dependencies]
gpui = { git = "https://github.com/demfabris/gpui", rev = "<sha>" }
gpui_platform = { git = "https://github.com/demfabris/gpui", rev = "<sha>" }
```

## Pulling a fix from upstream

```bash
git -C ~/src/zed format-patch -1 <sha> --stdout -- crates/gpui crates/gpui_wgpu | git am -3
```

Limit the pathspec to the crates the fix touches. Paths match upstream, so patches apply as-is.

## License

Apache-2.0, same as upstream. See `LICENSE-APACHE`. The bundled fonts keep their own licenses
under `assets/fonts`.

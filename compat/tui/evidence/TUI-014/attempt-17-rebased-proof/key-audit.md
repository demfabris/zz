Customize exit-key audit, 2026-09-17, pin d77c9dc6.

`mode-tree.c:1632` closes only for q, Escape, C-[ and C-g. C-c is
absent. The common mode tree handles Up/k/C-p, Down/j/C-n, shifted
Up/Down and K/J, page keys/C-b/C-f, g/Home/G/End, t/T/C-t, O/r,
Left/h/-, Right/l/+, M--/M-+, ?/slash/C-s, n/N, f/c and v.
F1/C-h opens help. `window-customize.c:1552` adds a, Enter/s, w,
S/W, d/D, u/U and H; it does not add another mode-exit key.
The empty-tree path also exits on both sides.

The fix removes C-c/raw ETX from the mode-exit branch and removes
unbound M-< and M-> navigation. It adds the missing ? search alias.
C-c still cancels an active prompt, matching the separate prompt path.
Unit tests cover exact exit membership and unbound C-c/raw ETX,
C-d, C-z, C-j, Space, M-<, M-> and x. Attached tests reopen the mode
for every key, compare the complete screen and pane mode, and cover
C-c, C-d, C-j, Space, M-<, M->, x, q, Escape and C-g.

This audit establishes mode exit membership. It does not turn the
previously documented unimplemented help, key-binding editing,
reset/unset, bulk-tag and mouse interactions into verified behavior.

# rootshell behavioral reference

[rootshell](https://github.com/demfabris/rootshell) is a SwiftUI terminal for
iPad and Catalyst. zz read it while designing the native iPhone and iPad
client; it is never compiled, linked, or shipped with zz, and no Swift source
is copied into `clients/ios`.

Read at commit
[`a5c318c5778efef7bf3f82a2ff7f5da82acf69bf`](https://github.com/demfabris/rootshell/tree/a5c318c5778efef7bf3f82a2ff7f5da82acf69bf).
The checkout is not tracked here. Clone it into the gitignored
`third_party/.cache/rootshell/` directory to read along:

```bash
git clone https://github.com/demfabris/rootshell third_party/.cache/rootshell
git -C third_party/.cache/rootshell checkout a5c318c5778efef7bf3f82a2ff7f5da82acf69bf
```

What it settles, and where:

- `Core/SettingsSync/Registry/Settings+*.swift` and
  `Core/SettingsSync/Store/SettingsStore.swift` for how a terminal app groups
  a large preference surface into pages a phone can present, and which
  settings are worth giving a device its own copy of;
- `Resources/fonts/` for the bundled monospace faces a terminal ships when it
  cannot rely on a system text stack. zz bundles 0xProto, Fira Code, and Geist
  Mono for the same reason, taken from their own upstreams under the SIL Open
  Font License rather than from here;
- `UI/Terminal/SplitPaneView.swift`, `TerminalSplitTreeView.swift`, and
  `Core/Terminal/SplitTree.swift` for pane controls and split geometry. Its
  split view commits terminal-cell resize requests to tmux; zz keeps the same
  division of responsibility, through its own daemon commands and the
  `zz-client` rectangle solver, rather than resizing panes client-side.

The design that came out of this reading is `knowledge/designs/ios-client.md`.
The upstream MIT license is retained beside this record for provenance.

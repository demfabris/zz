# tmux plugin compatibility corpus

The alias smoke suite checks zz against real tmux plugin initialization at the
following immutable revisions. These repositories are reference material only:
they are never compiled, linked, or shipped with zz. `compat/fetch-corpus.sh`
clones them into the gitignored `compat/.cache/plugins/` directory.

| Plugin | Upstream | Commit | License |
| --- | --- | --- | --- |
| tpm | [tmux-plugins/tpm](https://github.com/tmux-plugins/tpm) | [`e261deb1b47614eed3400089ce7197dc68acc4eb`](https://github.com/tmux-plugins/tpm/tree/e261deb1b47614eed3400089ce7197dc68acc4eb) | [MIT](tpm/LICENSE.md) |
| tmux-sensible | [tmux-plugins/tmux-sensible](https://github.com/tmux-plugins/tmux-sensible) | [`25cb91f42d020f675bb0a2ce3fbd3a5d96119efa`](https://github.com/tmux-plugins/tmux-sensible/tree/25cb91f42d020f675bb0a2ce3fbd3a5d96119efa) | [MIT](tmux-sensible/LICENSE.md) |
| vim-tmux-navigator | [christoomey/vim-tmux-navigator](https://github.com/christoomey/vim-tmux-navigator) | [`e41c431a0c7b7388ae7ba341f01a0d217eb3a432`](https://github.com/christoomey/vim-tmux-navigator/tree/e41c431a0c7b7388ae7ba341f01a0d217eb3a432) | [MIT](vim-tmux-navigator/LICENSE.md) |
| tmux-yank | [tmux-plugins/tmux-yank](https://github.com/tmux-plugins/tmux-yank) | [`acfd36e4fcba99f8310a7dfb432111c242fe7392`](https://github.com/tmux-plugins/tmux-yank/tree/acfd36e4fcba99f8310a7dfb432111c242fe7392) | [MIT](tmux-yank/LICENSE.md) |
| tmux-resurrect | [tmux-plugins/tmux-resurrect](https://github.com/tmux-plugins/tmux-resurrect) | [`cff343cf9e81983d3da0c8562b01616f12e8d548`](https://github.com/tmux-plugins/tmux-resurrect/tree/cff343cf9e81983d3da0c8562b01616f12e8d548) | [MIT](tmux-resurrect/LICENSE.md) |
| tmux-continuum | [tmux-plugins/tmux-continuum](https://github.com/tmux-plugins/tmux-continuum) | [`0698e8f4b17d6454c71bf5212895ec055c578da0`](https://github.com/tmux-plugins/tmux-continuum/tree/0698e8f4b17d6454c71bf5212895ec055c578da0) | [MIT](tmux-continuum/LICENSE.md) |
| tmux-fpp | [tmux-plugins/tmux-fpp](https://github.com/tmux-plugins/tmux-fpp) | [`878302f228ee14f0fa59717f63743d396b327a21`](https://github.com/tmux-plugins/tmux-fpp/tree/878302f228ee14f0fa59717f63743d396b327a21) | [MIT](tmux-fpp/LICENSE.md) |

The upstream license for each pinned checkout is retained verbatim beside this
record for provenance.

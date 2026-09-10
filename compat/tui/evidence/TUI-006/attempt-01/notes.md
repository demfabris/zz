# TUI-006 attempt-01

Cycle 5, the choosers lane. Every file here is a real run on alienware (see
`environment.txt`).

## Files

- `environment.txt`: both binaries' sha256, the zz revision and its clean
  state, the pin, the OS line, TERM, shell, bash, outer sizes and locale.
- `run-01-origin-main-digest.txt`: `compat/tui-choosers.sh` against zz built at
  3319ceba (unchanged origin/main) and the pin, before any product change. Exit
  1: 39 asserted comparisons, 28 differ, 8 recorded (7 for a sibling lane).
  The raw stdout was 235589 bytes because every differing row prints styled, so
  this file is a digest of it made with:

  ```
  awk '/^(ok|DIFF|note|all|[0-9]+ of|chooser)/ {print; next}
       /^      case / {print; pairs=0; next}
       /^      [0-9]+ of [0-9]+ rows differ/ {print; next}
       /^        +[0-9]+ (tmux|zz):/ { if (pairs < 4) {print; pairs++}; next }
       /^      cursor/ {print}' run-01.txt
  ```

  Every verdict line is kept; each differing case keeps its header, its row
  count and its first two differing row pairs.
- `self-check-01-origin-main.txt`: `compat/tui-choosers.sh --self-check` at the
  same revision. The four sabotages (a tag mark, a preview cell, a tree row, a
  cursor column, each planted on one side) are caught in their channels and the
  restoration equivalence passes. The chooser equivalence FAILS here, as it
  must before the fix: zz does not draw the pin's tree yet.

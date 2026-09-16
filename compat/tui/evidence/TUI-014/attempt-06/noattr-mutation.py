import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[5]
os.chdir(root)
source = Path("crates/zz-tui/src/render/pane_mode.rs")
original = source.read_text()
needle = "let selection = base_cell(&resolved_style(selection_style, theme).unwrap_or_else(plain));"
assert original.count(needle) == 1
try:
    source.write_text(original.replace(needle, "let selection = resolved_style(selection_style, theme).unwrap_or_else(plain);"))
    command = ["/tmp/zz-cargo.sh", "test", "-p", "zz-tui", "--lib", "a_switch_row_keeps_its_dim_runs_over_the_selection_style"]
    print("COMMAND", " ".join(command), flush=True)
    result = subprocess.run(command)
    print("MUTATION_EXIT", result.returncode, flush=True)
    if result.returncode != 101:
        raise SystemExit("mutation did not produce the expected test failure")
finally:
    source.write_text(original)
print("SOURCE_RESTORED", flush=True)

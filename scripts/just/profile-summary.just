import "settings.just"

set working-directory := "../.."

# Export and summarize CPU used by the isolated zz GUI, daemon, and CEF helpers.
cpu run:
    @python3 scripts/summarize-macos-cpu.py "{{ run }}"

# Export and summarize zz-owned work from one captured Metal run.
metal run:
    @python3 scripts/summarize-macos-metal.py "{{ run }}"

# Summarize terminal row-cache hits, misses, and prepaint timing from one diagnostic run.
terminal run:
    @python3 scripts/summarize-terminal-render.py "{{ run }}"

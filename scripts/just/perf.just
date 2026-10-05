import "settings.just"

set working-directory := "../.."

# Build zz_cli, then gate the daemon against a release tmux (bench/perf/README.md); extra args go to run.py.
gate stage="baseline" *args:
    @cargo build --release -p zz-cli --quiet
    @python3 bench/perf/run.py --zz target/release/zz_cli --stage {{ stage }} {{ args }}

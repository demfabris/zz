import "settings.just"

set working-directory := "../.."

run *args:
    @compat/run.sh {{ args }}

check:
    @compat/check.sh

import "settings.just"

set working-directory := "../.."

# Build the storybook, serve it on 127.0.0.1:8097 and rebuild on save.
run:
    @scripts/storybook-dev.sh

# Build the storybook into clients/storybook/dist; pass --release for an optimized build.
build *args:
    @scripts/build-storybook.sh {{ args }}

# Serve the last build without rebuilding.
serve:
    @scripts/storybook-serve.py clients/storybook/dist 8097

# Screenshot one story or section of a running storybook: just storybook shot agent/composer?theme=dark out.png
shot route out *args:
    @scripts/storybook-shot.mjs "{{ route }}" "{{ out }}" {{ args }}

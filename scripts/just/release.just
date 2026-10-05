import "settings.just"

set working-directory := "../.."

mod mac "release-mac.just"

# Install the pinned dry-run-first release driver.
setup:
    cargo install cargo-release --version 1.1.5 --locked

# Preview a SemVer bump, release commit, tag, and push; --execute applies and publishes it (the pushed tag starts release CI).
bump target *flags:
    @scripts/release.sh "{{ target }}" {{ flags }}

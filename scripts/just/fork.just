import "settings.just"

set working-directory := "../.."

# Show carried-patch fork status: our commits, upstream drift, Cargo.lock sync.
status:
    @scripts/fork-sync.sh status

# Rebase a fork's patch branch onto upstream (default: upstream branch tip).
rebase name target="":
    @scripts/fork-sync.sh rebase {{ name }} {{ target }}

import "settings.just"

set working-directory := "../.."

# Import a trimmed Ghostty snapshot at an upstream rev and merge it over zz's changes.
ghostty rev:
    @scripts/vendor-ghostty.sh {{ rev }}

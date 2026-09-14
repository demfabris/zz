#!/usr/bin/env bash
set -euo pipefail

executable="${1:?expected the absolute path to the development executable}"
link="$HOME/.local/bin/zz-dev"
if [[ -e "$link" && ! -L "$link" ]]; then
    echo "error: $link already exists and is not a symlink" >&2
    exit 1
fi
mkdir -p "$(dirname "$link")"
ln -sfn "$executable" "$link"
if [[ "$(uname -s)" == Linux ]]; then
    root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
    data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
    mkdir -p "$data_dir/icons/hicolor/scalable/apps" "$data_dir/applications"
    cp "$root/assets/linux/hicolor/scalable/apps/zz-dev.svg" "$data_dir/icons/hicolor/scalable/apps/zz-dev.svg"
    desktop_executable="${executable//\\/\\\\}"
    desktop_executable="${desktop_executable//\"/\\\"}"
    desktop_executable="${desktop_executable//\$/\\\$}"
    desktop_executable="${desktop_executable//\`/\\\`}"
    desktop_executable="${desktop_executable//%/%%}"
    desktop_executable="${desktop_executable//\\/\\\\}"
    cat > "$data_dir/applications/zz-dev.desktop" <<EOF
[Desktop Entry]
Name=zz Dev
Comment=Development terminal and browser workspace
Exec="$desktop_executable" app
Icon=zz-dev
StartupWMClass=zz-dev
Type=Application
Terminal=false
Categories=System;TerminalEmulator;
StartupNotify=true
EOF
fi

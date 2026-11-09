#!/usr/bin/env bash
# Installs Everwall for the current user on Linux (GNOME): the program in
# ~/.local/bin, its icon and its entry in the app grid. If it was running, it
# is closed, replaced and put back in the tray.
#   ./install.sh               build with cargo (from the source) and install
#   ./install.sh --no-build    install the last build
#   ./install.sh --uninstall   put back the wallpaper and accent colour from
#                              before Everwall, and remove it (the settings in
#                              ~/.local/share/Everwall stay)
# Next to a release's everwall binary (no src-tauri folder) it installs that
# binary and never builds.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin="$HOME/.local/bin/everwall"
icon="$HOME/.local/share/icons/hicolor/256x256/apps/everwall.png"
entry="$HOME/.local/share/applications/everwall.desktop"

running=false
if pgrep -x everwall > /dev/null; then
    running=true
    pkill -x everwall
    sleep 0.8
fi

if [[ "${1:-}" == "--uninstall" ]]; then
    [[ -x "$bin" ]] && "$bin" --uninstall
    rm -f "$bin" "$icon" "$entry"
    echo "Everwall removed. Its settings stay in ~/.local/share/Everwall."
    exit 0
fi

if [[ -d "$here/src-tauri" ]]; then
    if [[ "${1:-}" != "--no-build" ]]; then
        (cd "$here/src-tauri" && cargo build --release)
    fi
    # cargo puts the program in CARGO_TARGET_DIR when it is set
    source="${CARGO_TARGET_DIR:-$here/src-tauri/target}/release/everwall"
    source_icon="$here/src-tauri/icons/icon.png"
else
    source="$here/everwall"
    source_icon="$here/everwall.png"
fi

install -Dm755 "$source" "$bin"
install -Dm644 "$source_icon" "$icon"
mkdir -p "$(dirname "$entry")"
cat > "$entry" <<EOF
[Desktop Entry]
Type=Application
Name=Everwall
Comment=A different wallpaper on each screen
Exec="$bin"
Icon=everwall
Terminal=false
Categories=Utility;GTK;
EOF

# the app writes its own autostart entry and shortcuts when it starts
if $running; then
    nohup "$bin" --tray > /dev/null 2>&1 &
else
    nohup "$bin" > /dev/null 2>&1 &
fi
echo "Installed in $bin"

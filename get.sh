#!/usr/bin/env bash
# Installs Everwall on Ubuntu (GNOME, x86-64) from the latest release, for the
# current user:
#
#   curl -fsSL https://raw.githubusercontent.com/johan-socarras/Everwall/main/get.sh | bash
#
# It downloads Everwall-<version>-linux-x64.tar.gz from GitHub, checks it
# against the SHA-256 GitHub publishes for it, installs with apt the
# libraries Everwall needs if any is missing (asking for your password
# through sudo), and runs the package's install.sh: the program goes to
# ~/.local/bin, with its icon and its entry in the app grid, and starts.
# Running it again updates Everwall in place. Optional:
#
#   EVERWALL_VERSION=v1.0.0   install that release instead of the latest
#   GITHUB_TOKEN=...          for a private fork, or to avoid rate limits
#
# (pass them to bash: curl -fsSL ... | EVERWALL_VERSION=v1.0.0 bash)
#
# To uninstall: curl -fsSL ... | bash -s -- --uninstall
# It puts back the wallpaper and accent colour you had before Everwall and
# removes the program; your settings stay in ~/.local/share/Everwall.

set -euo pipefail

main() {
    local repo="johan-socarras/Everwall"
    local needs=(libwebkit2gtk-4.1-0 libayatana-appindicator3-1 zenity)

    fail() {
        printf '\033[31m%s\033[0m\nEverwall was not installed.\n' "$1" >&2
        exit 1
    }

    [ "$(uname -s)" = "Linux" ] || fail "This installer is for Linux. On Windows, use get.ps1: see the README."
    [ "$(uname -m)" = "x86_64" ] || fail "Everwall for Linux is built for x86-64 only; this machine is $(uname -m)."
    # Everwall installs for one user, in that user's home folder.
    [ "$(id -u)" -ne 0 ] || fail "Run this as your own user, not as root: it installs Everwall in your home folder and asks for sudo only if a library is missing."

    local bin="$HOME/.local/bin/everwall"
    if [ "${1:-}" = "--uninstall" ]; then
        if pgrep -x everwall >/dev/null 2>&1; then
            pkill -x everwall
            sleep 0.8
        fi
        [ -x "$bin" ] && "$bin" --uninstall
        rm -f "$bin" "$HOME/.local/share/icons/hicolor/256x256/apps/everwall.png" \
            "$HOME/.local/share/applications/everwall.desktop"
        echo "Everwall removed. Its settings stay in ~/.local/share/Everwall."
        exit 0
    fi

    command -v curl >/dev/null 2>&1 || fail "This installer needs curl."
    command -v tar >/dev/null 2>&1 || fail "This installer needs tar."

    local auth=()
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        auth=(-H "Authorization: Bearer $GITHUB_TOKEN")
    fi
    local which="latest"
    if [ -n "${EVERWALL_VERSION:-}" ]; then
        which="tags/$EVERWALL_VERSION"
    fi

    local release
    release=$(curl -fsSL -H "Accept: application/vnd.github+json" -H "User-Agent: everwall-install" \
        "${auth[@]}" "https://api.github.com/repos/$repo/releases/$which") \
        || fail "Could not find the release '$which' of $repo on GitHub."

    # Each asset of the release lists its fields in the same order, so the
    # n-th size, digest and URLs all belong to the n-th asset.
    local sizes digests urls apis
    sizes=$(printf '%s' "$release" | grep -oE '"size": *[0-9]+' | grep -oE '[0-9]+$' || true)
    digests=$(printf '%s' "$release" | grep -oE '"digest": *(null|"[^"]*")' | sed -E 's/^"digest": *//; s/"//g' || true)
    urls=$(printf '%s' "$release" | grep -oE '"browser_download_url": *"[^"]*"' | sed -E 's/.*"([^"]*)"$/\1/' || true)
    apis=$(printf '%s' "$release" | grep -oE '"url": *"https://api\.github\.com/repos/[^"]*/releases/assets/[0-9]+"' | sed -E 's/.*"([^"]*)"$/\1/' || true)
    local tag
    tag=$(printf '%s' "$release" | grep -oE '"tag_name": *"[^"]*"' | head -n 1 | sed -E 's/.*"([^"]*)"$/\1/')

    local index
    index=$(printf '%s\n' "$urls" | grep -nE '/Everwall-[^/]+-linux-x64\.tar\.gz$' | head -n 1 | cut -d: -f1 || true)
    [ -n "$index" ] || fail "The release $tag has no Linux package."
    local url size digest api name
    url=$(printf '%s\n' "$urls" | sed -n "${index}p")
    size=$(printf '%s\n' "$sizes" | sed -n "${index}p")
    digest=$(printf '%s\n' "$digests" | sed -n "${index}p")
    api=$(printf '%s\n' "$apis" | sed -n "${index}p")
    name=${url##*/}

    local folder
    folder=$(mktemp -d)
    # Expanded now: the trap runs after main returns, when $folder is gone.
    trap "rm -rf '$folder'" EXIT
    local archive="$folder/$name"

    echo "Downloading $name ($(( size / 1048576 )) MB)..."
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        # A private repository only serves its files through the API.
        curl -fsSL -H "Accept: application/octet-stream" -H "User-Agent: everwall-install" \
            "${auth[@]}" -o "$archive" "$api" || fail "The download failed."
    else
        curl -fsSL -H "User-Agent: everwall-install" -o "$archive" "$url" || fail "The download failed."
    fi

    local got
    got=$(stat -c %s "$archive")
    [ "$got" = "$size" ] || fail "The download is incomplete: $got bytes of $size."
    if [[ "$digest" =~ ^sha256:([0-9a-f]{64})$ ]]; then
        [ "$(sha256sum "$archive" | cut -d' ' -f1)" = "${BASH_REMATCH[1]}" ] \
            || fail "The download does not match the SHA-256 GitHub publishes for it."
        echo "Checked against the SHA-256 published on GitHub."
    else
        echo "GitHub publishes no SHA-256 for this file; checked its size only."
    fi

    tar -xzf "$archive" -C "$folder" || fail "The package could not be unpacked."
    local unpacked
    unpacked=$(find "$folder" -mindepth 1 -maxdepth 1 -type d -name 'Everwall-*' | head -n 1)
    [ -n "$unpacked" ] && [ -x "$unpacked/everwall" ] && [ -f "$unpacked/install.sh" ] \
        || fail "The package does not hold the everwall program and its install.sh."

    # The libraries it needs, the same ones the README lists.
    if command -v dpkg-query >/dev/null 2>&1 && command -v apt-get >/dev/null 2>&1; then
        local missing=() pkg
        for pkg in "${needs[@]}"; do
            dpkg-query -W -f='${Status}' "$pkg" 2>/dev/null | grep -q 'ok installed' || missing+=("$pkg")
        done
        if [ ${#missing[@]} -gt 0 ]; then
            command -v sudo >/dev/null 2>&1 || fail "Everwall needs ${missing[*]}; install them, or install sudo, and run this again."
            echo "Installing what Everwall needs: ${missing[*]}..."
            sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y "${missing[@]}" \
                || fail "apt could not install ${missing[*]}."
        fi
    else
        echo "Not a Debian-based system: make sure WebKitGTK 4.1, AppIndicator and zenity are installed."
    fi

    # A library that is missing, or a system older than the one the package
    # was built on, shows up here instead of as a program that never opens.
    local broken
    broken=$(ldd "$unpacked/everwall" 2>&1 | grep 'not found' || true)
    if [ -n "$broken" ]; then
        printf '%s\n' "$broken" >&2
        fail "This system cannot run the release package of Everwall (it is built on Ubuntu 26.04). Build it from the source instead: see the README."
    fi

    echo "Installing Everwall $tag for $(id -un)..."
    bash "$unpacked/install.sh" || fail "The package's install.sh stopped."
    printf '\033[32mEverwall %s is installed in %s.\033[0m\n' "$tag" "$bin"
    echo "It is in the tray and in the app grid, and starts when you log in."
}

main "$@"

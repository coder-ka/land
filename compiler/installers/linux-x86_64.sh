#!/bin/sh
set -eu

VERSION=${LAND_VERSION:-0.0.1}
INSTALL_DIRECTORY=${LAND_INSTALL_DIRECTORY:-"$HOME/.local/bin"}
BASE_URL=${LAND_BASE_URL:-"https://github.com/coder-ka/land/releases/download"}

if [ "$(uname -s)" != "Linux" ]; then
    echo "This installer supports Linux only" >&2
    exit 1
fi
case "$(uname -m)" in
    x86_64|amd64) ;;
    *)
        echo "This installer currently supports x86_64 only" >&2
        exit 1
        ;;
esac

ASSET=landc-linux-x86_64
RELEASE=${BASE_URL%/}/v$VERSION
TEMPORARY=$(mktemp -d)
trap 'rm -rf "$TEMPORARY"' EXIT HUP INT TERM

curl --fail --location --retry 3 --output "$TEMPORARY/$ASSET" "$RELEASE/$ASSET"
curl --fail --location --retry 3 --output "$TEMPORARY/$ASSET.sha256" "$RELEASE/$ASSET.sha256"

EXPECTED=$(awk 'NR == 1 { print $1 }' "$TEMPORARY/$ASSET.sha256")
ACTUAL=$(sha256sum "$TEMPORARY/$ASSET" | awk '{ print $1 }')
if [ "$ACTUAL" != "$EXPECTED" ]; then
    echo "landc checksum mismatch" >&2
    exit 1
fi

mkdir -p "$INSTALL_DIRECTORY"
install -m 755 "$TEMPORARY/$ASSET" "$INSTALL_DIRECTORY/landc"

case ":$PATH:" in
    *":$INSTALL_DIRECTORY:"*) ;;
    *)
        if [ "$INSTALL_DIRECTORY" = "$HOME/.local/bin" ]; then
            LINE='export PATH="$HOME/.local/bin:$PATH"'
            touch "$HOME/.profile"
            grep -F "$LINE" "$HOME/.profile" >/dev/null 2>&1 || printf '\n# Land\n%s\n' "$LINE" >>"$HOME/.profile"
            echo "Added $INSTALL_DIRECTORY to PATH in $HOME/.profile"
        else
            echo "Add $INSTALL_DIRECTORY to PATH"
        fi
        ;;
esac

"$INSTALL_DIRECTORY/landc" --version
echo "Installed landc to $INSTALL_DIRECTORY"

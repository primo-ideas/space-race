#!/bin/sh
# Installs a new Space Race build on a machine that is already set up. It runs as root, from the
# directory the archive was unpacked into, and is called by both deploy/deploy.ps1 and the
# "Deploy" GitHub workflow.
#
# It replaces three things and nothing else: the server binary, its data directory and the web
# client, then restarts the service. It never touches the space-race user, the systemd unit, the
# Apache configuration or the TLS certificate. Those belong to the machine and are put there once
# by the maintainer's setup-server.sh, which is not in this repository because it sets up the
# whole host. A deployment that rewrote them could take the whole site down -- not
# just the game. So when one of them is missing this script stops and says so, rather than
# creating it.
#
# A deployment restarts the server, which drops everyone connected. Races are not persisted.
set -eu

# The one place the game's name appears. Everything else is derived from it, so renaming the game
# is this line, here and in setup-server.sh.
NAME=space-race

USER_NAME=$NAME
HOME_DIR=/home/$NAME
WEB_DIR=/var/www/html/$NAME
SERVICE=$NAME.service
BINARY=$NAME-server

say() {
    echo "==> $1"
}

die() {
    echo "error: $1" >&2
    echo "       this machine is not set up for ${NAME} yet: run setup-server.sh on it" >&2
    echo "       (see docs/deploy.md)" >&2
    exit 1
}

say "checking the machine"
id "$USER_NAME" >/dev/null 2>&1 || die "there is no $USER_NAME user"
[ -d "$HOME_DIR" ] || die "there is no $HOME_DIR"
[ -d "$WEB_DIR" ] || die "there is no $WEB_DIR"
systemctl cat "$SERVICE" >/dev/null 2>&1 || die "$SERVICE is not installed"

# The binary cannot be overwritten while it runs, but it can be replaced: a rename swaps the
# directory entry and leaves the running process on the old file until it is restarted.
say "installing the game server"
install -o "$USER_NAME" -g "$USER_NAME" -m 755 "$BINARY" "$HOME_DIR/$BINARY.new"
mv -f "$HOME_DIR/$BINARY.new" "$HOME_DIR/$BINARY"

# Built beside the old one and swapped in, so a deployment that fails halfway leaves the running
# server with the tracks it already had.
say "installing the tracks and the car tuning"
rm -rf "$HOME_DIR/data.new"
cp -r data "$HOME_DIR/data.new"
chown -R "$USER_NAME:$USER_NAME" "$HOME_DIR/data.new"
rm -rf "$HOME_DIR/data"
mv "$HOME_DIR/data.new" "$HOME_DIR/data"

# Before the web client, so a browser loading the page finds a server that speaks its protocol
# version rather than one that is about to be replaced.
say "restarting the service"
systemctl restart "$SERVICE"

# Swapped in rather than copied over: the module is tens of megabytes, and copying in place would
# serve half a game to anyone loading the page meanwhile. The directory itself is replaced, so
# whatever the last deployment left behind goes with it. Only this directory: the document root
# above it, and whatever landing page lives there, is not ours.
say "installing the web client"
rm -rf "$WEB_DIR.new" "$WEB_DIR.old"
cp -r web "$WEB_DIR.new"
chown -R root:root "$WEB_DIR.new"
chmod -R a+rX "$WEB_DIR.new"
mv "$WEB_DIR" "$WEB_DIR.old"
mv "$WEB_DIR.new" "$WEB_DIR"
rm -rf "$WEB_DIR.old"

say "done"
# The unit restarts on failure, so a binary that dies on startup looks alive for a moment. Give it
# a couple of seconds to fall over, or a broken build passes for a good deployment.
sleep 2
if ! systemctl is-active --quiet "$SERVICE"; then
    echo "error: $SERVICE did not come back up after the install" >&2
    journalctl -u "$SERVICE" --no-pager --lines=30 >&2
    exit 1
fi
echo "    the game server is running"
systemctl status "$SERVICE" --no-pager --lines=5 | sed 's/^/    /'

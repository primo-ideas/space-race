# Deployment

Star Racer runs on the maintainer's Scaleway machine, at <https://primo-ideas.xyz/star-racer/>. The
machine is set up once, by a script that lives outside this repository because it sets up the
whole host; after that a deployment only replaces the build.

There are two ways to deploy, and they do the same three things to the machine:

- **A push to `main`** runs the "Deploy" workflow, which is the usual way a commit goes online.
  It can also be started by hand from the Actions tab. See
  [Continuous integration](ci.md#deploying).
- **`./deploy/deploy.ps1`**, from a Windows machine with the repository. This is how a build goes
  online without a commit, and how one goes online when GitHub is having a bad day. `-SkipBuild`
  uploads what was built last time, `-UploadOnly` stops once the archive is on the machine, and
  `-Server`, `-Key` and `-WebPath` point it somewhere else.

Both build the game server for Linux and the client for the browser, upload them, and run
`deploy/install.sh` as root on the machine.

## What a deployment replaces, and what it never touches

`deploy/install.sh` replaces exactly three things:

```text
/home/star-racer/star-racer-server   the binary, renamed over the old one
/home/star-racer/data/               tracks and car tuning, reloaded while it runs
/var/www/html/star-racer/            the web client: index.html, the bindings, the module
```

then restarts the service and checks it is still up two seconds later, so a binary that dies on
startup fails the deployment instead of passing for one.

The ranked ladder, `/home/star-racer/ratings.ron`, sits beside them and is not part of a build: a
deployment restarts the server, which reads it back (see [Quick play and ranked](ranked.md)).

It **never** touches the `star-racer` user, the systemd unit, the Apache configuration, the TLS
certificate, or anything under the document root that is not its own directory. Those belong to
the machine, not to the repository: a deployment that rewrote the virtual host could take the
whole site down, and the game is the least important thing on it. So `install.sh` checks they are
there and **stops with a message instead of creating them**. Creating them is
the machine's own `setup-server.sh`'s job (see [below](#setting-the-machine-up-once)), and a
deliberate act.

The two replacements that are not a single rename are staged beside their target and swapped in:
a deployment that fails halfway leaves the running server with the tracks it already had, and
nobody loading the page gets half a module.

## Setting the machine up, once

Not from this repository. The machine serves more than this game, and a script that creates the
`star-racer` user and edits the shared virtual host is the machine's business rather than one
game's — so it lives with the maintainer, as `~/Documents/setup-server.sh`, and it knows about
every game on the host.

```sh
scp ~/Documents/setup-server.sh root@primo-ideas.xyz:/tmp/
ssh root@primo-ideas.xyz sh /tmp/setup-server.sh --ci-key "$(cat ~/.ssh/deploy.pub)"
```

It sets up every game on the host: for each one, the system user, the home, the web directory, the
systemd unit on its own loopback port, the Apache configuration, and one `Include` line inside the
`SSLEngine on` block of the virtual host. It enables the Apache modules the games need, installs
`apache2` on a machine that has none — the only package it ever installs — and ends by allowing
the deployment key.

**It never upgrades the system and never touches TLS.** Keeping the machine patched, and holding
a certificate, are the maintainer's business and not a setup script's. It reads the virtual host
certbot wrote, to add a line to it, and that is the whole of its dealings with either. Where
certbot has not run yet it does everything else and says what is missing, so either order works:
the certificate then this script, or this script, the certificate, and this script again.

It is idempotent, which is also how the machine is changed afterwards: run it again after adding
a game, moving a port, or getting the certificate. `--dry-run` says what it would change and
changes nothing; `--only star-racer` restricts it to this game. Anything it overwrites is backed up
beside itself with a timestamp first, and the virtual host is `configtest`ed before Apache is
reloaded, then put back as it was if the check fails — a broken configuration there takes down
every site on the machine, not just the game.

The one thing it will not do is choose between two TLS virtual hosts that could both serve the
domain. It stops and says so instead.

### The deployment key

Generate the pair on your own machine, give the public half to the setup script above, and the
private half to GitHub:

```sh
ssh-keygen -t ed25519 -N "" -C "primo-ideas deploy" -f ~/.ssh/deploy
```

In the repository: **Settings → Secrets and variables → Actions → New repository secret**, named
`DEPLOY_SSH_KEY`, holding the whole of `~/.ssh/deploy` — the private file, `BEGIN`/`END` lines
included. That is the only thing the workflow needs; it is in the repository already and appears
in the Actions tab by itself. The same key serves the other game's repository.

### What the setup script leaves for this game

```text
/etc/systemd/system/star-racer.service   its own user, loopback port 8081, RUST_LOG=info
/etc/apache2/star-racer.conf             the WebSocket proxy, the wasm media type, the directory
<the TLS virtual host>                   one Include line, inside the SSLEngine block
/home/star-racer/                        the server's home, owned by the star-racer user
/var/www/html/star-racer/                where the web client goes
```

The port is 8081 and not the default 8080, which belongs to the other game on the same machine.
Nothing there writes to `/var/www/html` itself: the document root and whatever landing page sits
in it belong to the site.

### The machine as it is since November 2026

The machine was reinstalled on 2026-11-05 (Ubuntu 26.04, one core, under a gigabyte of memory),
with Apache, a certbot certificate and another site, and nothing of the game. The setup script was
not at hand, so the game was set up on it by hand on 2026-11-06, with the same user, home, web
directory and unit as the script's, but wired into Apache the way the other site on the machine
is, which leaves the virtual host certbot wrote untouched:

```text
/etc/apache2/conf-available/star-racer.conf   enabled with a2enconf, so every virtual host has it
```

```apache
<Location "/star-racer/ws">
	ProxyPass "http://127.0.0.1:8081/ws" upgrade=websocket
</Location>
<Directory "/var/www/html/star-racer">
	Options -Indexes
	AllowOverride None
	Require all granted
	AddType application/wasm .wasm
</Directory>
# The game was Space Race until 2026-11-11: its old address leads to the new one.
RedirectMatch permanent "^/space-race(/.*)?$" "/star-racer/"
```

On 2026-11-11 the machine followed the renaming to Star Racer: the `star-racer` user, home, web
directory and unit were made by hand in the same layout, the `space-race` service stopped and
disabled, its conf swapped for the one above, and v0.1.26 installed with `install.sh`. The old
user, home, unit file and web directory were removed the same day, at the maintainer's word.

### star-racer.xyz

The domain `star-racer.xyz` points at the same machine and serves the game at its root, from the
same directory, so the page reaches its server at `/ws`
([it finds it from its own address](#the-web-client-finds-its-own-server)):

```text
/etc/apache2/sites-available/002-plain-star-racer.conf   enabled with a2ensite
```

```apache
<VirtualHost *:80>
	ServerName star-racer.xyz
	DocumentRoot /var/www/html/star-racer
	<Location "/ws">
		ProxyPass "http://127.0.0.1:8081/ws" upgrade=websocket
	</Location>
</VirtualHost>
```

That is plain HTTP only, and **not playable yet**: the page always connects over `wss://`, and
the machine has no certificate for that name. The certificate is the maintainer's to request
(`certbot --apache -d star-racer.xyz`, which writes the TLS virtual host from this one); nothing
else is needed. A deployment needs no change: both names serve the one directory `install.sh`
replaces.

`upgrade=websocket` lets `mod_proxy_http` carry the WebSocket by itself (Apache 2.4.47 and
later), so no other module is needed. Ubuntu's own configuration already compresses
`application/wasm`: the 52 MB module of the time went over the wire as 14.5 MB (it is 23.9 MB since v0.1.26, see
[Web build](web.md#size)). Checked through Apache both
ways, the browser over `wss://` and the native client over plain `ws://` through an SSH tunnel to
the machine's port 80.

## Why nothing is built on the machine

Until September 2026 the machine had one core, a gigabyte of memory and no Rust toolchain; the
one that replaced it is larger, but still has no toolchain and no reason to grow one. The server
is cross-compiled to `x86_64-unknown-linux-musl`, from Windows by `deploy.ps1` and from a runner
by the workflow. Every crate the server uses is pure Rust and that target brings its own libc and
CRT objects, so no cross C compiler is needed -- but it still reaches for `cc` as its linker,
which a Windows machine does not have (``linker `cc` not found``). So `.cargo/config.toml` points
it at `rust-lld` instead, which ships with the toolchain and which rustc finds by name in its own
sysroot. The result is a `static-pie` ELF binary that depends on no library on the machine.

The mistake is easy to repeat: a Linux runner has a `cc`, so CI would have linked happily and
never shown the problem. The setting is committed rather than left to each machine, so Windows
and the runner build the same way.

That target and the wasm one each have a cache of their own, which is why both scripts name a
single crate with `-p` where the rest of the project builds the whole workspace.

## The web client finds its own server

A page served from the site connects to `wss://<the same host>/<its own directory>/ws` unless the
address says otherwise. Served at `/star-racer/index.html`, it reaches `/star-racer/ws`, which the
proxy forwards to the game server: a deployed page needs no `?server=` at all. A page served from
`127.0.0.1` is the exception — there is no proxy in front of a development server — and falls back
to `ws://127.0.0.1:8080/ws`. `?server=` still overrides both.

## Day to day

```sh
systemctl status star-racer        # is it running
journalctl -u star-racer -f        # what it is doing
systemctl restart star-racer       # after editing a track or the car by hand
```

Tracks and `car.ron` in `/home/star-racer/data` are reloaded within a second of being saved, so the
car can be tuned on the live server without restarting it. The next deployment overwrites them.

A deployment restarts the server, which drops everyone connected and ends any race in progress.
Nothing is persisted, so there is nothing to lose beyond the race itself.

## What this deliberately does not do

The machine is a personal one, running an unknown project: the setup is kept small rather than
hardened. The CI deploys as `root` over SSH with a key that has no command restriction, and the
host key is scanned rather than pinned. There is no staging environment, no rollback beyond
deploying the previous commit again, no firewall rule for the game (it is only reachable through
the loopback), no rate limiting, and no separate log file — journald keeps the logs.

The systemd unit does take the cheap protections: its own unprivileged user, `NoNewPrivileges`,
`PrivateTmp`, a read-only view of the filesystem (`ProtectSystem=strict`) with its own home as
the single writable path. They cost nothing and are already there.

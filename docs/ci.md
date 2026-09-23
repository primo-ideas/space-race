# Continuous integration

Five GitHub workflows in `.github/workflows/`, each with a badge of its own in the README. Four
check a commit; the fifth puts it online.

| Workflow | What it proves |
| --- | --- |
| `build-server.yml` | The game server compiles. It pulls no Bevy, so it is the quick one. |
| `test-server.yml` | The server, protocol, simulation and logger tests pass: encoding, authentication, lobbies, an autopilot lapping every shipped track, and a real server driven over WebSocket (see [Architecture](architecture.md#testing)). |
| `build-client-native.yml` | The desktop client compiles on Linux, tests included. It is the one workflow that installs system packages: Bevy links against ALSA, udev, Wayland, xkbcommon and X11, and a runner image carries none of their `-dev` halves. |
| `build-client-wasm.yml` | The client compiles for the browser. |
| `deploy.yml` | The commit is playable at <https://primo-ideas.xyz/space-race/> (see [Deploying](#deploying)). |

The four checks run on every push to `main`, on every pull request, and by hand. They build the
**dev profile** with `CARGO_PROFILE_DEV_DEBUG=0`: a check only needs to know the code compiles,
and debug info is the expensive half of a build. Local work builds the same dev profile, so a
release-only compilation error would slip past both — which is one reason the deployment builds
release and runs before anything is replaced.

Each workflow names a single crate and caches under a key of its own. That is the opposite of the
local rule, which always builds `--workspace`: naming a crate changes which features Cargo
unifies, so two workflows sharing one target directory would recompile Bevy against each other
(see [Architecture](architecture.md#feature-unification)). Separate keys cost more cache than they
save time, but the cache is free and the recompilation is not.

Restoring and saving are two steps rather than one, and the save runs whatever happened to the
build. `actions/cache` writes its cache only when the job succeeded, and a red build is exactly
when the next run should not start from nothing. The save is skipped on an exact key hit, which
already holds that `Cargo.lock`.

Nothing filters on paths. A README typo therefore runs the whole thing, which is harmless and
keeps the workflows simple — except that it also deploys, which is the one place that matters and
is why the deployment is gated and serialized rather than merely automatic.

## Deploying

`deploy.yml` runs on every push to `main` and on demand, in two jobs.

The first runs the same tests as `test-server.yml`, restoring that workflow's cache without ever
writing to it. Nothing goes online on a red test. The second cross-compiles the server to
`x86_64-unknown-linux-musl`, builds the client for the browser in release, runs `wasm-bindgen` at
the version the lock file names and `wasm-opt` over the result, then uploads the lot and runs
`deploy/install.sh` on the machine over SSH. It ends by fetching the page back, checking the
module is served as `application/wasm` and that `/space-race/ws` is not a 404, so a deployment that
cannot be played does not pass for a good one.

Between them, those two jobs are a stronger check than the four: they are the only place the
**release** profile is built, and the release module is the one players download.

It caches the Cargo registry and deliberately **not** `target`. A release build of Bevy for wasm
leaves a target directory of several gigabytes, and the whole repository shares one 10 GB cache:
saving it here would evict the four check caches, which run on every pull request, in order to
speed up the job that runs least often.

Deployments are serialized (`concurrency: deploy`) and never cancelled halfway. `install.sh`
swaps a binary, a data directory and a web directory one after another, and a run killed between
two of them would leave the machine serving a client and a server that disagree.

The workflow references a `production` environment it does not configure. GitHub creates it on
the first run, which is where a required reviewer can be added later to put a human in front of
every deployment without touching the workflow file.

## What it takes, on GitHub and on the machine

Two things, once, and [Deployment](deploy.md#setting-the-machine-up-once) has the commands:

- The machine has been set up by the maintainer's `setup-server.sh`, which lives outside this
  repository because it sets up the whole host. It creates the user, the service and
  the Apache wiring that `install.sh` refuses to invent.
- A repository secret **`DEPLOY_SSH_KEY`** holds a private key whose public half is in that
  machine's `/root/.ssh/authorized_keys`.

The host key is scanned at each run rather than pinned. Pinning is stronger, but a rebuilt machine
would then break deployments with a confusing error, and what pinning defends against — somebody
answering for this host on a GitHub runner's first connection to it — is outside what this project
defends against (see [Deployment](deploy.md#what-this-deliberately-does-not-do)).

Forks do not deploy: the job is skipped unless the repository is `primo-ideas/space-race`, and a
fork has no key in any case.

## Why this could not exist before

There was a deployment workflow once, in the same place, deleted because it could not work. The
reason had nothing to do with how it was written, and is worth keeping here so the shape of the
problem is recognized if it comes back.

The old machine had **no IPv4 address**, deliberately: an IPv4 address is billed and the
maintainer did not want to pay for one. That left two directions, and both were closed:

- **GitHub to the server.** GitHub-hosted runners have no IPv6 egress. The job failed with
  `Network is unreachable`, before ever reaching the SSH handshake.
- **The server to GitHub.** `github.com`, `api.github.com`, `codeload.github.com` and
  `objects.githubusercontent.com` have **no AAAA record at all**. From the machine,
  `getent ahostsv6 github.com` answered `::ffff:140.82.121.4`, an IPv4-mapped address and not a
  real one. So the server could not fetch a release either, and the obvious repair — have the
  machine pull instead of being pushed to — failed for the mirror-image reason.

The machine was not isolated: over IPv6 it reached Cloudflare, Google, the Ubuntu and Debian
mirrors, Let's Encrypt, GitLab and Scaleway's object storage without trouble. It was GitHub
specifically that was unreachable, and no NAT64 answered at the well-known `64:ff9b::/96` prefix.

A larger Scaleway machine replaced it in September 2026, with an A record as well as an AAAA one.
That closes the first direction — a runner reaches it over IPv4 — and the second never needed
closing. Nothing else about the argument changed, so if the IPv4 address is ever given up, this
workflow goes with it and `deploy/deploy.ps1` becomes the only way again.

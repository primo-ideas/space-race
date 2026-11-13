# Releases and downloads

Since v0.1.41 (2026-11-13, the user's request "Tu maintiendras les releases github et tu proposeras
en téléchargement ce qu'il faut sur le site de star racer"), every version that goes online also
gets a GitHub release with the desktop client to download, and the site offers it.

## When a release is made

**Every deployed version is released.** Deploying is the moment a version becomes the one people
play, so it is the one to download too: after `deploy.ps1` succeeds and the live site checks out,
Claude pushes the tag `vX.Y.Z` of the deployed commit (`git tag vX.Y.Z <commit>` then
`git push origin vX.Y.Z`). Versions committed but never deployed get no release, nor do the
versions that only record a deployment. A tag can be released again by hand from the Actions tab
(`Release`, "Run workflow", with the tag), which replaces its files.

## What the Release workflow does

`.github/workflows/release.yml` runs on every `v*` tag:

1. **Builds the client** for three systems at once, in release, on GitHub's runners: Windows x86-64,
   Linux x86-64 and macOS on Apple silicon. Nothing is built on this machine, which has no
   toolchain for the other two. The client is one file: its fonts are compiled in, and natively it
   plays on `star-racer.xyz` unless told otherwise (see [Protocol](protocol.md#the-servers-address)).
   A release build on Windows opens no console window beside the game (`windows_subsystem`); a debug
   one keeps it, for the logs.
2. **Packages** each in a folder `star-racer/` with the binary, `LICENSE` (the Unlicense) and
   `FONT-LICENSE.txt` (Saira's OFL), as `star-racer-windows-x86_64.zip`,
   `star-racer-linux-x86_64.tar.gz` and `star-racer-macos-arm64.tar.gz`.
3. **Publishes** the release "Star Racer vX.Y.Z", marked latest, its notes the version's commit
   message, which says what changed and why.

The names never change from one release to the next, so
`https://github.com/primo-ideas/star-racer/releases/latest/download/<name>` always hands out the
newest build, and the site links to that.

The builds are **not signed**: Windows SmartScreen asks before running one the first time, and
macOS Gatekeeper wants it opened from a right click. Signing needs a certificate from Microsoft's
and Apple's programs, which an alpha does not have. The Linux build needs ALSA and udev at run
time, as every desktop has.

## On the site

`client/web/download.html`, deployed beside the game, so it is at
<https://star-racer.xyz/download.html> and <https://primo-ideas.xyz/star-racer/download.html>:
PLAY IN THE BROWSER first, then the three downloads, the visitor's own system lined in neon, what
running an unsigned build asks, and links to every release and the source. It asks nothing of any
other server, as the game does not. The web game's first screen has a DOWNLOAD button, in the place
the native game has QUIT, which opens it in a tab of its own; a phone, which has no use for it,
does not show it.

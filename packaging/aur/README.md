# AUR packaging for hyprburst

This directory holds the [AUR](https://aur.archlinux.org/) packaging for
Hyprburst:

- [`PKGBUILD`](PKGBUILD) — builds the `hyprburst` package from the published
  crates.io source tarball.
- [`.SRCINFO`](.SRCINFO) — generated metadata the AUR requires alongside the
  `PKGBUILD`. **Never edit it by hand** — regenerate it from the `PKGBUILD`.

The package name is **`hyprburst`** (the bare `burst` name is taken on
crates.io, so the project settled on `hyprburst` everywhere). It is a
from-source package: it downloads the crate tarball and runs `cargo build
--release`. There is no `-bin` package yet because the project does not ship
prebuilt binaries.

## What the package installs

| Path | Source |
| --- | --- |
| `/usr/bin/hyprburst` | the release binary |
| `/usr/share/hyprburst/hyprburst.conf` | drop-in Hyprland overlay config |
| `/usr/share/hyprburst/config.example.toml` | annotated example config |
| `/usr/share/doc/hyprburst/README.md` | project README |
| `/usr/share/licenses/hyprburst/LICENSE` | GPL-3.0-or-later license |

After installing, source the Hyprland config and bind a key (see the project
README's *Hyprland Setup*):

```sh
install -Dm644 /usr/share/hyprburst/hyprburst.conf ~/.config/hypr/hyprburst.conf
echo 'source = ~/.config/hypr/hyprburst.conf' >> ~/.config/hypr/hyprland.conf
```

## Validate locally before publishing

The crate tarball referenced by `source=()` only exists once the version is
published to crates.io. Validate against a real published version, or stage a
locally-built tarball first:

```sh
# Option A — the version is already on crates.io:
cd packaging/aur
makepkg -si              # download, build, run check(), install

# Option B — validate before publishing the crate, using a local tarball:
cargo package                                   # -> target/package/hyprburst-X.Y.Z.crate
mkdir -p /tmp/hb-aur && cp target/package/hyprburst-*.crate /tmp/hb-aur/
cd packaging/aur
SRCDEST=/tmp/hb-aur BUILDDIR=/tmp/hb-aur PKGDEST=/tmp/hb-aur \
  makepkg -f --skipinteg          # finds the staged tarball, skips the download
```

`makepkg` runs `check()` (the behavioural integration tests) as part of the
build. The repo-hygiene golden tests are intentionally **not** run from the
tarball — they read files (`RELEASING.md`, `.github/workflows`) that ship in git
but not in the crate.

## Publish the package for the first time

You must be an AUR account holder with the package name registered to you.

1. Clone the (empty) AUR repo:
   ```sh
   git clone ssh://aur@aur.archlinux.org/hyprburst.git aur-hyprburst
   ```
2. Copy `PKGBUILD` and `.SRCINFO` into it.
3. Regenerate `.SRCINFO` to be safe, commit, and push:
   ```sh
   makepkg --printsrcinfo > .SRCINFO
   git add PKGBUILD .SRCINFO
   git commit -m "Initial import: hyprburst X.Y.Z"
   git push
   ```

## Update the package for a new release

Run after the new version is published to crates.io (see
[`../../RELEASING.md`](../../RELEASING.md) §7):

1. Bump `pkgver` to the new version and reset `pkgrel=1` in `PKGBUILD` (bump
   `pkgrel` instead if only the packaging changed, not the upstream version).
2. The source uses `sha256sums=('SKIP')`; if you switch to pinned checksums,
   refresh them with `updpkgsums`.
3. Regenerate the metadata:
   ```sh
   makepkg --printsrcinfo > .SRCINFO
   ```
4. Test the build locally with `makepkg -si`.
5. Commit and push `PKGBUILD` + `.SRCINFO` to `aur.archlinux.org`.

Keep this `PKGBUILD`/`.SRCINFO` in sync with the copy pushed to the AUR — the
`tests/aur_packaging.rs` golden tests fail CI if `pkgver` drifts from
`Cargo.toml`.

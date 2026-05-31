# Releasing Burst

Official releases of Burst are cut by the project owner (see
[`MAINTAINERS.md`](MAINTAINERS.md)). This document is the canonical, repeatable
checklist for publishing a release across crates.io, GitHub, and the AUR.

Every release candidate must pass the same gates CI enforces before it is
published. Do not skip steps, and do not `--no-verify`.

## 0. Preconditions

- You are the project owner (or acting with the owner's authorization).
- `main` is green: the [CI workflow](.github/workflows/ci.yml) passes
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test --all-targets` on the latest push to `main`.
- Working tree is clean (`git status`) and you are on an up-to-date `main`.

## 1. Decide the version bump

Burst follows semantic versioning. Pick the bump from the changes since the last
release:

| Change | Bump |
| --- | --- |
| Breaking change (`feat!` / `BREAKING CHANGE:`) | major |
| New feature (`feat`) | minor |
| Bug fix / perf (`fix`, `perf`) | patch |
| Docs / chore / refactor only | usually none — skip the release |

Edit `[package].version` in `Cargo.toml`, then run `cargo build` to refresh
`Cargo.lock`. Stage both files.

## 2. Update the changelog

Record the release notes in `CHANGELOG.md` (added in Phase 4). Add a new section
for the version with the date and a summary of user-facing changes, grouped by
Added / Changed / Fixed. The changelog entry is the source of truth for the
GitHub release body.

## 3. Validate package readiness

Run the same checks the [release-readiness
workflow](.github/workflows/release-readiness.yml) runs, locally, before tagging:

```sh
# Confirm only intended files are packaged (src, tests, packaging, docs, license).
cargo package --list

# Build the crate exactly as crates.io would and verify it is publishable.
cargo publish --dry-run
```

Review the `cargo package --list` output against the `include` rules in
`Cargo.toml`. There must be no stray files, secrets, or local artifacts.

## 4. Commit, tag, and push

Commit the version bump and changelog:

```sh
git commit -am "chore(release): vX.Y.Z"
```

Create an annotated tag that matches the `Cargo.toml` version exactly (the
release-readiness workflow validates `vX.Y.Z` against the crate version):

```sh
git tag -a vX.Y.Z -m "Burst vX.Y.Z"
git push origin main
git push origin vX.Y.Z
```

Pushing the tag triggers the release-readiness workflow. Wait for it to pass.

## 5. Publish to crates.io

Once the dry run and the tag-triggered workflow are green, publish for real:

```sh
cargo publish
```

Only the crate owner can publish. If `cargo publish` fails after partial upload,
the version is burned on crates.io and cannot be reused — bump to the next patch
and start over from step 1. crates.io ownership is held by the project owner; do
not add owners without the owner's approval.

## 6. Create the GitHub release

Create a GitHub release for the `vX.Y.Z` tag, using the changelog section as the
body:

```sh
gh release create vX.Y.Z --title "Burst vX.Y.Z" --notes-file <(sed -n '/## vX.Y.Z/,/## v/p' CHANGELOG.md)
```

Attach prebuilt binaries here later if the project starts shipping them.

## 7. Update the AUR package

Update the AUR package (see Phase 5 packaging plan) to the new version:

- Bump `pkgver` and reset `pkgrel` to `1` in the `PKGBUILD`.
- Refresh checksums against the new crates.io / GitHub source tarball.
- Regenerate `.SRCINFO` (`makepkg --printsrcinfo > .SRCINFO`).
- Test the build locally with `makepkg -si`.
- Commit and push to the AUR remote.

## 8. Verify the published artifacts

- `cargo install burst` installs the new version from crates.io.
- The AUR package installs and runs.
- README install instructions match the published artifacts.

## Cargo.lock policy

Burst is a binary crate, so `Cargo.lock` is committed and shipped in the
published package (it is listed in the `include` set in `Cargo.toml`). This keeps
release builds reproducible. Never `.gitignore` `Cargo.lock`.

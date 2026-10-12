# Releasing

1. In a PR, set the new version in `Cargo.toml`, `crates/tnc2-core/Cargo.toml`, `crates/tnc2-node/Cargo.toml` and `crates/tnc2-pico/Cargo.toml`, then run `cargo update -w` at the top and in `crates/tnc2-pico`, `tools/qemu-pace` and `tools/m33bench`, to update the lock files. `tools/check-version.sh` says whether everything agrees, and CI runs it too.
2. Merge the PR.
3. Tag the merge commit by its full SHA (the PR page shows it), not by a branch name, and push the tag. With the SHA of your merge commit in place of this one:

   ```sh
   git fetch origin
   git tag -a v0.2.0 0f3c9a1d2b4e5f60718293a4b5c6d7e8f9012345 -m v0.2.0
   git push origin v0.2.0
   ```

That is all. The `release` workflow checks the tag matches the crate versions, runs the tests, builds the Linux archives, the Pico 2 W UF2s and `SHA256SUMS` (`tools/release-assets.sh`), and publishes a GitHub release titled with the tag. Its description is `.github/release-assets.md`, the guide to which file is which, followed by GitHub's list of the PRs merged since the previous tag. The `publish` workflow pushes the Docker image as `ghcr.io/packethacking/tnc2-emu:0.2.0` (and `:0.2`, `:latest`) from the same tag.

If the release workflow fails for a reason outside the code (a download, a runner), run it again from the Actions tab (Run workflow, with the tag). A fix to the code goes out as the next version instead, because the tag stays where it is. It never replaces a release that already exists: to cut one again, delete it first with `gh release delete v0.2.0`, which keeps the tag.

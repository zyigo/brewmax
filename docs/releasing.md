# Releasing brewmax

brewmax is installed with:

```sh
brew install zyigo/tap/brewmax
```

That formula lives in [zyigo/homebrew-tap](https://github.com/zyigo/homebrew-tap) and installs a prebuilt Apple silicon binary attached to a GitHub Release of this repo.

## Cutting a release

1. **Bump the version** in `Cargo.toml` (for example `0.2.0`), then run `cargo build` so `Cargo.lock` picks it up. Commit both:

   ```sh
   git commit -am "Release 0.2.0"
   ```

2. **Tag and push.** The tag must be `v` plus the `Cargo.toml` version, or the release workflow stops:

   ```sh
   git tag v0.2.0
   git push origin main v0.2.0
   ```

3. **Wait for the Release workflow** (`.github/workflows/release.yml`). It:
   - checks the tag matches `Cargo.toml`;
   - runs the tests;
   - builds `brewmax` on Apple silicon;
   - publishes a GitHub Release with `brewmax-<version>-aarch64-apple-darwin.tar.gz` and its `.sha256`.

   Watch it with `gh run watch`.

4. **Point the tap at the release:**

   ```sh
   scripts/update-tap.sh 0.2.0
   ```

   This writes `Formula/brewmax.rb` with the new URL and SHA-256, then commits and pushes it to `zyigo/homebrew-tap`. You need `gh` logged in with push access to the tap.

5. **Check it installs:**

   ```sh
   brew update
   brew upgrade zyigo/tap/brewmax   # or: brew install zyigo/tap/brewmax
   brew test zyigo/tap/brewmax
   ```

## If something goes wrong

- **The workflow failed before publishing:** fix the problem, then move the tag to the fixed commit:

  ```sh
  git tag -f v0.2.0
  git push -f origin v0.2.0
  ```

- **A bad release was published:** delete it, then cut a new patch version:

  ```sh
  gh release delete v0.2.0 --cleanup-tag
  ```

  Don't re-upload a different binary under the same version: the formula pins its SHA-256, and anyone who already installed it would get a checksum mismatch.

brewmax can't update itself while it's running, so it Holds itself (see `GLOSSARY.md`). Quit it, then run `brew upgrade brewmax`.

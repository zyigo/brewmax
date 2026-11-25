#!/usr/bin/env bash
# Point the Homebrew formula in zyigo/homebrew-tap at a published release.
#
#   scripts/update-tap.sh 0.1.0
#
# Reads the release's SHA-256 from GitHub, writes Formula/brewmax.rb in a
# fresh clone of the tap, commits and pushes it. Needs `gh` logged in with
# push access to zyigo/homebrew-tap. See docs/releasing.md.
set -euo pipefail

version="${1:?usage: scripts/update-tap.sh <version, e.g. 0.1.0>}"
version="${version#v}"
repo="zyigo/brewmax"
tap="zyigo/homebrew-tap"
asset="brewmax-$version-aarch64-apple-darwin.tar.gz"
url="https://github.com/$repo/releases/download/v$version/$asset"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

gh release download "v$version" --repo "$repo" --pattern "$asset.sha256" --dir "$work"
sha256="$(cut -d' ' -f1 "$work/$asset.sha256")"
if [[ ! "$sha256" =~ ^[0-9a-f]{64}$ ]]; then
  echo "Could not read a SHA-256 for $asset" >&2
  exit 1
fi

gh repo clone "$tap" "$work/tap" -- --quiet
mkdir -p "$work/tap/Formula"
cat > "$work/tap/Formula/brewmax.rb" <<FORMULA
class Brewmax < Formula
  desc "Terminal UI for Homebrew that won't change software you're using"
  homepage "https://github.com/$repo"
  url "$url"
  sha256 "$sha256"
  version "$version"

  depends_on arch: :arm64
  depends_on :macos

  def install
    bin.install "brewmax"
  end

  test do
    assert_equal "brewmax #{version}", shell_output("#{bin}/brewmax --version").strip
  end
end
FORMULA

cd "$work/tap"
if git diff --quiet -- Formula/brewmax.rb && git ls-files --error-unmatch Formula/brewmax.rb >/dev/null 2>&1; then
  echo "Formula already points at $version; nothing to do."
  exit 0
fi
git add Formula/brewmax.rb
git commit --quiet -m "brewmax $version"
git push --quiet
echo "Updated $tap: brewmax $version ($sha256)"

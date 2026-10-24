#!/bin/bash
# Update the homebrew-tap repo's macclean cask after a release.
# Usage: scripts/update_tap.sh <tag> <dmg-path>
# Needs TAP_GITHUB_TOKEN (PAT with repo scope on ranb27/homebrew-tap).
# No-op if the token is absent — safe to call unconditionally in CI.
set -euo pipefail

TAG="${1:?usage: update_tap.sh <tag> <dmg-path>}"
DMG="${2:?usage: update_tap.sh <tag> <dmg-path>}"
VER="${TAG#v}"

if [ -z "${TAP_GITHUB_TOKEN:-}" ]; then
  echo "TAP_GITHUB_TOKEN not set — skipping tap update"
  exit 0
fi

SHA=$(shasum -a 256 "$DMG" | cut -d' ' -f1)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

git clone --depth 1 "https://x-access-token:${TAP_GITHUB_TOKEN}@github.com/ranb27/homebrew-tap.git" "$WORK/tap"
mkdir -p "$WORK/tap/Casks"

cat > "$WORK/tap/Casks/macclean.rb" <<EOF
cask "macclean" do
  version "$VER"
  sha256 "$SHA"

  url "https://github.com/ranb27/macclean/releases/download/v#{version}/macclean-macos-arm64.dmg"
  name "macclean"
  desc "A disk cleaner that can't hurt your Mac — deletes through the Trash, not rm"
  homepage "https://ranb27.github.io/macclean"

  depends_on arch: :arm64
  depends_on macos: :big_sur

  app "macclean.app"

  # Homebrew quarantines every cask since v7 (no opt-out flag). Our own tap
  # can strip it post-install — the app then opens with no warning at all.
  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-dr", "com.apple.quarantine", "#{appdir}/macclean.app"]
  end

  caveats <<~EOS
    macclean is unsigned — the cask removes the quarantine attribute on
    install, so it opens cleanly with no Gatekeeper warning.
  EOS
end
EOF

cd "$WORK/tap"
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git add -A
if git diff --cached --quiet; then
  echo "cask already at $TAG — nothing to push"
else
  git commit -m "macclean $TAG"
  git push
  echo "homebrew-tap updated to $TAG (sha256 $SHA)"
fi

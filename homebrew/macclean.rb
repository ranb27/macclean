cask "macclean" do
  version "0.3.1"
  sha256 "8076b50f15d29fc2d8476549ca0e18aa03a9e5f66d59200757fd67400e65b4db"

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

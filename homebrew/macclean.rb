cask "macclean" do
  version "0.3.0"
  sha256 "964afdc24142c07652d8361fd813c5055cbf297a48b347b67256969b04d8b09e"

  url "https://github.com/ranb27/macclean/releases/download/v#{version}/macclean-macos-arm64.dmg"
  name "macclean"
  desc "A disk cleaner that can't hurt your Mac — deletes through the Trash, not rm"
  homepage "https://ranb27.github.io/macclean"

  depends_on arch: :arm64
  depends_on macos: ">= :big_sur"

  app "macclean.app"

  caveats <<~EOS
    macclean is unsigned. To skip the Gatekeeper warning entirely:
      brew install --cask macclean --no-quarantine
    Otherwise right-click the app → Open once.
  EOS
end

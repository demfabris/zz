# The casks published to github.com/demfabris/homebrew-zz. Their source lives
# in packaging/homebrew/zz.rb in demfabris/zz; the release workflow fills in
# the version and checksum with scripts/render-cask.sh and pushes the result to
# the tap, so edit the template rather than the published copies. The same
# template renders the `zz@beta` cask: the renderer retargets the token, the
# conflict, and drops livecheck, which only knows the stable release.
cask "zz" do
  version "0.0.0"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://github.com/demfabris/zz/releases/download/v#{version}/zz-#{version}-macos-arm64.dmg",
      verified: "github.com/demfabris/zz/"
  name "zz"
  desc "Terminal multiplexer with terminal, browser, and agent panes"
  homepage "https://github.com/demfabris/zz"

  livecheck do
    url :url
    strategy :github_latest
  end

  conflicts_with cask: "zz@beta"
  # The bundle carries a full Chromium; only the arm64 slice is released.
  depends_on arch: :arm64
  depends_on macos: :big_sur

  app "zz.app"
  # `cli` is the headless zz (CLI and daemon) that the app bundles.
  # The `zz` symlink on PATH points to it; `zz app` opens the desktop app.
  binary "#{appdir}/zz.app/Contents/MacOS/cli", target: "zz"

  zap trash: [
    "~/.config/zz",
    "~/Library/Application Support/zz",
    "~/Library/Saved Application State/dev.zz.app.savedState",
  ]
end

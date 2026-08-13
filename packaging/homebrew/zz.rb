# The cask published to github.com/demfabris/homebrew-zz. Its source lives in
# packaging/homebrew/zz.rb in demfabris/zz; the release workflow fills in the
# version and checksum with scripts/render-cask.sh and pushes the result to the
# tap, so edit the template rather than the published copy.
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

  # The bundle carries a full Chromium; only the arm64 slice is released.
  depends_on arch: :arm64
  depends_on macos: :big_sur

  app "zz.app"
  # macOS resolves an app bundle from the path the executable was launched with
  # and does not follow symlinks doing it, so a symlink to Contents/MacOS/zz
  # would start zz with no Info.plist. `cli` is a launcher that canonicalizes
  # itself and execs the real executable from inside the bundle.
  binary "#{appdir}/zz.app/Contents/MacOS/cli", target: "zz"

  zap trash: [
    "~/.config/zz",
    "~/Library/Application Support/zz",
    "~/Library/Saved Application State/dev.zz.app.savedState",
  ]
end

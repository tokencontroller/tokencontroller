# The release workflow (.github/workflows/release.yml) rewrites this file with
# the download of each release for macOS and Linux. Until the first release,
# tc builds from the main branch: brew install --HEAD tc
# brew tap tokencontroller/tap https://github.com/tokencontroller/tokencontroller
class Tc < Formula
  desc "Cost accounting for AI agents: reads session files, submits token sheets"
  homepage "https://docs.tokencontroller.com/collect/cli"
  license "MIT"

  head do
    url "https://github.com/tokencontroller/tokencontroller.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    system "cargo", "install", *std_cargo_args(path: "apps/cli")
    pkgshare.install "apps/cli/classifier/pick-ticket"
  end

  test do
    assert_match "tc ", shell_output("#{bin}/tc --version")
  end
end

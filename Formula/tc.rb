# Written by .github/workflows/release.yml from apps/cli/packaging/release.py.
# brew tap tokencontroller/tap https://github.com/tokencontroller/tokencontroller
# brew install tc
class Tc < Formula
  desc "Cost accounting for AI agents: reads session files, submits token sheets"
  homepage "https://docs.tokencontroller.com/collect/cli"
  version "1.0.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/tokencontroller/tokencontroller/releases/download/v1.0.0/tc-aarch64-apple-darwin.tar.gz"
      sha256 "c6ea783ac19eb1fa1a86fbdb33e2b9f128aa40a0b58f9fdea737508145218840"
    end
    on_intel do
      url "https://github.com/tokencontroller/tokencontroller/releases/download/v1.0.0/tc-x86_64-apple-darwin.tar.gz"
      sha256 "5d3a7f5cd3c2b5aa0ea52ac2100a4b58f7d6df46e134bf2dbe932d42363ce1f7"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/tokencontroller/tokencontroller/releases/download/v1.0.0/tc-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "6d19a0fe83160346a0f9233e60bbe6ea3ff65ce9ad45665ad0eb450ccd2da5a5"
    end
    on_intel do
      url "https://github.com/tokencontroller/tokencontroller/releases/download/v1.0.0/tc-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "b0da2a60c0d4d8b2401c1d8b6b68cbecb61cab78716300295ba7e9ee9cac5d6c"
    end
  end

  head do
    url "https://github.com/tokencontroller/tokencontroller.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args(path: "apps/cli")
      pkgshare.install "apps/cli/classifier/pick-ticket"
    else
      bin.install "tc"
      pkgshare.install "pick-ticket"
    end
  end

  test do
    assert_match "tc ", shell_output("#{bin}/tc --version")
  end
end

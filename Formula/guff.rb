# Homebrew formula for guff (not yet in homebrew-core).
#
#   brew tap dakimura/guff https://github.com/dakimura/guff
#   brew install guff
#
# Update shas when cutting a release (see docs/INSTALL.md).
class Guff < Formula
  desc "Blazing-fast golangci-lint compatible Go linter"
  homepage "https://github.com/dakimura/guff"
  version "0.4.1"
  license "GPL-3.0-only"

  on_macos do
    on_arm do
      url "https://github.com/dakimura/guff/releases/download/v0.4.1/guff_0.4.1_darwin_arm64.tar.gz"
      sha256 "8404c50fc829e24f676bd6a0471d73db754109aa1e78598723105ce9bc6650fd"
    end
    on_intel do
      url "https://github.com/dakimura/guff/releases/download/v0.4.1/guff_0.4.1_darwin_amd64.tar.gz"
      sha256 "12117e3190e9cbe8798156393df98b377a187ee666ae579fc2be5c6c4278ec62"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/dakimura/guff/releases/download/v0.4.1/guff_0.4.1_linux_arm64.tar.gz"
      sha256 "3b45c0052fb24f3f38aebf1a82c9bb0bdcc414024ce6f397bc6cdad34460fd21"
    end
    on_intel do
      url "https://github.com/dakimura/guff/releases/download/v0.4.1/guff_0.4.1_linux_amd64.tar.gz"
      sha256 "736da819d528bccfb1e44a08e9e500d243596c54766fb479b0cf2405a7d60c60"
    end
  end

  depends_on "go"

  def install
    bin.install "guff"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/guff version --short")
  end
end

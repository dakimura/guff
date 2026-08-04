# Homebrew formula for guff (not yet in homebrew-core).
#
#   brew tap dakimura/guff https://github.com/dakimura/guff
#   brew install guff
#
# Update shas when cutting a release (see docs/INSTALL.md).
class Guff < Formula
  desc "Blazing-fast golangci-lint compatible Go linter"
  homepage "https://github.com/dakimura/guff"
  version "0.3.0"
  license "GPL-3.0-only"

  on_macos do
    on_arm do
      url "https://github.com/dakimura/guff/releases/download/v0.3.0/guff_0.3.0_darwin_arm64.tar.gz"
      sha256 "1614c2df99e9ee1d0ee3b82df1a0c89d3de1715b43380d1cb25898c75ad52ebc"
    end
    on_intel do
      url "https://github.com/dakimura/guff/releases/download/v0.3.0/guff_0.3.0_darwin_amd64.tar.gz"
      sha256 "99222fe7315a50c8163f17be0c564c58d282b83acbecca66b18ff867e2a8fb0f"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/dakimura/guff/releases/download/v0.3.0/guff_0.3.0_linux_arm64.tar.gz"
      sha256 "ffabc9a394b202805107e6285cdfae3e5f8eca23364f5650d1900f954f57e8ee"
    end
    on_intel do
      url "https://github.com/dakimura/guff/releases/download/v0.3.0/guff_0.3.0_linux_amd64.tar.gz"
      sha256 "b0b72afe61fa4227057c7473b64c5fd98c6cbfb1312b228124e2d52ea41a9f8e"
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

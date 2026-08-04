# Homebrew formula for guff (not yet in homebrew-core).
#
#   brew tap dakimura/guff https://github.com/dakimura/guff
#   brew install guff
#
# Update shas when cutting a release (see docs/INSTALL.md).
class Guff < Formula
  desc "Blazing-fast golangci-lint compatible Go linter"
  homepage "https://github.com/dakimura/guff"
  version "0.2.0"
  license "GPL-3.0-only"

  on_macos do
    on_arm do
      url "https://github.com/dakimura/guff/releases/download/v0.2.0/guff_0.2.0_darwin_arm64.tar.gz"
      sha256 "046ecc88a151b9ab14936d569f6c0cf0490f9fb184669ba4c46ed7ea117a1231"
    end
    on_intel do
      url "https://github.com/dakimura/guff/releases/download/v0.2.0/guff_0.2.0_darwin_amd64.tar.gz"
      sha256 "24d78f3259dfe1c2169fc2cadd094453fae0a81dbcfca74d5c9e8d7ee150ee54"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/dakimura/guff/releases/download/v0.2.0/guff_0.2.0_linux_arm64.tar.gz"
      sha256 "6f414a9ffe6c2b38e6d63dc2caa71e930539f746c77a0f1ccad08ee4fe8f0d5f"
    end
    on_intel do
      url "https://github.com/dakimura/guff/releases/download/v0.2.0/guff_0.2.0_linux_amd64.tar.gz"
      sha256 "8e6d7194bbe6db0c59d5d64b3f73c4d0d3353371285a660434dc3e3987a758fb"
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

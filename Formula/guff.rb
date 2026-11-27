# Homebrew formula for guff (not yet in homebrew-core).
#
#   brew tap dakimura/guff https://github.com/dakimura/guff
#   brew install guff
#
# Update shas when cutting a release (see docs/INSTALL.md).
class Guff < Formula
  desc "Blazing-fast golangci-lint compatible Go linter"
  homepage "https://github.com/dakimura/guff"
  version "0.9.0"
  license "GPL-3.0-only"

  on_macos do
    on_arm do
      url "https://github.com/dakimura/guff/releases/download/v0.9.0/guff_0.9.0_darwin_arm64.tar.gz"
      sha256 "7cf7d428e5d6f5ac2bb8cd50ac993be8fc2f39d63bf5d42681f83c43e52bb9e3"
    end
    on_intel do
      url "https://github.com/dakimura/guff/releases/download/v0.9.0/guff_0.9.0_darwin_amd64.tar.gz"
      sha256 "d143d71bb041918ab9cf1351d9b64bd8168d5ff82b4047e28cef2463b5f623f5"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/dakimura/guff/releases/download/v0.9.0/guff_0.9.0_linux_arm64.tar.gz"
      sha256 "fee1dc0fdfdf3ac85b6de8a0d8d9d669072d23abb3acc052b9776a6773dc4f2b"
    end
    on_intel do
      url "https://github.com/dakimura/guff/releases/download/v0.9.0/guff_0.9.0_linux_amd64.tar.gz"
      sha256 "0b9af059131ba3a76dd1d446490485ea64afceeed45b89881c1a4ee9e2fe83ff"
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

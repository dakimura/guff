# Homebrew formula for guff (not yet in homebrew-core).
#
#   brew tap dakimura/guff https://github.com/dakimura/guff
#   brew install guff
#
# Update shas when cutting a release (see docs/INSTALL.md).
class Guff < Formula
  desc "Blazing-fast golangci-lint compatible Go linter"
  homepage "https://github.com/dakimura/guff"
  version "0.4.0"
  license "GPL-3.0-only"

  on_macos do
    on_arm do
      url "https://github.com/dakimura/guff/releases/download/v0.4.0/guff_0.4.0_darwin_arm64.tar.gz"
      sha256 "ea66c5e8dd9f6457a96c633b1594f99092f91f2e7ab7faab907a496cfff892df"
    end
    on_intel do
      url "https://github.com/dakimura/guff/releases/download/v0.4.0/guff_0.4.0_darwin_amd64.tar.gz"
      sha256 "9a95e11fb506b5642c1d81cab11478fe68e89f8af7b2b340f341877a09cf229f"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/dakimura/guff/releases/download/v0.4.0/guff_0.4.0_linux_arm64.tar.gz"
      sha256 "a3b241088c30f7c85cbbbc6098d33dedcaeed9b071733362038be32ec2b0fc2f"
    end
    on_intel do
      url "https://github.com/dakimura/guff/releases/download/v0.4.0/guff_0.4.0_linux_amd64.tar.gz"
      sha256 "3b3ecf4a0b07328b50574a9ee73e6a0a5b2a84f311025e6668bd0ab084f78d94"
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

# typed: false
# frozen_string_literal: true

# Wlel is distributed as prebuilt archives. To publish, push this file to a
# tap repository (for example WildanDeveloper/homebrew-wlel) as Formula/wlel.rb;
# users then run `brew install WildanDeveloper/wlel/wlel`.
class Wlel < Formula
  desc "Arena-first systems programming language with a C backend"
  homepage "https://github.com/WildanDeveloper/wlel"
  version "1.2.0"
  license "Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/WildanDeveloper/wlel/releases/download/v1.2.0/wlel-1.2.0-aarch64-apple-darwin.tar.gz"
      sha256 "134e9a227984819804e9560ae20a3f162b647464df62ea9a087831563f12a81a"
    else
      url "https://github.com/WildanDeveloper/wlel/releases/download/v1.2.0/wlel-1.2.0-x86_64-apple-darwin.tar.gz"
      sha256 "837eb430f2ab3d65fd1660a67533ab142ae0abca39f8606939bc2bfe8446b854"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/WildanDeveloper/wlel/releases/download/v1.2.0/wlel-1.2.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "dc2eab5d373a2d46e7b371e24fd7eafc05c1e637ab7be61d2276828505c97e35"
    else
      url "https://github.com/WildanDeveloper/wlel/releases/download/v1.2.0/wlel-1.2.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "fea7536423e46f80cb17581f1c9a45492382df258cdb9254f65da781603dd059"
    fi
  end

  def install
    bin.install Dir["*/wlel"].first
    doc.install "*/README.md"
  end

  test do
    assert_match "wlel #{version}", shell_output("#{bin}/wlel --version")
  end
end

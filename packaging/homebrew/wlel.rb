# typed: false
# frozen_string_literal: true

# Wlel is distributed as prebuilt archives. To publish, push this file to a
# tap repository (for example WildanDeveloper/homebrew-wlel) as Formula/wlel.rb;
# users then run `brew install WildanDeveloper/wlel/wlel`.
class Wlel < Formula
  desc "Arena-first systems programming language with a C backend"
  homepage "https://github.com/WildanDeveloper/wlel"
  version "1.3.0"
  license "Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/WildanDeveloper/wlel/releases/download/v1.3.0/wlel-1.3.0-aarch64-apple-darwin.tar.gz"
      sha256 "769cf83efd6726305accfac338ba0f4d82074cd8cf4c99344728eae4b2a26df6"
    else
      url "https://github.com/WildanDeveloper/wlel/releases/download/v1.3.0/wlel-1.3.0-x86_64-apple-darwin.tar.gz"
      sha256 "c9b83bd7ea22b3c5b197107a2765bb631baf2721e2386fcfe2524693294689d6"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/WildanDeveloper/wlel/releases/download/v1.3.0/wlel-1.3.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "7611665337b8b38e393bd6844157be2637a36582187baf663e2104fce9de919a"
    else
      url "https://github.com/WildanDeveloper/wlel/releases/download/v1.3.0/wlel-1.3.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "21741fbbfd0c3a5b196ab5aeb41a6cdcd52bc9409f845696a93685851fbc648a"
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

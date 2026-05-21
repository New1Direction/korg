# Homebrew Formula Template for korg
# To deploy, place this in your homebrew-tap repository under Formula/korg.rb

class Korg < Formula
  desc "Korg Heavy-Tier Agent Swarm & Knowledge Vault Orchestrator"
  homepage "https://github.com/clubpenguin/Korg"
  license "MIT"
  version "0.1.0"

  # Update these URLs and SHA256 hashes when publishing new releases
  if OS.mac?
    if Hardware::CPU.intel?
      url "https://github.com/clubpenguin/Korg/releases/download/v#{version}/korg-macos-x86_64"
      sha256 "REPLACE_WITH_MACOS_X86_64_SHA256"
    else
      url "https://github.com/clubpenguin/Korg/releases/download/v#{version}/korg-macos-arm64"
      sha256 "REPLACE_WITH_MACOS_ARM64_SHA256"
    end
  elsif OS.linux?
    url "https://github.com/clubpenguin/Korg/releases/download/v#{version}/korg-linux-x86_64"
    sha256 "REPLACE_WITH_LINUX_X86_64_SHA256"
  end

  def install
    # Rename binary to 'korg' and install it into homebrew bin
    binary_name = OS.mac? ? (Hardware::CPU.intel? ? "korg-macos-x86_64" : "korg-macos-arm64") : "korg-linux-x86_64"
    bin.install binary_name => "korg"
  end

  test do
    # Simple check that the binary executes and prints version info
    assert_match "korg #{version}", shell_output("#{bin}/korg --version")
  end
end

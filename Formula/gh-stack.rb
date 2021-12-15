class GhStack < Formula
  desc "Manage stacked PR workflows on Github via the command line"
  homepage "https://github.com/luqven/gh-stack"
  version "v0.3.0-beta-2"
  url "https://github.com/luqven/gh-stack/releases/download/#{version}/gh-stack-mac.tar.gz"
  sha256 "7a4ece47eaae0877a9a0611807a29d8fd111b2531e150641538348fcc258e515"

  def install
    bin.install "gh-stack"
  end
end
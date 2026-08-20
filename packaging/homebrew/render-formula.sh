#!/usr/bin/env bash
# Render the Homebrew formula for a given release version.
# Usage: packaging/homebrew/render-formula.sh 0.1.0
set -euo pipefail

VERSION="${1:?usage: render-formula.sh <version>}"
REPO="${REPO:-timonviola/sensors.rs}"
BASE="https://github.com/${REPO}/releases/download/v${VERSION}"

sums="$(curl -fsSL "${BASE}/SHA256SUMS")"

sha_for() {
  echo "${sums}" | awk -v f="sensors-${VERSION}-$1.tar.gz" '$2 == f || $2 == "*"f { print $1 }'
}

SHA_ARM_MAC="$(sha_for aarch64-apple-darwin)"
SHA_X86_MAC="$(sha_for x86_64-apple-darwin)"
SHA_ARM_LINUX="$(sha_for aarch64-unknown-linux-musl)"
SHA_X86_LINUX="$(sha_for x86_64-unknown-linux-musl)"

for v in SHA_ARM_MAC SHA_X86_MAC SHA_ARM_LINUX SHA_X86_LINUX; do
  [ -n "${!v}" ] || { echo "missing checksum for ${v}" >&2; exit 1; }
done

cat <<EOF
# typed: false
# frozen_string_literal: true

class SensorsRs < Formula
  desc "Fast, dependency-free reimplementation of lm-sensors' sensors(1) for macOS and Linux"
  homepage "https://github.com/${REPO}"
  version "${VERSION}"
  license "MIT"

  on_macos do
    on_arm do
      url "${BASE}/sensors-${VERSION}-aarch64-apple-darwin.tar.gz"
      sha256 "${SHA_ARM_MAC}"
    end
    on_intel do
      url "${BASE}/sensors-${VERSION}-x86_64-apple-darwin.tar.gz"
      sha256 "${SHA_X86_MAC}"
    end
  end

  on_linux do
    on_arm do
      url "${BASE}/sensors-${VERSION}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "${SHA_ARM_LINUX}"
    end
    on_intel do
      url "${BASE}/sensors-${VERSION}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "${SHA_X86_LINUX}"
    end
  end

  def install
    bin.install "sensors"
  end

  test do
    assert_match "sensors", shell_output("#{bin}/sensors --version")
  end
end
EOF

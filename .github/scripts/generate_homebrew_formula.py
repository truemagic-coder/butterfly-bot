#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path


def render_formula(version: str, url: str, sha256: str) -> str:
    return f'''class ButterflyBot < Formula
  desc "Local-first AI assistant with CLI, daemon, desktop UI, and WASM tools"
  homepage "https://github.com/truemagic-coder/butterfly-bot"
  url "{url}"
  sha256 "{sha256}"
  version "{version}"
  license "MIT"
  head "https://github.com/truemagic-coder/butterfly-bot.git", branch: "main"

  depends_on "pkgconf" => :build
  depends_on "rust" => :build
  depends_on "protobuf" => :build

  def install
    system "cargo", "build", "--release", "--locked", "--bin", "butterfly-bot", "--bin", "butterfly-botd"

    libexec.install "target/release/butterfly-bot"
    libexec.install "target/release/butterfly-botd"

    wasm_modules = Dir["wasm/*_tool.wasm"]
    odie "No wasm tool modules found under wasm/" if wasm_modules.empty?
    pkgshare.install wasm_modules

    (bin/"butterfly-bot").write <<~EOS
      #!/bin/bash
      set -euo pipefail

      APP_ROOT="${{HOME}}/.butterfly-bot"
      mkdir -p "${{APP_ROOT}}" "${{APP_ROOT}}/data"

      if [[ -e "${{APP_ROOT}}/wasm" && ! -L "${{APP_ROOT}}/wasm" ]]; then
        rm -rf "${{APP_ROOT}}/wasm"
      fi
      if [[ ! -L "${{APP_ROOT}}/wasm" ]]; then
        ln -sfn "#{{pkgshare}}" "${{APP_ROOT}}/wasm"
      fi

      cd "${{APP_ROOT}}"
      export BUTTERFLY_BOT_DB="${{BUTTERFLY_BOT_DB:-${{APP_ROOT}}/data/butterfly-bot.db}}"

      exec "#{{libexec}}/butterfly-bot" "$@"
    EOS

    (bin/"butterfly-botd").write <<~EOS
      #!/bin/bash
      set -euo pipefail

      APP_ROOT="${{HOME}}/.butterfly-bot"
      mkdir -p "${{APP_ROOT}}" "${{APP_ROOT}}/data"

      if [[ -e "${{APP_ROOT}}/wasm" && ! -L "${{APP_ROOT}}/wasm" ]]; then
        rm -rf "${{APP_ROOT}}/wasm"
      fi
      if [[ ! -L "${{APP_ROOT}}/wasm" ]]; then
        ln -sfn "#{{pkgshare}}" "${{APP_ROOT}}/wasm"
      fi

      cd "${{APP_ROOT}}"
      export BUTTERFLY_BOT_DB="${{BUTTERFLY_BOT_DB:-${{APP_ROOT}}/data/butterfly-bot.db}}"

      exec "#{{libexec}}/butterfly-botd" --db "${{BUTTERFLY_BOT_DB}}" "$@"
    EOS

    chmod 0755, bin/"butterfly-bot"
    chmod 0755, bin/"butterfly-botd"
  end

  def caveats
    <<~EOS
      Installed commands:
        butterfly-bot    (primary desktop app)
        butterfly-botd   (daemon launcher)

      Runtime data directory:
        ~/.butterfly-bot

      WASM tool modules are installed at:
        #{{pkgshare}}
    EOS
  end

  test do
    output = shell_output("#{{bin}}/butterfly-bot --help")
    assert_match "ButterFly Bot CLI", output
  end
end
'''


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Generate Homebrew formula for Butterfly release"
    )
    parser.add_argument("--version", required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    formula = render_formula(args.version, args.url, args.sha256)
    out_path = Path(args.output)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(formula, encoding="utf-8")


if __name__ == "__main__":
    main()

#!/usr/bin/env bash
# 用 dmgbuild 把 .app 打成 DMG，再检查 .DS_Store 写对了（缺了或位置不对就失败）。
#   tools/dmg/build.sh <PonteX One.app> <输出.dmg>
set -euo pipefail
app="$1"; out="$2"
root="$(cd "$(dirname "$0")/../.." && pwd)"
here="$root/tools/dmg"
venv="${PONTEX_DMG_VENV:-${TMPDIR:-/tmp}/pontex-dmg-venv}"
if [[ ! -x "$venv/bin/dmgbuild" ]]; then
  python3 -m venv "$venv"
  "$venv/bin/pip" install -q --disable-pip-version-check -r "$here/requirements.txt"
fi
rm -f "$out"
mkdir -p "$(dirname "$out")"
"$venv/bin/dmgbuild" -s "$here/settings.py" \
  -D app="$app" \
  -D icon="$root/src-tauri/pontex/icons/icon.icns" \
  -D background="$root/src-tauri/pontex/icons/dmg.tiff" \
  "PonteX One" "$out"
"$venv/bin/python" "$here/verify.py" "$out"

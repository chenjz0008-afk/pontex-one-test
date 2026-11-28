"""补上新版访达认的背景引用（pBB0 书签），再压缩成最终的 DMG。

    python finish.py <dmgbuild 打出的可写 DMG> <输出.dmg>

dmgbuild 1.6 只写旧的 backgroundImageAlias。对照实验（同一台 macOS 15 测试机）：
它打的 DMG 不显示背景；成熟软件的 DMG（Rectangle）同时带 pBB0 书签，背景正常。
所以挂载在 /Volumes/<卷名> 下补一个指向背景图的 pBB0，和访达自己存的一样。
"""

import os
import subprocess
import sys

from ds_store import DSStore
from mac_alias import Bookmark


def run(*args: str) -> str:
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def main(source: str, out: str) -> None:
    # 挂到 /Volumes/<卷名>：书签里记的路径和用户双击打开时一致
    attached = run("hdiutil", "attach", "-readwrite", "-nobrowse", "-noautoopen", source)
    mount = attached.strip().splitlines()[-1].split("\t")[-1]
    try:
        background = os.path.join(mount, ".background.tiff")
        if not os.path.exists(background):
            sys.exit("✗ 可写 DMG 里没有 .background.tiff")
        with DSStore.open(os.path.join(mount, ".DS_Store"), "r+") as store:
            store["."]["pBB0"] = ("blob", Bookmark.for_file(background).to_bytes())
        run("sync")
    finally:
        run("hdiutil", "detach", mount)
    if os.path.exists(out):
        os.remove(out)
    run("hdiutil", "convert", source, "-format", "ULMO", "-o", out)
    print(f"✓ 补上背景书签并压缩：{out}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])

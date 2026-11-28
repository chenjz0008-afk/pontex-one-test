"""检查 DMG 的拖拽安装窗口：.DS_Store 在、背景在、图标位置和窗口设置都对。不对就退出码 1。

    python verify.py <PonteX-One.dmg>

0.11.0 之前的 DMG 在 CI 上打包时跳过了写 .DS_Store，打开是一个没有背景、图标乱放的普通窗口。
这个检查让那种包打不出来。
"""

import os
import plistlib
import subprocess
import sys
import tempfile

from ds_store import DSStore

APP = "PonteX One.app"
WANT_ICONS = {APP: (165, 200), "Applications": (495, 200)}


def fail(message: str) -> None:
    print(f"✗ DMG 检查失败：{message}")
    sys.exit(1)


def main(dmg: str) -> None:
    mount = tempfile.mkdtemp(prefix="pontex-dmg-")
    subprocess.run(
        ["hdiutil", "attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint", mount, dmg],
        check=True,
        stdout=subprocess.DEVNULL,
    )
    try:
        check(mount)
    finally:
        subprocess.run(["hdiutil", "detach", "-quiet", mount], check=False)
    print("✓ DMG 拖拽安装窗口：.DS_Store、背景、图标位置、窗口设置都对")


def check(mount: str) -> None:
    names = os.listdir(mount)
    for need in (".DS_Store", APP, "Applications", ".VolumeIcon.icns"):
        if need not in names:
            fail(f"缺少 {need}（现有：{', '.join(sorted(names))}）")
    if os.readlink(os.path.join(mount, "Applications")) != "/Applications":
        fail("Applications 不是指向 /Applications 的快捷方式")
    # 背景在 .background/background.tiff（finish.py 挪过去的；根目录的点开头文件新版访达不认）
    folder = os.path.join(mount, ".background")
    has_background = os.path.isfile(os.path.join(folder, "background.tiff"))
    if not has_background:
        fail("没有背景图 .background/background.tiff")

    with DSStore.open(os.path.join(mount, ".DS_Store"), "r") as store:
        for name, (x, y) in WANT_ICONS.items():
            try:
                got = tuple(store[name]["Iloc"])
            except KeyError:
                fail(f"{name} 没有位置（Iloc）")
            if got[:2] != (x, y):
                fail(f"{name} 的位置是 {got[:2]}，应该是 {(x, y)}")

        bwsp = store["."]["bwsp"]
        if isinstance(bwsp, (bytes, bytearray)):
            bwsp = plistlib.loads(bwsp)
        for key in ("ShowToolbar", "ShowSidebar", "ShowStatusBar", "ShowPathbar"):
            if bwsp.get(key):
                fail(f"窗口的 {key} 没关")
        bounds = bwsp.get("WindowBounds", "")
        if "{660, 400}" not in bounds:
            fail(f"窗口大小不对：{bounds}")

        icvp = store["."]["icvp"]
        if isinstance(icvp, (bytes, bytearray)):
            icvp = plistlib.loads(icvp)
        if icvp.get("backgroundType") != 2:
            fail(f"窗口没有用图片背景（backgroundType={icvp.get('backgroundType')}）")
        if not icvp.get("backgroundImageAlias"):
            fail("背景图没有挂上（缺 backgroundImageAlias）")
        try:
            store["."]["pBB0"]
        except KeyError:
            fail("缺少新版访达认的背景书签（pBB0），新版 macOS 不会显示背景")
        if int(icvp.get("iconSize", 0)) != 128:
            fail(f"图标大小是 {icvp.get('iconSize')}，应该是 128")
        if icvp.get("arrangeBy") not in (None, "none"):
            fail(f"图标会被自动排列（arrangeBy={icvp.get('arrangeBy')}）")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(__doc__)
        sys.exit(2)
    main(sys.argv[1])

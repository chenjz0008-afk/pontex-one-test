"""让访达自己给 DMG 设背景，再压缩成最终的 DMG。

    python finish.py <dmgbuild 打出的可写 DMG> <输出.dmg>

为什么：dmgbuild 写的背景引用（旧格式 alias）新版访达不认。对照实验（同一台 macOS 15 测试机）：
dmgbuild 的 DMG 不管 TIFF 还是 PNG、补不补 pBB0 书签都不显示背景；由访达自己设一次（create-dmg 的做法），
背景正常，和成熟软件（Rectangle）一样。图标位置、窗口大小仍由 dmgbuild 写，访达只设背景。
需要能控制访达（CI 的 macOS 机器可以；本机第一次会请你允许「终端」控制「访达」）。
"""

import os
import subprocess
import sys


def run(*args: str) -> str:
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def main(source: str, out: str) -> None:
    # 要让访达看见这个盘：不能用 -nobrowse
    attached = run("hdiutil", "attach", "-readwrite", "-noautoopen", source)
    mount = attached.strip().splitlines()[-1].split("\t")[-1]
    disk = os.path.basename(mount)
    try:
        # 背景放进隐藏的 .background/ 文件夹（访达自己也这样放）
        found = [n for n in os.listdir(mount) if n.startswith(".background.")]
        if len(found) != 1:
            sys.exit(f"✗ 可写 DMG 里的背景图不对：{found}")
        os.makedirs(os.path.join(mount, ".background"), exist_ok=True)
        name = "background" + os.path.splitext(found[0])[1]
        os.replace(os.path.join(mount, found[0]), os.path.join(mount, ".background", name))

        script = f'''
            tell application "Finder"
              tell disk "{disk}"
                open
                set theViewOptions to the icon view options of container window
                set background picture of theViewOptions to file ".background:{name}"
                update without registering applications
                delay 3
                close
              end tell
            end tell'''
        run("osascript", "-e", script)
        # 访达改写后会把「使用自定义图标」的标记丢掉：补回来，卷图标才显示 PonteX One
        run("SetFile", "-a", "C", mount)
        run("sync")
    finally:
        run("hdiutil", "detach", mount)
    if os.path.exists(out):
        os.remove(out)
    run("hdiutil", "convert", source, "-format", "ULMO", "-o", out)
    print(f"✓ 访达设好背景并压缩：{out}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])

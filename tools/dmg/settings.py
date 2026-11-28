# dmgbuild 的设置：PonteX One 的拖拽安装窗口。
# 不经过访达、不用 AppleScript，直接写出 .DS_Store，在 CI 的无界面机器上结果也一样。
#   dmgbuild -s tools/dmg/settings.py -D app=<PonteX One.app> -D icon=<icon.icns> "PonteX One" out.dmg
# 背景：tools/dmg/background.html 渲染 1x / 2x 两张 PNG（72 / 144 dpi），用 tiffutil -cathidpicheck 合成
# src-tauri/pontex/icons/dmg.tiff。图标中心在 (165, 200) 和 (495, 200)，和背景上的箭头对齐。
import os.path

app = defines["app"]  # noqa: F821  dmgbuild 注入
name = os.path.basename(app)
here = os.path.dirname(os.path.abspath(__file__)) if "__file__" in globals() else os.getcwd()

format = "ULMO"  # LZMA：最小；macOS 10.15+ 都能打开（我们要求 12+）
filesystem = "HFS+"
size = None
files = [app]
symlinks = {"Applications": "/Applications"}
hide_extensions = [name]
icon = defines.get("icon")  # noqa: F821  卷图标
background = defines["background"]  # noqa: F821

# 窗口：和背景一样大；不要工具栏、侧边栏、状态栏、路径栏、标签栏
window_rect = ((200, 120), (660, 400))
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
sidebar_width = 0
default_view = "icon-view"
show_icon_preview = False

# 图标视图：128 像素，文字在下面，不自动排列
arrange_by = None
grid_offset = (0, 0)
grid_spacing = 100
scroll_position = (0, 0)
label_pos = "bottom"
text_size = 13
icon_size = 128
icon_locations = {name: (165, 200), "Applications": (495, 200)}

import type { Agent, Outcome, Place, Status } from "./api";

const zh = {
  settings: "设置",
  back: "返回",
  done: "完成",
  cancel: "取消",
  close: "关闭",
  retry: "重试",
  checking: "正在检查…",
  // 连接
  connectTitle: "输入 PonteX Key",
  connectLead: "验证后，为这台电脑上的 Agent 接入 PonteX。",
  keyLabel: "API Key",
  keyHelp: "还没有 Key？查看创建方法",
  next: "下一步",
  verifying: "正在验证…",
  keyStored: "Key 只发往 api.ponte-x.com，并写进所选 Agent 的本机配置，以便关闭本软件后继续使用。",
  planTitle: "选择要接入的 Agent",
  planLead: "可以随时切回原来的配置。",
  planNow: "现在：",
  planNowNative: "原生",
  planNowPontex: "已是 PonteX",
  planNowOther: (name: string) => `${name} → PonteX，可随时切回`,
  planNowOtherKept: (name: string) => name,
  planNowMissing: "未安装，装好后自动接入",
  planNowBroken: "配置文件有语法错误，不会改动",
  planNowWb: "添加 PonteX 模型，不改其他模型",
  planVerifyNote: "接入后会用每个 Agent 的主模型各发一次最小请求来确认可用，产生极少量用量。",
  apply: "接入",
  applying: "正在接入…",
  resultTitle: "接入结果",
  changeKey: "换一把 Key",
  quotaNotice: "这把 Key 有效，但现在没有可用额度。可以先配置，额度恢复后再验证。",
  rotatedNotice: (at: string) => `这是轮换前的旧 Key，将于 ${at} 失效。`,
  // 首页
  homeTitle: "我的 Agent",
  newSession: "切换 PonteX / 原生后，要把正在运行的 Agent 彻底关闭，再重新打开才生效。",
  notConnectedTitle: "还没有连接 PonteX",
  connect: "连接 PonteX",
  native: "原生",
  other: "其他",
  model: "主模型",
  recheck: "重新检测",
  version: "版本",
  location: "配置文件",
  lastVerify: "上次验证",
  verifyOk: "成功",
  requestId: "请求编号",
  envNotes: "环境变量",
  envBlocking: "会挡住 PonteX 配置，请从所在文件中移除",
  envHarmless: "不影响 PonteX，切回原生后可能生效",
  verifyNow: "验证",
  restoreOne: "还原",
  restoreTitle: (name: string) => `还原 ${name}`,
  restoreConfirm: (name: string) => `删除 ${name} 里所有 PonteX 相关配置，回到接入前的样子。会话记录不受影响。`,
  brokenHint: (path: string) => `${path} 运行不了。可以用下面这行命令从镜像重装，装好后会自动接入 PonteX。`,
  mirrorCommand: "镜像安装命令",
  copy: "复制",
  copied: "已复制",
  dismiss: "关闭",
  officialOffline: "Google 已于 2026-06-18 停止为个人账号提供 Gemini CLI 服务，原生需要付费 Key 或企业授权。",
  lockedSessions: (n: number) => `Codex 会把切换前的 ${n} 个会话设为只读，切回去后恢复。`,
  coexist: "检测到 CC Switch。两个软件管理同一份 Agent 配置，以最后一次操作为准。",
  gotIt: "知道了",
  // 设置
  keySection: "PonteX Key",
  generalSection: "通用",
  language: "语言",
  languageAuto: "跟随系统",
  autostart: "开机自动启动",
  closeToTray: "关闭窗口后留在菜单栏 / 托盘",
  helpSection: "帮助",
  diagnose: "导出诊断信息",
  diagnoseHint: "不含 Key 和令牌，保存后会在文件夹中显示。",
  diagnosed: (path: string) => `诊断信息已保存到 ${path}`,
  openData: "打开数据目录",
  docs: "帮助文档",
  aboutSection: "关于",
  about: (v: string, e: string) => `PonteX One ${v} · 基于 CC Switch ${e}`,
  license: "CC Switch 以 MIT 许可证发布，版权归其作者所有。",
  disconnectSection: "断开",
  disconnect: "断开并还原",
  disconnectHint: "所有 Agent 回到接入前的配置，并从本机删除 Key。卸载软件前请先断开。",
  disconnectConfirm: "断开后，五个 Agent 都会回到接入前的配置，Key 会从这台电脑上删除。",
  disconnecting: "正在还原…",
  rotateHint: "只更新正在使用 PonteX 的 Agent，其余不变。",
  update: "更新 Key",
  rotated: "Key 已更新",
  rotatedBody: (names: string) => `Key 已更新，${names} 已换成新 Key。把正在运行的 Agent 彻底关闭，再重新打开才生效。`,
  // 技能
  skillTitle: "安装技能",
  skillLoading: "正在获取技能信息…",
  skillFetching: (v: string) => `来自 PonteX Skills · 版本 ${v}，正在下载并校验…`,
  skillSection: "技能",
  skillEmpty: "还没有安装技能",
  skillEmptyHint: "在 PonteX Skills 网站上点「安装」，会回到这里确认。",
  skillBrowse: "浏览技能",
  skillManage: "管理已装技能",
  skillUsage: "把正在运行的 Agent 彻底关闭、再重新打开后生效。可以在「AI 工具箱 → 技能」里启用或停用。",
  skillPlace: (label: string, v: string) => `${label} · 版本 ${v}`,
  skillEdited: "你改过",
  skillRemoveTitle: (title: string) => `移除「${title}」`,
  skillRemoveConfirm: (agents: string) => `从 ${agents} 里移除这个技能。你改过的内容会先备份到数据目录。`,
  skillRemoved: (title: string) => `已移除「${title}」`,
  skillFrom: (v: string, n: number) => `来自 PonteX Skills · 版本 ${v} · ${n} 个文件`,
  skillScripts: (n: number) => `包含 ${n} 个脚本文件，由 Agent 在你同意后运行`,
  skillUnverified: "该技能尚未在此 Agent 上验证",
  skillNotInstalled: "未安装",
  skillInstall: (n: number) => `安装到 ${n} 个 Agent`,
  skillPick: "请选择 Agent",
  skillDone: "安装完成",
  skillDoneBody: (title: string, v: string) => `「${title}」（版本 ${v}）已安装到：`,
  skillFailed: "有 Agent 没有装上",
  skillBackup: "你改过的旧版本已备份到数据目录。",
  skillRemove: "移除",
  installing: "正在安装…",
  upgrade: (v: string) => `更新到 ${v}`,
  installAll: (n: number) => `安装全部（${n}）`,
  install: "安装",
  officialSite: "官网安装链接",
  installTitle: (names: string) => `安装 ${names}`,
  installPreparing: "正在查询最新版本…",
  installFrom: "命令行版。优先从国内镜像下载官方发行包，验证官方签名后安装。",
  planFailed: "暂时查不到版本，这次先跳过",
  planOffline: "现在连不上下载源，请稍后再试。",
  planMirror: "也可以复制下面的命令，在终端里从镜像安装。装好后自动接入 PonteX。",
  installSize: (mb: string) => `约占 ${mb}`,
  installWhere: "安装位置",
  installPathReady: "终端里可以直接使用",
  installPathProfile: (file: string) => `会在 ${file} 末尾加一段，让新终端找得到命令`,
  installPathUser: "会加入当前用户的 Path",
  installSafe: "不需要管理员权限，不改动已有的安装；已装的同名命令始终优先。",
  installNode: "Gemini CLI 需要 Node，会附带一份只给它用的。",
  installThen: "装好后自动接入 PonteX。",
  installStart: "开始安装",
  installed: (names: string, commands: string) => `${names} 装好了。新开一个终端，输入 ${commands} 就能用。`,
  downloadingNode: "正在下载 Node",
  downloadingRuntime: "正在下载微软 VC++ 运行库",
  retryIn: (s: number) => `下载源暂时不可用，${s} 秒后自动重试`,
  appChoice: "Codex 装哪个版本",
  appOption: "桌面版",
  appRecommended: "推荐",
  appDesc: (v: string, size: string) => `ChatGPT 桌面应用里的 Codex · ${v} · ${size}`,
  cliOption: "命令行",
  cliDesc: (v: string, size: string) => `在终端里使用 · ${v} · 约占 ${size}`,
  appShared: "两个版本读同一份配置，装哪个都会自动接入 PonteX。",
  appWhere: (dir: string) => `装到「应用程序」：${dir}`,
  appWhereWindows: "装成 Windows 应用（开始菜单里叫 ChatGPT）",
  appSafe: "官方源和镜像并行下载，核对 OpenAI 的官方签名后才安装；之后由它自己更新。",
  appAdmin: "安装时 Windows 会请您确认一次：Codex 自带一个后台安全服务，系统规定装服务要管理员同意。",
  appFailed: "桌面版暂时查不到，可以先装命令行版",
  appDone: "Codex 桌面版装好了，已接入 PonteX。",
  openApp: "打开 Codex",
  installApp: "安装桌面版",
  phases: {
    queued: "排在后面，前一个装完自动开始",
    resolving: "正在查询版本",
    downloading: "正在下载",
    verifying: "正在核对官方签名",
    authorizing: "请在 Windows 弹出的确认框里点「是」",
    unpacking: "正在解包",
    testing: "正在试运行",
    activating: "正在启用",
  },
  managed: "由 PonteX One 安装",
  uninstall: "移除",
  uninstallTitle: (name: string) => `移除 ${name}`,
  uninstallConfirm: (name: string) => `移除 PonteX One 安装的 ${name}？会先把它的配置还原到接入前，会话记录保留。`,
  fatalTitle: "PonteX One 没能启动",
  fatalSafe: "各个 Agent 里已经写好的配置不受影响，照常可以使用。",
  fatalRetry: "重试",
  fatalDetail: "技术信息",
  fatal: {
    DB_TOO_NEW: "本机数据由更新的版本创建，请下载最新版后再打开。数据没有被改动。",
    DB_LOCKED: "本机数据正被另一个程序占用。请关掉其他 PonteX One 窗口后重试。",
    DB_CORRUPT: "本机数据文件损坏，读不出来。请导出诊断信息发给我们，不要删除数据目录。",
    DB_FULL: "磁盘空间不足，本机数据写不进去。请腾出一些空间后重试。",
    DB_ACCESS: "本机数据文件打不开，可能是没有权限，或者磁盘空间不足。",
    STARTUP: "启动时遇到了问题。请重试，还不行就导出诊断信息发给我们。",
  } as Record<string, string>,
  download: "下载最新版",
  // v2 界面：Agent / AI 工具箱 / 设置
  tabAgents: "Agent",
  tabToolbox: "AI 工具箱",
  summary: (installed: number, using: number) => `${installed} 个 Agent 已安装 · ${using} 个已启用 PonteX`,
  restartNote: "切换 PonteX / 原生后，要把正在运行的 Agent 彻底关闭，再重新打开才生效。",
  switchedTo: (name: string, target: "pontex" | "native" | "previous") =>
    target === "pontex" ? `${name} 已切到 PonteX` : target === "native" ? `${name} 已切到原生` : `${name} 已切回原来的配置`,
  restartOne: (name: string) => `把正在运行的 ${name} 彻底关闭，再重新打开才生效`,
  restartAll: "把正在运行的 Agent 彻底关闭，再重新打开才生效",
  undo: "撤销",
  open: "打开",
  openWhere: (name: string) => `在哪个文件夹里打开 ${name}`,
  searchFolders: (n: number) => `搜索用过的 ${n} 个文件夹`,
  noFolderMatch: (q: string) => `没有找到「${q}」`,
  pickFolder: "选择文件夹…",
  newFolder: "新建文件夹…",
  forgetFolder: "从列表里移除（文件夹不动）",
  forgotFolder: (name: string) => `已从列表里移除「${name}」`,
  folderUntouched: "文件夹本身没动",
  firstLaunch: (name: string) => `会打开一个终端窗口，${name} 已在你选的文件夹里启动好，直接打字说要做什么就行。`,
  newFolderName: "新文件夹的名字",
  newFolderIn: "建在",
  newFolderInTail: "里",
  changeOnce: "换个位置",
  createAndOpen: "新建并打开",
  launched: (name: string, folder: string) => `已在「${folder}」里打开 ${name}`,
  launchedSub: "终端窗口在最前面，直接打字说要做什么就行",
  dirNames: { Documents: "文稿", Desktop: "桌面", Downloads: "下载" } as Record<string, string>,
  skillsTab: "技能",
  mcpTab: "MCP",
  enabledIn: "启用于",
  browseSkills: "浏览更多技能",
  addMcp: "添加 MCP",
  skillsNote: "图标亮着 = 已在这个 Agent 里启用。点一下就启用或停用；停用不会删掉，随时能再启用。",
  mcpNote: "图标亮着 = 已在这个 Agent 里启用。名字前的点：绿色能用，黄色要先登录，红色连不上。",
  importedNotice: (n: number) => `已读入各 Agent 里现有的 ${n} 个 MCP，原配置没动。`,
  noSkills: "还没有技能。在 PonteX Skills 网站上点「安装」，就会出现在这里。",
  noMcps: "还没有 MCP。点右上角「添加 MCP」，常用的点一下就能加。",
  noAgentsYet: "先在 Agent 页装一个 Agent，再来这里启用。",
  toggleTip: (agent: string, on: boolean) => `${agent} · ${on ? "已启用" : "未启用"}`,
  toggleFailed: (agent: string) => `启用到 ${agent} 失败`,
  health: { ok: "能用", login: "要先登录", failed: "连不上", checking: "正在检查" } as Record<string, string>,
  edit: "编辑",
  remove: "删除",
  mcpDeleted: (name: string) => `已删除 ${name}`,
  mcpDeletedSub: "已从各 Agent 的配置里移除",
  mcpEditTitle: (name: string) => `编辑 ${name}`,
  mcpTarget: "命令或网址",
  mcpTargetHint: "npx -y @scope/server　或　https://…　或　直接粘贴 README 里的 JSON",
  mcpName: "名称",
  mcpEnv: "环境变量",
  mcpHeaders: "请求头",
  mcpRecognized: (name: string, n: number, headers: boolean) => `认出来了：${name}${n ? `，带 ${n} 个${headers ? "请求头" : "环境变量"}` : ""}`,
  mcpJsonIncomplete: "JSON 还没贴完整",
  mcpNeedTarget: "填一个命令或网址",
  mcpStatus: "状态",
  save: "保存",
  add: "添加",
  mcpAdded: (name: string) => `已添加 ${name}`,
  mcpAddedSub: (agents: string) => (agents ? `已在 ${agents} 里启用` : "暂时没在任何 Agent 里启用"),
  mcpSaved: (name: string) => `已保存 ${name}`,
  presets: { fetch: "抓网页内容", context7: "查最新的库文档", playwright: "操作浏览器", memory: "跨会话记住事情", time: "时间与时区" } as Record<string, string>,
  keyEnabled: "已启用",
  keyChange: "更换",
  keyChangeTitle: "更换 API Key",
  keyPaste: "粘贴新的 Key，会自动检查能不能用",
  keyGet: "去控制台拿 Key",
  keyValid: "✓ 有效",
  keyWillApply: (names: string) => (names ? `保存后会启用到 ${names}` : "保存后，接入 PonteX 的 Agent 都用它"),
  keySaved: (n: number) => `已换成新的 Key，并更新到 ${n} 个 Agent`,
  foldersSection: "打开文件夹",
  foldersDefault: "新建的文件夹默认放在",
  foldersDefaultHint: (cloud: string) => `在你的个人文件夹里，不会被 ${cloud} 同步`,
  change: "更改…",
  openFolderToo: "同时打开这个文件夹",
  openFolderTooHint: (finder: string) => `先在${finder}里打开文件夹，再打开终端：Agent 建了什么文件，一眼就能看到`,
  terminal: "用哪个终端",
  terminalHint: "只列出你电脑上装了的",
  terminals: { terminal: "终端（系统自带）", iterm: "iTerm", wt: "Windows 终端", powershell: "PowerShell" } as Record<string, string>,
  finder: (win: boolean): string => (win ? "资源管理器" : "访达"),
  // PonteX One 自己的更新
  updateTitle: (v: string) => `新版本 ${v}`,
  updateLead: (current: string) => `你现在用的是 ${current}。这次更新了：`,
  updateNoNotes: "改进和修复。",
  updateSafe: "只更新 PonteX One 本身，各个 Agent 的配置不受影响。",
  updateOnQuit: "不点也没关系：退出 PonteX One 时会自动装好。",
  updateLater: "稍后",
  updateNow: "立即更新",
  updateInstalling: "正在更新…",
  updated: (v: string) => `已更新到 ${v}`,
  updateSection: "更新",
  updateCurrent: (v: string) => `PonteX One ${v}`,
  updateStatus: {
    idle: "",
    checking: "正在检查…",
    latest: "已是最新版",
    downloading: "正在下载新版本…",
    ready: "新版本已准备好",
    installing: "正在更新…",
    failed: "检查更新没成功，稍后会再试",
    unsupported: "这个版本不能自动更新，请到官网下载新版",
  } as Record<string, string>,
  updateCheck: "检查更新",
  updateReady: (v: string) => `${v} 已准备好`,
  updateRestart: "重启更新",
  updateView: "查看更新内容",
  autoUpdate: "自动更新",
  autoUpdateHint: "有新版本时在后台下好，准备好后提醒你",
  updateRequired: (v: string) => `需要更新到 ${v} 以上才能继续正常使用。`,
};

type Dict = typeof zh;

const en: Dict = {
  settings: "Settings",
  back: "Back",
  done: "Done",
  cancel: "Cancel",
  close: "Close",
  retry: "Retry",
  checking: "Checking…",
  connectTitle: "Enter your PonteX key",
  connectLead: "Once verified, the agents on this computer can use PonteX.",
  keyLabel: "API key",
  keyHelp: "No key yet? See how to create one",
  next: "Next",
  verifying: "Verifying…",
  keyStored: "Your key is sent only to api.ponte-x.com and saved in the local config of the agents you choose, so they keep working after this app closes.",
  planTitle: "Choose agents to connect",
  planLead: "You can go back to your previous setup at any time.",
  planNow: "Now: ",
  planNowNative: "Native",
  planNowPontex: "Already PonteX",
  planNowOther: (name) => `${name} → PonteX, switch back any time`,
  planNowOtherKept: (name) => name,
  planNowMissing: "Not installed, connects once installed",
  planNowBroken: "Config file has a syntax error and won't be touched",
  planNowWb: "Adds PonteX models, keeps the rest",
  planVerifyNote: "After connecting, one minimal request per agent confirms its main model works. This uses a tiny amount of credit.",
  apply: "Connect",
  applying: "Connecting…",
  resultTitle: "Result",
  changeKey: "Use another key",
  quotaNotice: "This key is valid but has no credit right now. You can set things up and verify later.",
  rotatedNotice: (at) => `This is the pre-rotation key and expires at ${at}.`,
  homeTitle: "My agents",
  newSession: "After switching, fully quit the running agent and open it again for the change to apply.",
  notConnectedTitle: "PonteX is not connected",
  connect: "Connect PonteX",
  native: "Native",
  other: "Other",
  model: "Main model",
  recheck: "Check again",
  version: "Version",
  location: "Config file",
  lastVerify: "Last verified",
  verifyOk: "OK",
  requestId: "Request ID",
  envNotes: "Environment variables",
  envBlocking: "Overrides the PonteX config; remove it from this file",
  envHarmless: "Doesn't affect PonteX, may apply in native mode",
  verifyNow: "Verify",
  restoreOne: "Restore",
  restoreTitle: (name) => `Restore ${name}`,
  restoreConfirm: (name) => `Removes every PonteX setting from ${name} and returns it to how it was before connecting. Sessions stay.`,
  brokenHint: (path) => `${path} won't run. Reinstall it from a mirror with the command below; it connects to PonteX once installed.`,
  mirrorCommand: "Mirror install command",
  copy: "Copy",
  copied: "Copied",
  dismiss: "Dismiss",
  officialOffline: "Google ended Gemini CLI for personal accounts on 2026-06-18. Native needs a paid key or an enterprise licence.",
  lockedSessions: (n) => `Codex makes ${n} earlier session(s) read-only until you switch back.`,
  coexist: "CC Switch found. Both apps manage the same agent config; the last action wins.",
  gotIt: "Got it",
  keySection: "PonteX key",
  generalSection: "General",
  language: "Language",
  languageAuto: "System",
  autostart: "Start at login",
  closeToTray: "Keep running in the menu bar / tray when closed",
  helpSection: "Help",
  diagnose: "Export diagnostics",
  diagnoseHint: "Contains no keys or tokens. Shown in its folder once saved.",
  diagnosed: (path) => `Diagnostics saved to ${path}`,
  openData: "Open data folder",
  docs: "Documentation",
  aboutSection: "About",
  about: (v, e) => `PonteX One ${v} · built on CC Switch ${e}`,
  license: "CC Switch is released under the MIT licence; copyright belongs to its authors.",
  disconnectSection: "Disconnect",
  disconnect: "Disconnect and restore",
  disconnectHint: "Every agent returns to its earlier setup and the key is removed. Do this before uninstalling.",
  disconnectConfirm: "All five agents return to the setup they had before connecting, and the key is removed from this computer.",
  disconnecting: "Restoring…",
  rotateHint: "Only agents using PonteX are updated.",
  update: "Update key",
  rotated: "Key updated",
  rotatedBody: (names) => `Key updated for ${names}. Fully quit running agents and open them again to apply.`,
  skillTitle: "Install skill",
  skillLoading: "Looking up the skill…",
  skillFetching: (v) => `From PonteX Skills · version ${v}, downloading and verifying…`,
  skillSection: "Skills",
  skillEmpty: "No skills installed yet",
  skillEmptyHint: "Choose Install on the PonteX Skills website to confirm it here.",
  skillBrowse: "Browse skills",
  skillManage: "Manage skills",
  skillUsage: "Open a new session to use it. View or remove it in Settings → Skills.",
  skillPlace: (label, v) => `${label} · version ${v}`,
  skillEdited: "edited by you",
  skillRemoveTitle: (title) => `Remove “${title}”`,
  skillRemoveConfirm: (agents) => `Removes this skill from ${agents}. Anything you edited is backed up to the data folder first.`,
  skillRemoved: (title) => `Removed “${title}”`,
  skillFrom: (v, n) => `From PonteX Skills · version ${v} · ${n} files`,
  skillScripts: (n) => `Includes ${n} script file(s), run by the agent with your approval`,
  skillUnverified: "Not verified on this agent",
  skillNotInstalled: "Not installed",
  skillInstall: (n) => `Install to ${n} agent${n === 1 ? "" : "s"}`,
  skillPick: "Choose an agent",
  skillDone: "Installed",
  skillDoneBody: (title, v) => `“${title}” (version ${v}) was installed to:`,
  skillFailed: "Some agents failed",
  skillBackup: "Your edited version was backed up to the data folder.",
  skillRemove: "Remove",
  installing: "Installing…",
  upgrade: (v) => `Update to ${v}`,
  installAll: (n) => `Install all (${n})`,
  install: "Install",
  officialSite: "Official site",
  installTitle: (names) => `Install ${names}`,
  installPreparing: "Looking up the latest version…",
  installFrom: "Command-line version. The official release, downloaded from a mirror first and installed after its signature is verified.",
  planFailed: "Version lookup failed, skipped this time",
  planOffline: "No download source is reachable right now. Try again later.",
  planMirror: "Or copy this command to install from a mirror in your terminal. It connects to PonteX once installed.",
  installSize: (mb) => `About ${mb}`,
  installWhere: "Location",
  installPathReady: "Already available in your terminal",
  installPathProfile: (file) => `Adds a block to the end of ${file} so new terminals can find it`,
  installPathUser: "Adds it to your user Path",
  installSafe: "No admin rights. Existing installs are untouched and always take precedence.",
  installNode: "Gemini CLI needs Node; a private copy is included.",
  installThen: "Connects to PonteX once installed.",
  installStart: "Install",
  installed: (names, commands) => `${names} installed. Open a new terminal and run ${commands}.`,
  downloadingNode: "Downloading Node",
  downloadingRuntime: "Downloading the Microsoft VC++ runtime",
  retryIn: (s) => `Download sources are unavailable. Retrying in ${s}s`,
  appChoice: "Which Codex",
  appOption: "Desktop app",
  appRecommended: "Recommended",
  appDesc: (v, size) => `Codex in the ChatGPT desktop app · ${v} · ${size}`,
  cliOption: "Command line",
  cliDesc: (v, size) => `Use it in a terminal · ${v} · about ${size}`,
  appShared: "Both read the same config, so either one connects to PonteX.",
  appWhere: (dir) => `Installs to Applications: ${dir}`,
  appWhereWindows: "Installs as a Windows app (ChatGPT in the Start menu)",
  appSafe: "Downloaded from the official source and mirrors in parallel; installed only after OpenAI's signature checks out. It updates itself afterwards.",
  appAdmin: "Windows will ask you to confirm once: Codex includes a background security service, and installing a service needs admin approval.",
  appFailed: "The desktop app can't be looked up right now. You can install the command line first.",
  appDone: "The Codex desktop app is installed and connected to PonteX.",
  openApp: "Open Codex",
  installApp: "Install the desktop app",
  phases: {
    queued: "Next in line, starts by itself",
    resolving: "Looking up version",
    downloading: "Downloading",
    verifying: "Checking the official signature",
    authorizing: "Click Yes in the Windows prompt",
    unpacking: "Unpacking",
    testing: "Trying it out",
    activating: "Enabling",
  },
  managed: "Installed by PonteX One",
  uninstall: "Remove",
  uninstallTitle: (name) => `Remove ${name}`,
  uninstallConfirm: (name) => `Remove the ${name} installed by PonteX One? Its config is restored first; sessions stay.`,
  fatalTitle: "PonteX One couldn't start",
  fatalSafe: "Your agents keep working with the config they already have.",
  fatalRetry: "Try again",
  fatalDetail: "Technical details",
  fatal: {
    DB_TOO_NEW: "This data was created by a newer version. Download the latest PonteX One to open it. Nothing was changed.",
    DB_LOCKED: "Another program is using the local data. Close other PonteX One windows and try again.",
    DB_CORRUPT: "The local data file is damaged. Export diagnostics and send them to us; don't delete the data folder.",
    DB_FULL: "The disk is full, so local data can't be written. Free some space and try again.",
    DB_ACCESS: "The local data file can't be opened. It may lack permission, or the disk is full.",
    STARTUP: "Something went wrong while starting. Try again, or export diagnostics and send them to us.",
  },
  download: "Download latest",
  tabAgents: "Agents",
  tabToolbox: "AI Toolbox",
  summary: (installed, using) => `${installed} agent${installed === 1 ? "" : "s"} installed · ${using} using PonteX`,
  restartNote: "After switching between PonteX and native, fully quit the running agent and open it again for the change to apply.",
  switchedTo: (name, target) =>
    target === "pontex" ? `${name} now uses PonteX` : target === "native" ? `${name} now uses native` : `${name} is back to its previous setup`,
  restartOne: (name) => `Fully quit the running ${name} and open it again to apply`,
  restartAll: "Fully quit running agents and open them again to apply",
  undo: "Undo",
  open: "Open",
  openWhere: (name) => `Open ${name} in a folder`,
  searchFolders: (n) => `Search ${n} recent folders`,
  noFolderMatch: (q) => `Nothing matches “${q}”`,
  pickFolder: "Choose folder…",
  newFolder: "New folder…",
  forgetFolder: "Remove from list (the folder stays)",
  forgotFolder: (name) => `Removed “${name}” from the list`,
  folderUntouched: "The folder itself is untouched",
  firstLaunch: (name) => `A terminal window opens with ${name} already running in the folder you pick. Just type what you want done.`,
  newFolderName: "Name of the new folder",
  newFolderIn: "Created in",
  newFolderInTail: "",
  changeOnce: "Change location",
  createAndOpen: "Create and open",
  launched: (name, folder) => `Opened ${name} in “${folder}”`,
  launchedSub: "The terminal window is in front — just type what you want done",
  dirNames: { Documents: "Documents", Desktop: "Desktop", Downloads: "Downloads" },
  skillsTab: "Skills",
  mcpTab: "MCP",
  enabledIn: "Enabled in",
  browseSkills: "Browse skills",
  addMcp: "Add MCP",
  skillsNote: "A lit icon means the skill is enabled in that agent. Click to enable or disable; disabling never deletes it.",
  mcpNote: "A lit icon means the server is enabled in that agent. Dot before the name: green works, yellow needs sign-in, red can't connect.",
  importedNotice: (n) => `Read ${n} existing MCP server${n === 1 ? "" : "s"} from your agents. Their configs were not changed.`,
  noSkills: "No skills yet. Click “Install” on the PonteX Skills site and they show up here.",
  noMcps: "No MCP servers yet. Click “Add MCP” — common ones are one click away.",
  noAgentsYet: "Install an agent on the Agents tab first, then enable things here.",
  toggleTip: (agent, on) => `${agent} · ${on ? "enabled" : "not enabled"}`,
  toggleFailed: (agent) => `Couldn't enable in ${agent}`,
  health: { ok: "Works", login: "Needs sign-in", failed: "Can't connect", checking: "Checking" },
  edit: "Edit",
  remove: "Delete",
  mcpDeleted: (name) => `Deleted ${name}`,
  mcpDeletedSub: "Removed from every agent's config",
  mcpEditTitle: (name) => `Edit ${name}`,
  mcpTarget: "Command or URL",
  mcpTargetHint: "npx -y @scope/server, https://…, or paste the JSON from a README",
  mcpName: "Name",
  mcpEnv: "Environment variables",
  mcpHeaders: "Headers",
  mcpRecognized: (name, n, headers) => `Recognized ${name}${n ? ` with ${n} ${headers ? "header" : "variable"}${n === 1 ? "" : "s"}` : ""}`,
  mcpJsonIncomplete: "The JSON isn't complete yet",
  mcpNeedTarget: "Enter a command or URL",
  mcpStatus: "Status",
  save: "Save",
  add: "Add",
  mcpAdded: (name) => `Added ${name}`,
  mcpAddedSub: (agents) => (agents ? `Enabled in ${agents}` : "Not enabled in any agent yet"),
  mcpSaved: (name) => `Saved ${name}`,
  presets: { fetch: "Fetch web pages", context7: "Up-to-date library docs", playwright: "Drive a browser", memory: "Remember across sessions", time: "Time and time zones" },
  keyEnabled: "Enabled",
  keyChange: "Change",
  keyChangeTitle: "Change API key",
  keyPaste: "Paste the new key — it's checked automatically",
  keyGet: "Get a key in the console",
  keyValid: "✓ Valid",
  keyWillApply: (names) => (names ? `Saving applies it to ${names}` : "Agents connected to PonteX will use it"),
  keySaved: (n) => `Switched to the new key and updated ${n} agent${n === 1 ? "" : "s"}`,
  foldersSection: "Opening folders",
  foldersDefault: "New folders go in",
  foldersDefaultHint: (cloud) => `In your home folder, not synced by ${cloud}`,
  change: "Change…",
  openFolderToo: "Also open the folder",
  openFolderTooHint: (finder) => `Shows the folder in ${finder} first, then opens the terminal, so you can see what the agent creates`,
  terminal: "Terminal",
  terminalHint: "Only the ones installed on this computer",
  terminals: { terminal: "Terminal (built in)", iterm: "iTerm", wt: "Windows Terminal", powershell: "PowerShell" },
  finder: (win) => (win ? "File Explorer" : "Finder"),
  updateTitle: (v) => `Version ${v} is here`,
  updateLead: (current) => `You're on ${current}. What's new:`,
  updateNoNotes: "Improvements and fixes.",
  updateSafe: "Only PonteX One itself is updated; your agents' settings stay as they are.",
  updateOnQuit: "No rush: it installs automatically when you quit PonteX One.",
  updateLater: "Later",
  updateNow: "Update now",
  updateInstalling: "Updating…",
  updated: (v) => `Updated to ${v}`,
  updateSection: "Updates",
  updateCurrent: (v) => `PonteX One ${v}`,
  updateStatus: {
    idle: "",
    checking: "Checking…",
    latest: "You're up to date",
    downloading: "Downloading the new version…",
    ready: "A new version is ready",
    installing: "Updating…",
    failed: "Couldn't check for updates; will try again later",
    unsupported: "This build can't update itself. Download the latest from the website",
  },
  updateCheck: "Check for updates",
  updateReady: (v) => `${v} is ready`,
  updateRestart: "Restart to update",
  updateView: "What's new",
  autoUpdate: "Automatic updates",
  autoUpdateHint: "Downloads new versions in the background and tells you when they're ready",
  updateRequired: (v) => `Please update to ${v} or later to keep things working.`,
};

export type T = Dict & { lang: "zh" | "en" };

export function pickLanguage(pref: string | null | undefined): T {
  const lang = pref ?? navigator.language;
  return lang.toLowerCase().startsWith("zh")
    ? { ...zh, lang: "zh" }
    : { ...en, lang: "en" };
}

/** 原因码 → 一句话。没有收录的原样给出，方便反馈。 */
const REASONS: Record<string, [string, string]> = {
  KEY_FORMAT: ["请输入 px- 开头的 API Key。", "Enter an API key that starts with px-."],
  FOLDER_NAME_EMPTY: ["给文件夹起个名字。", "Give the folder a name."],
  FOLDER_NAME_CHARS: ["名字里不能有 \\ / : * ? \" < > | 这些符号。", "The name can't contain \\ / : * ? \" < > |."],
  FOLDER_NAME_DOT: ["名字不能以 . 开头（会变成隐藏文件夹）。", "The name can't start with a dot (it would be hidden)."],
  FOLDER_NAME_LONG: ["名字太长了，短一点。", "That name is too long."],
  FOLDER_EXISTS: ["这里已经有同名的文件夹了，换个名字，或者返回直接打开它。", "A folder with that name is already there. Pick another name, or go back and open it."],
  FOLDER_MISSING: ["这个文件夹不在了，可能被删掉或移走了。", "That folder is gone — it may have been deleted or moved."],
  NOT_A_CLI: ["这个 Agent 不在终端里用。", "This agent doesn't run in a terminal."],
  SKILL_MISSING: ["找不到这个技能的文件了。", "This skill's files are missing."],
  MCP_NAME_EMPTY: ["给它起个名字。", "Give it a name."],
  UPDATE_BUSY_INSTALLING: ["正在安装 Agent，装完再更新。", "An agent is being installed. Update once it's done."],
  UPDATE_BUSY_SWITCHING: ["正在切换，稍等几秒再更新。", "A switch is in progress. Try again in a few seconds."],
  UPDATE_NOT_READY: ["新版本还没下好，稍等一下。", "The new version isn't downloaded yet."],
  PROVISIONING_KEY_NOT_FOR_INFERENCE: ["这是管理密钥（pxp-），请改用 px- 开头的 API Key。", "This is a provisioning key (pxp-). Use an API key that starts with px-."],
  API_KEY_REQUIRED: ["请输入 Key。", "Enter a key."],
  INVALID_API_KEY: ["Key 不正确，或轮换后的旧 Key 已过宽限期。", "The key is wrong, or it was rotated and has expired."],
  API_KEY_REVOKED: ["这把 Key 已被吊销。", "This key has been revoked."],
  API_KEY_EXPIRED: ["这把 Key 已过期。", "This key has expired."],
  IP_NOT_ALLOWED: ["当前网络不在这把 Key 的 IP 白名单里。", "This network isn't on the key's IP allowlist."],
  ORG_SUSPENDED: ["企业已被停用，请联系 PonteX。", "Your organisation is suspended. Contact PonteX."],
  INSUFFICIENT_BALANCE: ["企业余额不足，请充值后重试。", "Your organisation is out of credit."],
  WALLET_FROZEN: ["企业钱包已冻结，请联系 PonteX。", "Your organisation's wallet is frozen. Contact PonteX."],
  MEMBER_QUOTA_EXCEEDED: ["成员额度已用完，请联系管理员。", "The member quota is used up. Ask your admin."],
  PERIOD_LIMIT_EXCEEDED: ["这把 Key 本周期额度已用完。", "This key's quota for the period is used up."],
  MODEL_NOT_FOUND: ["模型不存在或未上架。", "The model doesn't exist."],
  MODEL_NOT_ALLOWED: ["这把 Key 不能使用这个模型。", "This key can't use this model."],
  MODEL_NOT_AVAILABLE: ["这个模型现在不能从这个 Agent 的协议调用。", "This model isn't available over this agent's protocol."],
  FIELD_UNSUPPORTED: ["网关不支持请求里的某个字段。", "The gateway rejected a field in the request."],
  NO_MODELS: ["这把 Key 没有可用的模型。", "This key has no usable models."],
  NETWORK: ["连不上 PonteX，请检查网络或代理。", "Can't reach PonteX. Check your network or proxy."],
  TIMEOUT: ["PonteX 响应超时，请稍后重试。", "PonteX timed out. Try again."],
  BAD_RESPONSE: ["返回内容无法识别，网络可能被代理拦截。", "Unexpected response; a proxy may be interfering."],
  NOT_INSTALLED: ["这个 Agent 还没有安装。", "This agent isn't installed."],
  NOT_CONNECTED: ["请先连接 PonteX。", "Connect PonteX first."],
  NOT_EFFECTIVE: ["写入后没有生效，可能同时有其他程序在修改配置。", "The change didn't take effect; another program may be editing the config."],
  NO_PREVIOUS: ["没有可以恢复的原配置。", "There is no previous setup to restore."],
  WORKBUDDY_MODELS_TAKEN: ["WorkBuddy 里已有同名模型，没有可以添加的。", "WorkBuddy already has models with these names."],
  INSTALL_OFFLINE: ["所有下载源都连不上，请检查网络或代理。已下载的部分会保留，重试时接着下。", "No download source is reachable. What was downloaded is kept for the next try."],
  INSTALL_MISSING: ["各个源都还没有这个版本，请稍后再试。", "No source has this version yet. Try again later."],
  INSTALL_UNTRUSTED: ["没有取到通过官方签名验证的版本，没有安装。", "No version passed signature verification. Nothing was installed."],
  INSTALL_CHECKSUM: ["下载内容和官方签名对不上，没有安装。", "The download doesn't match its official signature. Nothing was installed."],
  INSTALL_DISK: ["磁盘空间不足，或数据目录不可写。", "Not enough disk space, or the data folder isn't writable."],
  INSTALL_ARCHIVE: ["安装包内容不符合预期，没有安装。", "The package isn't what was expected. Nothing was installed."],
  INSTALL_RUN: ["下载正确，但在这台电脑上运行不了，没有启用。", "Downloaded correctly but it won't run on this computer. Not enabled."],
  INSTALL_CANCELLED: ["已取消。已下载的部分会保留，重试时接着下。", "Cancelled. What was downloaded is kept for the next try."],
  INSTALL_UNSUPPORTED: ["这台电脑的系统暂不支持自动安装。", "Automatic install isn't supported on this system."],
  INSTALL_BUSY: ["正在安装，装完再移除。", "Still installing. Remove it after that finishes."],
  INSTALL_CONFLICT: ["几个下载源给出的同一版本对不上，为安全起见没有安装，请稍后再试。", "Sources disagree about the same version. Nothing was installed for safety; try again later."],
  INSTALL_OLD_OS: ["这台电脑的系统版本太旧，装不了 Codex 桌面版（macOS 需 13 以上，Windows 需 10 的 2004 版以上），可以改装命令行版。", "This system is too old for the Codex desktop app (macOS 13+ or Windows 10 2004+). The command line works."],
  INSTALL_APP_TAKEN: ["「应用程序」里已经有一个叫 ChatGPT 的其他应用，为免覆盖没有安装。请先把它移走或改名。", "Another app named ChatGPT is already in Applications. It wasn't overwritten; move or rename it first."],
  INSTALL_DENIED: ["Windows 的确认框里没有点「是」，没有安装。已下载的内容会保留。", "The Windows prompt wasn't approved, so nothing was installed."],
  INSTALL_DEPENDENCY: ["这台电脑缺少它需要的系统组件，没有启用。请导出诊断信息发给我们。", "This computer is missing a system component it needs. Not enabled; please send us a diagnostic report."],
  INSTALL_EXISTS: ["不是由 PonteX One 安装的，没有改动。", "Not installed by PonteX One. Nothing was changed."],
  SKILL_NOT_FOUND: ["没有找到这个技能。", "Skill not found."],
  SKILL_BAD_LINK: ["安装链接不正确。", "The install link is invalid."],
  SKILL_BAD_MANIFEST: ["技能清单不正确，没有安装。", "The skill manifest is invalid. Nothing was installed."],
  SKILL_BAD_ZIP: ["技能包含有不安全的内容，没有安装。", "The package contains unsafe entries. Nothing was installed."],
  SKILL_CHECKSUM: ["技能包校验不通过，没有安装。", "The package failed verification. Nothing was installed."],
  SKILL_TOO_LARGE: ["技能包过大，没有安装。", "The package is too large. Nothing was installed."],
  SKILL_SITE_ERROR: ["PonteX Skills 暂时无法访问。", "PonteX Skills is unavailable."],
  SKILL_LINKED: ["这个位置由其他工具管理，没有改动。", "Another tool manages this location. Nothing was changed."],
  SKILL_NOT_READY: ["请重新从网站打开安装链接。", "Open the install link from the website again."],
  SKILL_NOT_OURS: ["不是由 PonteX One 安装的，没有改动。", "Not installed by PonteX One. Nothing was changed."],
};

export function reason(t: T, code: string | null | undefined): string {
  if (!code) return "";
  if (code.startsWith("UNREADABLE:")) {
    const file = code.slice("UNREADABLE:".length);
    return t.lang === "zh"
      ? `配置文件有语法错误，没有改动：${file}`
      : `The config file has a syntax error and was left alone: ${file}`;
  }
  const known = REASONS[code];
  if (known) return known[t.lang === "zh" ? 0 : 1];
  const tail = (prefix: string) => code.slice(prefix.length).replace(/^:\s*/, "");
  if (code.startsWith("TERMINAL_FAILED"))
    return t.lang === "zh" ? `终端没能打开：${tail("TERMINAL_FAILED")}` : `Couldn't open the terminal: ${tail("TERMINAL_FAILED")}`;
  if (code.startsWith("FOLDER_CREATE"))
    return t.lang === "zh" ? `文件夹没能建好：${tail("FOLDER_CREATE")}` : `Couldn't create the folder: ${tail("FOLDER_CREATE")}`;
  const http = /^HTTP_(\d+)$/.exec(code);
  if (http) return t.lang === "zh" ? `请求失败（${http[1]}）` : `Request failed (${http[1]})`;
  return code;
}

export function statusText(t: T, agent: Agent): string {
  const { code, detail } = agent.status as Status;
  const z = t.lang === "zh";
  switch (code) {
    case "checking":
      return t.checking;
    case "not-installed":
      return z ? "未安装" : "Not installed";
    case "waiting":
      return z ? "未安装 · 装好后自动接入" : "Not installed · connects once installed";
    case "broken":
      return z ? "已安装，但无法运行" : "Installed, but won't run";
    case "unreadable":
      return z ? "配置文件有语法错误，未改动" : "Config file has a syntax error, left alone";
    case "failed":
      return reason(t, detail);
    case "overridden":
      return z ? `被环境变量 ${detail} 覆盖` : `Overridden by ${detail}`;
    case "drifted":
      return z ? "已被其他程序改回原生" : "Changed back to native by another program";
    case "other-key":
      return z ? `使用另一把 PonteX Key（${detail ?? ""}…）` : `Using another PonteX key (${detail ?? ""}…)`;
    case "other":
      return z ? `正在使用 ${detail}` : `Using ${detail}`;
    case "verified":
      return z ? "已启用 PonteX" : "Using PonteX";
    case "written":
      return z ? "已启用 PonteX · 还没验证" : "Using PonteX · not verified yet";
    case "verify-failed":
      return `${z ? "已启用 PonteX · " : "Using PonteX · "}${reason(t, detail)}`;
    case "native":
      return detail ? `${z ? "原生" : "Native"} · ${detail}` : z ? "原生" : "Native";
    case "added":
      return z ? "已添加 PonteX 模型 · 在 WorkBuddy 里选用" : "PonteX models added · pick them in WorkBuddy";
    case "not-added":
      return z ? "未添加 PonteX 模型" : "PonteX models not added";
  }
}

export function actionText(t: T, agent: Agent): string {
  const z = t.lang === "zh";
  switch (agent.status.action) {
    case "install":
      if (agent.id === "workbuddy") return t.officialSite;
      return agent.mirror ? t.mirrorCommand : "";
    case "verify":
      return z ? "验证" : "Verify";
    case "apply":
      if (agent.status.code === "drifted" || agent.status.code === "other-key")
        return z ? "重新接入" : "Connect again";
      if (agent.status.code === "failed") return t.retry;
      return z ? "接入" : "Connect";
    case "open-app":
      return z ? "打开 WorkBuddy" : "Open WorkBuddy";
    case "show-detail":
      return z ? "查看" : "View";
    default:
      return "";
  }
}

/** 按软件里选的语言显示时间，不跟系统走。 */
export function timeText(t: T, at: number | string): string {
  const date = new Date(typeof at === "number" ? at * 1000 : at);
  return date.toLocaleString(t.lang === "zh" ? "zh-CN" : "en-GB", { dateStyle: "medium", timeStyle: "short" });
}

export const list = (t: T, names: string[]) => names.join(t.lang === "zh" ? "、" : ", ");

export type Tone = "ok" | "idle" | "warn" | "bad";

export function tone(agent: Agent): Tone {
  switch (agent.status.code) {
    case "verified":
    case "added":
      return "ok";
    case "unreadable":
    case "failed":
    case "verify-failed":
    case "broken":
      return "bad";
    case "overridden":
    case "drifted":
    case "other-key":
      return "warn";
    default:
      return "idle";
  }
}

export function outcomeText(t: T, outcome: Outcome): string {
  const z = t.lang === "zh";
  switch (outcome.result) {
    case "applied":
      return z ? "已接入" : "Connected";
    case "restored":
      return z ? "已还原" : "Restored";
    case "staged":
      return z ? "未改动" : "Left as is";
    case "waiting":
      return z ? "装好后自动接入" : "Connects once installed";
    case "no-model":
      return z ? "这把 Key 没有可用的模型，未改动" : "This key has no usable model; left as is";
    default:
      return reason(t, outcome.error);
  }
}

export function placeText(t: T, place: Place, dir: string): string {
  const z = t.lang === "zh";
  switch (place.kind) {
    case "up-to-date":
      return z ? "已是最新" : "Up to date";
    case "update":
      return z ? `从 ${place.from} 更新` : `Update from ${place.from}`;
    case "modified":
      return z ? "你改过这个技能，会先备份" : "You edited this skill; it will be backed up";
    case "renamed":
      return z
        ? `已有同名技能，会装为 ${dir.split(/[\\/]/).pop()}`
        : `Name is taken; installs as ${dir.split(/[\\/]/).pop()}`;
    case "linked":
      return z ? "由其他工具管理，不改动" : "Managed by another tool, left alone";
    default:
      return "";
  }
}

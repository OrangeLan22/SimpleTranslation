![](D:\RustProject\SimpleTranslation\src-tauri\icons\Square142x142Logo.png)

# **简译** 

一个 Windows 划词翻译工具。Tauri v2 + Rust 后端 + 原生 HTML/CSS/JS 前端（无打包器、无 npm 依赖）。

---

## 1. 技术选型

| 层 | 选型 | 原因 |
|---|---|---|
| 外壳 | Tauri v2 | 打包体积小（安装包 ~2.5 MB），WebView2 复用系统自带运行时 |
| 后端 | Rust + `windows` crate | 全局低级钩子、UI Automation、剪贴板、窗口样式都要调 Win32 |
| 前端 | 原生 HTML/CSS/JS | 没有构建步骤，改完即生效；`frontendDist` 直接指向 `src/` |
| 翻译 | DeepSeek（OpenAI 兼容协议） | 单一供应商，但走标准 `chat/completions`，换供应商成本低 |
| 更新检查 | Gitee API | 见 §7，**没有应用内更新** |

未引入的技术：

- **无前端框架**（React/Vue）
- **无打包器**（Vite/Webpack）

---

## 2. 目录结构

```
SimpleTranslation/
├── src/                      # 前端（全部静态文件，直接加载）
│   ├── index.html            # 主窗口：文档翻译 + 设置
│   ├── main.js               # 主窗口全部逻辑（~1200 行）
│   ├── styles.css            # 主窗口样式（含 CSS 变量主题）
│   ├── popup.html/.js/.css   # 划词翻译浮窗
│   ├── selection.html        # 划词悬浮按钮（38px 圆形）
│   ├── splash.html           # 启动画面（单文件，CSS 内联）
│   ├── notify.html           # 更新提示浮窗（右下角）
│   ├── tooltip.js            # 自绘悬停提示（替代原生 title）
│   ├── assets/               # 图标、头像、二维码
│   └── vendor/marked.min.js  # Markdown 渲染（第三方，本地托管）
│
├── src-tauri/
│   ├── src/
│   │   ├── main.rs           # 入口， lib::run()
│   │   ├── lib.rs            # 应用装配：插件、命令、窗口、更新、启动流程
│   │   ├── config.rs         # 配置结构与读写、旧版迁移
│   │   ├── selection.rs      # 划词取词（钩子 + UIA + 剪贴板兜底）
│   │   ├── translate.rs      # 翻译请求、SSE 流式解析、模型列表
│   │   └── tray.rs           # 系统托盘菜单
│   ├── capabilities/         # Tauri 权限（见 §5，极易踩坑）
│   ├── icons/                # 应用图标（tauri icon 生成）
│   └── tauri.conf.json       # 窗口定义、打包配置
```

---

## 3. 进程与窗口模型

**一个进程，五个窗口**（`tauri.conf.json` 的 `app.windows`）：

| label | 用途 | 关键配置 |
|---|---|---|
| `main` | 主窗口，文档翻译 + 设置 | `decorations: false`，自绘标题栏 |
| `popup` | 划词翻译浮窗 | `alwaysOnTop`、`skipTaskbar`、失焦自动隐藏 |
| `selection` | 划词悬浮按钮 | `transparent: true`、`focus: false`、46×46 |
| `splash` | 启动画面 | `alwaysOnTop`、`skipTaskbar`、无边框 |
| `notify` | 更新提示（右下角） | `alwaysOnTop`、`skipTaskbar`、`focus: false` |

全部窗口 `visible: false` 创建，由 Rust 按流程 `show()`。这样能控制首帧不闪白。

**单实例**：`tauri-plugin-single-instance` 注册在 Builder 最前面（必须最先），
第二个实例启动时被拦下，回调里唤起已有窗口。注意 `set_focus()` 对最小化窗口无效，要显式 `unminimize()`。

---

## 4. 划词取词（selection.rs）—— 全项目最复杂的部分

Windows 上"判断用户有没有选中文本"没有官方 API，这里是三层策略 + 严格的触发条件。

### 4.1 触发条件：不是所有鼠标松开都要取词

只有这三种才算"用户想选东西"：

1. **拖动选择**（按下后移动超过阈值才松手）
2. **双击 / 三击**（在 `LAST_UP_TICK` 时间窗内）
3. **Shift + 单击**（`shift_pressed()`）

**普通单击不触发。** 原因：有些程序（表格、列表）单击会选中整行，此时发 `Ctrl+C` 能复制到内容，
但用户并没有"要翻译"的意图 —— 误弹按钮比漏弹更烦人。

但是这样还是有些bug，应为需要发送`Ctrl+C`，有些时候可能还是会影响操作，所以添加了对特定程序禁用和快捷键划词翻译。

### 4.2 取词三层

```
鼠标松开
   ↓
① UIA 选区广播（MTA 线程全局监听 UIA_Text_TextSelectionChangedEventId）
   ↓ 未收到
② 落点元素主动查找（ElementFromPoint → IUIAutomationTextPattern）
   —— 只探测一层
   ↓ 未收到（该应用不支持 TextPattern）
③ Ctrl+C + 读剪贴板 + 立刻还原
```

**关键坑 1：UIA 只探落点一层，不沿父链。**
Electron / Chromium 系的容器会`谎报`支持 TextPattern 但选区为空。

**关键坑 2：发 `Ctrl+C` 前必须等修饰键松开。**
用户按 `Alt+Shift+T` 时，Alt 和 Shift 还按着，此时发出的实际是 `Ctrl+Shift+C`
—— 在记事本里是开发者工具，在游戏里会按到 C 键*（我就说怎么给保安的绊线掏出来了）*。
`wait_modifiers_released(700)` 会轮询等待，但**不检查 Ctrl**（快捷键本身可能带 Ctrl，等它会白等）。

**关键坑 3：剪贴板的还原。**
读完立刻还原。原来为空时必须 `arboard::clear()`，否则复制的内容会留在用户剪贴板里。

### 4.3 不该取词的情况

- 前台是本程序自己的窗口（`foreground_is_self()`）
- 桌面 / 任务栏（`is_desktop_or_shell()` 匹配 `Progman` / `WorkerW` / `Shell_TrayWnd`）
- **用户在设置里加的黑名单程序**（`selection_blocked_by_rules()`，按进程名比对）
- **前台全屏**（`is_foreground_fullscreen()`，用 `rcMonitor` 而非 `rcWork`，因为全屏游戏会盖住任务栏）
- 选区超过阈值（`selection_threshold`，默认 5000 字，0 表示不限）

这些判断**必须全部在发 `Ctrl+C` 之前**完成。

### 4.4 悬浮按钮的消失逻辑

按钮出现后，用户做下列操作会立即收起：

- 点击别处、滚轮滚动
- 输入非修饰键（Shift/Ctrl/Alt/Win 按下**不**收，否则 Shift+单击会自己把按钮顶掉）
- 按钮刚显示 400ms 内忽略收起请求（避免和显示动作冲突）

按钮本身用 `WS_EX_NOACTIVATE` 不抢焦点，否则会把用户正在操作的窗口顶掉。

---

## 5. Tauri 权限（capabilities）—— 最容易踩的坑

**`capabilities/default.json` 的 `windows` 数组必须列出所有需要调用 `invoke` 的窗口。**

```json
"windows": ["main", "popup", "selection", "splash", "notify"]
```

新增窗口后如果忘了加进去，该窗口的 `invoke` 会被**静默拒绝** —— 不报错、不崩溃、什么都不发生。

---

## 6. 配置系统（config.rs）

**路径**：`%APPDATA%\com.simpletranslation.app\config.json`
（由 `identifier` 决定，改 identifier 会让所有用户的配置丢失，**不要改**）

**结构要点**：

```rust
Config {
    version, theme,
    trigger: TriggerConfig { mode, hotkey(废弃) },   // mode: button | hotkey | both
    hotkeys: HotkeysConfig { show_window, translate, disable, enable },
    update: UpdateConfig { enabled },
    api: ApiConfig { provider, base_url, model, api_key, temperature },
    scenes: Vec<Scene>, active_scene_id,
    languages: LanguageConfig { source, target },
    stream, selection_enabled,
    selection_rules: SelectionRules { excluded_apps, skip_fullscreen },
    selection_threshold, window: WindowState,
}
```

**几个设计约束**：

- **懒加载**：`AppState` 用 `Mutex<Option<Config>>`，首次访问才读盘。
  直接存 `Config` 会在窗口创建时就拉配置，但那时配置还没加载完。
- **`AppState` 必须在 `Builder` 上 `manage()`**，不能在 `setup()` 里 —— 窗口和 WebView 在 `build()` 阶段就创建了。
- **旧版迁移**：`load_or_init()` 里 `migrate_legacy()` 处理字段改名
  （`trigger.mode: "selection" → "button"`、`trigger.hotkey → hotkeys.translate`、
  `maxSelectionLength → selection_threshold`），迁移后立刻落盘。
- **窗口几何存逻辑像素**，和 `tauri.conf.json` 的 `minWidth/minHeight` 语义一致。
  存物理像素会导致高 DPI 下窗口越开越大。

**窗口位置脏数据防护**：窗口最小化时 Windows 会返回 `-32000` 哨兵坐标，
直接存进配置会变成 `-21333`（`-32000 / 1.5`）。三道防护：
最小化时跳过保存 → 保存前用 `point_on_some_monitor()` 校验 → 读取时校验并自动修正。

---

## 7. 更新检查 —— 无自动更新

**早期做过自动更新，已放弃。** 原因是 GitHub releases 在国内下载 3 MB 的安装包要 130 秒
（25 KB/s），公共镜像（ghproxy 等）也不稳定。而 Gitee 的发行版附件又要求登录，无token容易返回403。

**现在的方案**：

```
启动 / 手动检查
   ↓ GET https://gitee.com/api/v5/repos/orangelan/SimpleTranslationDL/releases/latest
   ↓ 读 tag_name、body（更新日志）、assets 里的 .exe 直链
   ↓ parse_version 比较（兼容 v / beta 前缀）
   ↓
有新版本 → ① 主窗口顶部黄色提示条
           ② 右下角自绘浮窗（4.2 秒自动收）
           ③ 设置页状态文字
点「查看详情」→ 更新日志弹窗（Markdown 渲染 Gitee 发行版描述）
点「去下载」  → 浏览器打开附件直链（浏览器带登录态，能正常下）
```

**`version` 前缀在展示层**：底层 `Cargo.toml` / `tauri.conf.json` 必须是合法 semver（如 `1.1.2`），
`v` 前缀由 `get_app_version()` 拼接上去。直接把 `v1.1.2` 写进版本字段会导致打包失败。

---

## 8. 翻译链路（translate.rs）

```
前端 invoke("translate", { req }) 
   ↓ run_translate()
   ↓ pick_prompt()：按 sceneId 找场景提示词，找不到或为空则回退到内置通用提示词
   ↓ render_prompt()：替换 {target} {source} {text}
   ↓ POST {base_url}/chat/completions （stream: true）
   ↓ 逐行解析 SSE "data: {...}" → emit("translate-chunk")
   ↓ emit("translate-done", { fullText })
```

**提示词三个变量**：

| 变量 | 含义 |
|---|---|
| `{target}` | 目标语言 |
| `{source}` | 源语言；`auto` 时替换成「自动检测到的语言」 |
| `{text}` | 原文 —— **一般不用写**，原文已单独作为 user message 发送，写了会重复 |

**空提示词兜底**：新建场景的 `prompt` 默认为空，`pick_prompt()` 会回退到 `DEFAULT_GENERAL_PROMPT`。
不兜底的话 system message 会是空字符串，模型行为不可控。

---

## 9. 前端约定

**没有构建步骤**，所以代码直接跑在 WebView2 里。几个约定：

**禁止文本选中，输入框例外**：

```css
* { user-select: none; }
input, textarea, [contenteditable] { user-select: text; }

/* 译文区域要连所有后代一起豁免：
   全局的 * 是直接命中，会盖过从容器继承下来的值 */
.output, .output * { user-select: text; }
```

**主题**：CSS 变量定义在 `:root` 和 `:root[data-theme="dark"]`，
`apply_theme()` 同时设置窗口原生底色（`set_background_color`），消除 WebView2 首帧白屏。

**圆角统一用 `corner-shape: squircle`**：

```css
border-radius: 10px;
corner-shape: squircle;   /* 必须和 border-radius 同一个规则块 */
```

**悬停提示用 `data-tooltip` 而不是 `title`**：原生 `title` 是 Windows 画的浅色小方块，和界面风格不搭。
`tooltip.js` 提供自绘版本，给元素加 `data-tooltip="文字"` 即可。

**事件监听的两条路*路径：Tauri 的 `listen()` 是异步注册的，后端的 `emit` 可能抢先到达而丢失。
涉及"窗口刚显示就推数据"的场景，后端应同时缓存一份，前端 `listen()` 之后再主动 `invoke` 取一次。

---

## 10. 构建与发布

### 本地开发

```powershell
.\run.ps1          # = npx tauri dev
.\run.ps1 build    # = npx tauri build
```

### 打包

```powershell
npx tauri build
```

产物：`src-tauri/target/release/bundle/nsis/简译_<版本>_x64-setup.exe`
不需要签名环境变量（已移除 updater）。

### 版本号

三处必须同步改：

- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `package.json`

底层必须是合法 semver（`1.1.2`），界面显示的 `v` 前缀由代码拼接。

### 发版流程

1. 改三处版本号
2. `npx tauri build`
3. 上传到两个平台：
   - **Gitee**（`orangelan/SimpleTranslationDL`）：应用从这里读版本号和更新日志，
     描述里写的 Markdown 会原样显示在应用的更新日志弹窗里
   - **GitHub**：给用户手动下载用，应用不从这里读任何东西
4. 两边附件都是同一个 `.exe`

⚠️ Gitee 发行版**不要勾 prerelease、不要存草稿**，否则 `/releases/latest` 会 404。

---

## 11. 已知限制

| 限制 | 说明 |
|---|---|
| 仅 Windows | `selection.rs` 全是 Win32 调用，其他平台不编译 |
| VSCode 终端取词会清选区 | 终端不支持 UIA，走 `Ctrl+C` 兜底时终端会把它当"复制并取消高亮"。这是终端自身行为，无解 |
| 部分应用取不到词 | 游戏、远程桌面等会拦截 `SendInput`，只能靠黑名单或全屏检测规避 |
| Gitee API 匿名限流 | 低频够用；若遇到限流，需在 `GITEE_API` 加 `access_token` |
| 黑名单按进程名匹配 | `chrome` 会排除所有 Chrome 窗口，不能精确到某个路径 |
| `identifier` 不能改 | `com.simpletranslation.app` 决定配置目录，改了丢配置文件 |


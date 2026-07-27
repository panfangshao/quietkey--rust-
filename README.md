# quietkey
一个热键暂停/播放后台的视频，**不切窗口、不抢焦点、不打断打字**。
写笔记的时候想暂停视频，不用再 Alt+Tab 过去按空格再切回来。

# 目录说明

| 文件/目录   |  传?    |        原因                          

│ src/       │ ✅ 必须 │ 源码                 

│ Cargo.toml │ ✅ 必须 │ 依赖清单。没它 cargo build 直接失败

│ Cargo.lock │ ✅ 必须 │ 可执行程序(不是库)就该提交它,锁死依赖版本,保证换台机器编出来一模一样 │

│ build.rs   │ ✅ 必须 │ 生成 .ico 并嵌进 exe。没它编出来的程序没图标     

│ .gitignore │ ✅      │ 让 target 自动被忽略         

│ README.md  │ ✅      │ 说明文档                     

│ CLAUDE.md  │ 🤔 你定 │ 私人仓库可以，对外不行         

│ target/    │ ❌      │ 4.2 GB,且是编译产物。用.gitignore 里记录它让它已经排除了  

一句话:除了 target/,其余全传(所以你在这里看不到target文件夹)。


## 它和「切窗口按空格」的脚本有什么不同

常见做法是：激活视频窗口 → 模拟按空格 → 再激活回笔记窗口。能用，但有两个硬伤：
屏幕会闪，而且焦点被抢走的那一瞬间打的字会散落到别的窗口里。

quietkey 走的是四层策略链，逐层降级，**前三层完全不碰前台焦点**：

| 层 | 手段 | 适用 | 打扰 |
|---|---|---|---|
| 1 | Windows 媒体会话（GSMTC） | 浏览器网页视频、Spotify 等 | 无 |
| 2 | `WM_APPCOMMAND` 定向投递 | VLC / PotPlayer / foobar2000 | 无 |
| 3 | `PostMessage` 键盘消息 | 原生 Win32 播放器 | 无 |
| 4 | 抢焦点 + `SendInput` + 还原 | 兜底，默认关闭 | 屏幕闪一下 |

第一个成功的层即生效。界面上会显示这次命中的是哪一层，一眼看出有没有掉到会闪烁的兜底路径。

### 为什么网页视频必须走第 1 层

向浏览器的顶层窗口投递合成的 `WM_KEYDOWN` 是**行不通的**——Chromium 的键盘输入
走自己的 renderer 输入管线，不读顶层窗口的消息队列，合成消息会被直接丢弃。

所以对网页视频来说，媒体会话 API 不是优化，而是唯一可行的无感方案。好处还不止于此：
它不依赖窗口标题（标题会随播放进度变），窗口最小化或被完全遮挡也照样生效。

### 同时开着多个视频时，控哪一个

跟随**你最近手动播放或暂停过的那一个**。

判据是每个媒体会话自带的「播放位置信息最后更新时刻」——手动播放、暂停、拖进度条
都会刷新它，而一直播着不动**不会**。所以"播了又停"这种净状态没变的操作也认得出来。
按会话逐个比时间戳，与具体是哪个播放器无关，同一浏览器的不同视频标签页也能分开。

在「策略链 → 跟随最近播放/暂停过的那个」可以关掉这个行为。
「目标窗口」里填了进程名的话，仍以配置为准。

### 为什么用 RegisterHotKey 而不是键盘钩子

热键走 `global-hotkey`，底层是 `RegisterHotKey`；没有用 `rdev` 那类低级键盘钩子：

- **杀软**：全局低级键盘钩子是 keylogger 的教科书特征，未签名的自编译程序很容易被拦。
- **全系统输入延迟**：钩子回调跑在本进程，受 `LowLevelHooksTimeout`（约 300ms）约束，
  在回调里做窗口查找会拖慢**整个系统**的键盘响应。
- **隐私**：钩子能看到你敲的每一个字符，`RegisterHotKey` 只能看到注册的那一个组合键。

附带好处：系统会独占消费注册过的组合键，不会再漏给正在打字的笔记软件。

空闲时进程完全休眠，CPU 占用 0%。

## 使用

双击 `target\release\quietkey.exe` 即可，无依赖、单文件。首次启动会弹出设置界面：

1. **全局热键** — 默认 `Alt+Q`，可以点「录制」直接按一个新的。
2. **目标窗口** — 只有第 2/3/4 层需要它。点「从当前窗口列表选择…」在列表里点一下播放器。
   存的是进程名 + 窗口类名这类匹配规则，不是窗口句柄，所以播放器重启后依然有效。
   标题默认不参与匹配（它会随播放进度变）。
   > 如果你控制的是浏览器网页视频或其它有媒体会话的播放器，第 1 层就够了，这一步可以跳过。
3. **保存并应用**，然后关掉窗口——程序缩到托盘继续跑。

托盘图标双击可以再打开设置，右键菜单里有「立即播放/暂停」和「退出」。
**关窗口不会退出程序**，真正退出只走托盘菜单的「退出」。

配置存在 `%APPDATA%\quietkey\config.toml`。想恢复出厂设置就删掉它。

## 排查

界面下方的「诊断」区：

- **立即测试一次** — 用界面上当前的设置试一次，不必先保存。下面的「上次触发」
  会列出每一层的尝试结果和耗时。
- **刷新媒体会话** — 列出系统当前所有媒体会话，标出谁是「最近碰过」的（★，
  也就是热键会作用的那一个）。**如果你的播放器不在这个列表里，说明它没注册媒体会话，
  第 1 层对它无效**，得靠第 2/3 层（需要填目标窗口）。

如果前三层都显示「跳过」，再去打开第 4 层兜底。

命令行也能跑诊断，不用开界面（**只在 debug 构建下看得到输出**，release 没有控制台）：

```powershell
.\target\debug\quietkey.exe --list-sessions   # 只读列出媒体会话，不会动你的视频
.\target\debug\quietkey.exe --test-once       # 按当前配置跑一次策略链并打印逐层结果
```

`--list-sessions` 的输出：

```
共 2 个媒体会话：
  ▶ 播放中    msedge                   301.8 秒前碰过
  ⏸ 已暂停 ★ chrome                   最近碰过
```

`--test-once` 的输出：

```
结果: PostMessage 按键 · 空格已投递到 notepad.exe 的主窗口（无法确认是否响应） · 18.7ms
  目标窗口             命中  notepad.exe · 「无标题 - 记事本」
  GSMTC 媒体会话       跳过  系统当前没有任何媒体会话
  WM_APPCOMMAND      跳过  notepad.exe 未处理 WM_APPCOMMAND
  PostMessage 按键     命中  空格已投递到 notepad.exe 的主窗口（无法确认是否响应）
```

跑 debug 版还会往控制台打日志，每次触发一行，直接回答"为什么控的是它"：

```
INFO quietkey::strategy::gsmtc] 会话活跃度: msedge(27.7s前) chrome(最近)
INFO quietkey] 热键触发: GSMTC 媒体会话 · chrome → 已播放（最近活跃） · 12.8ms
```

---

# 自己编译

## 环境

- **Rust MSVC 工具链** — <https://rustup.rs>
- **Visual Studio 生成工具**（提供 `link.exe`）— 安装时勾选「使用 C++ 的桌面开发」
- **Windows SDK**（提供 `rc.exe`，编 exe 图标资源要用）—— 随上一项一起装

## 常用命令

```powershell
cargo check                  # 最快，只做类型检查，改代码时用它
cargo build                  # debug → target\debug\quietkey.exe（有控制台、有日志）
cargo build --release        # release → target\release\quietkey.exe（无控制台、无日志）
cargo run                    # 编译并直接运行 debug 版
cargo test                   # 单元测试
cargo clippy --all-targets   # 静态检查，比 check 更严
```

提高日志等级：`$env:RUST_LOG="quietkey=debug"; cargo run`

## 编译产物

| 产物 | 路径 | 特点 |
|---|---|---|
| debug | `target\debug\quietkey.exe` | 保留控制台、能看日志，体积大 |
| release | `target\release\quietkey.exe` | 无控制台窗口、**看不到任何日志**，日常使用版 |

要发给别人/放桌面用，只需拷 release 那**一个文件**，无依赖。

首次 release 编译要 2 分多钟（`Cargo.toml` 里开了 `lto = true` + `codegen-units = 1`，
拿编译时间换体积和启动速度）。debug 只要几秒。

> 编译时报 `拒绝访问 (os error 5)`，是因为**目标 exe 正在运行**，
> 先从托盘退出它再编。

# 项目结构

```
quietkey/
├── Cargo.toml     依赖清单 + 编译配置
├── Cargo.lock     依赖版本锁定（要提交进 git）
├── build.rs       构建脚本：生成多尺寸 .ico 并嵌进 exe 资源段
├── README.md      本文件
├── CLAUDE.md      设计依据与踩过的坑，改代码前该看的文件
├── src/
└── target/        编译产物（自动生成，可整个删）
```

## src/

| 文件 | 职责 |
|---|---|
| `main.rs` | 入口：日志 → 读配置 → 注册热键 → 起 `hotkey-worker` 线程 → 开窗口。也处理 `--test-once` / `--list-sessions` |
| `app.rs` | egui 设置界面：热键、目标窗口、策略链开关、逐层 trace、诊断。还负责中文字体、托盘事件、拦截关窗改成缩托盘 |
| `theme.rs` | Win7 Aero 风格的浅色主题。所有配色集中在这里，换风格只改这一个文件 |
| `config.rs` | 配置结构与读写，落在 `%APPDATA%\quietkey\config.toml` |
| `hotkey.rs` | 热键注册/注销，`"Alt+Q"` 这类字符串与按键的互转 |
| `state.rs` | 跨线程共享的 `Shared`：进去配置快照，出来执行报告。UI 线程和 worker 线程只通过它通信 |
| `tray.rs` | 托盘图标与右键菜单，把托盘事件转存进队列并唤醒界面重绘 |
| `icon.rs` | 图标像素的唯一来源，托盘/窗口/exe 三处共用。带单元测试 |

## src/strategy/ —— 核心：四层降级策略链

| 文件 | 职责 |
|---|---|
| `mod.rs` | 调度器。按打扰程度从小到大依次尝试，第一个成功即停，每层结果记进 trace |
| `gsmtc.rs` | 第 1 层 Windows 媒体会话。挑哪个会话的五级规则也在这里 |
| `recency.rs` | 判定「最近被播放/暂停过的是哪个会话」。无状态纯函数，带单元测试 |
| `messages.rs` | 第 2 层 `WM_APPCOMMAND` + 第 3 层 `PostMessage` 投空格 |
| `fallback.rs` | 第 4 层 抢焦点 + SendInput + 还原。唯一会打断你的一层，默认关闭 |

## src/win/ —— Win32 封装

| 文件 | 职责 |
|---|---|
| `mod.rs` | 对外接口：`resolve_target`（按规则实时解析窗口句柄）、`list_windows` |
| `list.rs` | 枚举窗口，过滤 owner 窗口、工具窗口、DWM cloaked 的隐形 UWP 窗口 |
| `focus.rs` | `force_foreground`——后台进程抢前台要做的 `AttachThreadInput` 那套，只有第 4 层用 |

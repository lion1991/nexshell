# RDP 剪贴板对齐 Windows App 实施计划

Status: in progress — 第 0、1、3、5 步完成

Decision record: [ADR 0016](../adr/0016-rdp-clipboard-windows-app-parity.md)

## 目标

- 在 Mac 和远端之间直接复制粘贴文本、富文本、图片和文件，两个方向都通。
- 远端复制时不拖慢 NexShell 界面；传大文件时有进度，能取消。
- 可以逐主机限制方向或关闭剪贴板。

## 第 0 步：调研（完成）

- 静态分析 Windows App：了解它支持的格式、延迟提供的方式、XPC 辅助进程和相关设置项。
- 用 Swift 探针实测延迟提供：回调所在线程、有没有超时、剪贴板管理器的影响。

结论记在 ADR 0016 的"Windows App 的做法"和"本机实测"两节。

## 第 1 步：NSPasteboard 接入，Mac → 远端支持富文本和图片（完成）

- `clipboard.rs` 拆成目录：
  - `mod.rs`：backend 与轮询；
  - `local_mac.rs` / `local_other.rs`：本地剪贴板接口，macOS 直接调 NSPasteboard，其他平台用 arboard；
  - `text.rs`：CF_UNICODETEXT 转换，含现有纯函数和单测；
  - `dib.rs`：像素转 CF_DIB。
- 轮询：检查间隔从 1 秒改为 250 ms，判断依据从文本 hash 改为 `changeCount`。
- 广播：按剪贴板里有的类型广播格式——CF_UNICODETEXT、`Rich Text Format`、`PNG` + CF_DIB。剪贴板里有 file URL 时不广播图片。
- 应答：`on_format_data_request` 把读取和转换交给工作线程，结果经原来的 channel 送回事件循环。
- 远端 → Mac 这一步仍只处理文本，立即拉取，写入改走 NSPasteboard。写入时持锁，轮询看不到清空后、写完前的中间态。
- 诊断：设 `NEXSHELL_RDP_CLIPBOARD_TRACE` 后，打印双方的格式清单、请求耗时和应答大小。
- 真机修正：截图工具先清空剪贴板、过一阵才写入图片，写入时 `changeCount` 不再增加。只看它会把中途的空剪贴板当成最终内容，还会把远端剪贴板清空。所以变化标记改为 `changeCount` 加类型清单。

验证：
- 单测：DIB 生成（行序、行距、白底合成），格式清单的组合。
- 真机：
  - Mac 截图到剪贴板，粘进远端画图、Word、微信；
  - 文本编辑（TextEdit）里的富文本粘进写字板或 Word，保留格式；
  - 文本双向照旧可用。

## 第 2 步：辅助进程，远端 → Mac 延迟提供文本、富文本和图片

- `--pasteboard-helper` 模式：
  - 不初始化 UI，只跑主 runloop，负责写入带 provider 的 `NSPasteboardItem`；
  - provider 被调用时，经管道向父进程要数据，在它自己的主线程上阻塞等待。
- 父进程：
  - 首次有远端复制时拉起辅助进程，读管道的线程把请求派给对应会话的 RDP 线程；
  - 会话断开时，若剪贴板还是这次会话写入的，就让辅助进程清空剪贴板。
- 图片：远端给了 PNG 就直接用；否则把 CF_DIBV5 / CF_DIB 转成 PNG 和 TIFF。
- 两个会话之间互相复制（A 的内容粘进 B）要能走通。

验证：
- 在开着 Alfred 的机器上，远端每复制一次，NexShell 界面都不卡：看 `[rdp-ui]` 帧率诊断。
- 远端截图工具复制，Mac 预览选"从剪贴板新建"；Word 富文本粘进 Pages 或 TextEdit。

## 第 3 步：Mac 文件 → 远端（完成）

- 能力：本端声明 `STREAM_FILECLIP_ENABLED | FILECLIP_NO_FILE_PATHS | CAN_LOCK_CLIPDATA | HUGE_FILE_SUPPORT_ENABLED`。服务端不同意文件流时，Finder 复制的文件仍按文本（文件名）广播。
- 展开（`files.rs`，在轮询线程）：
  - Finder 给的 file URL 多为 `file:///.file/id=…`，经 `filePathURL` 转成路径；
  - 目录在前、内容随后，相对路径用 `\`；
  - 跳过 `.DS_Store`、读不到元数据的条目、特殊文件，以及拼上路径后超过 259 个 UTF-16 单元的条目；
  - 文件名里 Windows 不允许的字符（含 `\`、`/`、`:`）换成 `_`；
  - 目录按规范化路径去重，挡住符号链接成环；
  - 超过协议上限 100000 条时整份放弃，退回按文本广播。
- 清单对应：fork 回调只给序号和锁 ID。backend 记下最近一次广播的路径清单，收到 Lock 时另存快照，Unlock 时丢掉，带锁 ID 的请求按快照取文件。事件循环先把清单交给 backend，再调 `initiate_file_copy`。本端预先过滤，fork 的校验不会再删条目，序号保持一致。
- 应答：SIZE 请求现取文件大小；RANGE 请求在应答线程读取，单次最多 32 MB，连续读同一文件时复用句柄。
- 锁回收：事件循环每 5 秒调一次 `drive_timeouts`。
- 去重：Finder 复制一次会分几批写剪贴板，变化标记跟着变好几次。记下远端剪贴板里是哪批顶层路径，同一批不重发；远端复制、拒收，或本端改发别的格式时清掉记录。
- 时序细节：
  - 连接时剪贴板里已有文件：初始化阶段发不了文件清单，先按文本广播，通道就绪后由轮询补发；
  - 展开大目录时不持锁，展开完发现远端已写入新内容，就丢掉这份清单。
- cliprdr 协议层报错改为只记日志，不再断开会话；网络写失败照旧断开。

验证：
- 单个文件、多个文件、嵌套目录、中文文件名，逐个核对 hash；
- 1 GB 文件：资源管理器显示进度，中途取消后远端能正常恢复。

## 第 4 步：远端文件 → Finder

1. 先实测：
   - 装着 Alfred 时，Windows App 在远端复制 1 GB 文件后会不会立即开始下载；
   - 在 Finder 里粘贴时的等待表现；
   - Alfred 读不读 file URL。
2. 按实测结果实现：provider 应答 file URL 前，用 `ChunkedFetch` 下载到 `~/Library/Caches/NexShell/rdp-clipboard/`；目录按 FileGroupDescriptorW 里的相对路径重建。
3. 会话断开或远端重新复制时，清理缓存；在界面上提示下载进度。

## 第 5 步：⌘ 剪贴板快捷键（提前做，完成）

- ⌘C / ⌘X / ⌘V / ⌘A / ⌘Z 转成 Ctrl 组合发给远端，其他 ⌘ 组合仍发 Win。
- 参照 Windows App 的 `CommandKeyDelay`：`ModifierTracker` 记下 ⌘ 当前扮演的键。
  - ⌘ 按下先不发，由下一个键决定当 Ctrl 还是 Win。
  - 单按 ⌘ 松开时补发一次 Win 的按下和抬起，开始菜单照常弹出。
  - 按着 ⌘ 换键时两者可以来回切，例如先 ⌘E、再 ⌘C。
- 丢了 ⌘ 的抬起事件时，对账只抬起已经发出去的键；没发出去的不补发 Win，免得弹开始菜单。
- ⌘C 会被本地"编辑"菜单截走，RDP 画面收不到这个按键；在复制处理里转发 Ctrl+C。
- 真机修正：macOS 不投递 ⌘ 组合键的 KeyUp。原先把这个键记成"按着"，远端也一直按着，第二次 ⌘V 就不再发按下，改前的 Win+E 等组合同样会卡键。现在 ⌘ 按着时，每次按下（含自动重复）都当场补发抬起。
- 真机修正：⌘Tab 切换应用不触发 `on_blur`。切走时本窗口只收到 ⌘ 按下，切回来只收到 ⌘ 抬起，被当成单按 ⌘ 补发了 Win，远端弹出开始菜单。现在订阅活动窗口变化，窗口进出前台时都抬起远端按键、清掉 ⌘ 的状态。
- 关闭开关并入第 6 步的主机设置。

## 第 6 步：逐主机剪贴板模式

- 主机设置加一项"剪贴板"：双向、仅本地 → 远端、仅远端 → 本地、关闭。默认双向。
- 同一处加"⌘C / ⌘V 当 Ctrl"开关（第 5 步），默认开。
- 关闭时不注册 cliprdr 通道；单向时对应方向不广播也不应答。
- 连接信息面板显示当前的剪贴板模式。

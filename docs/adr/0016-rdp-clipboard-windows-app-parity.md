# RDP 剪贴板对齐 Windows App：富格式、文件互拷、远端数据延迟提供

Status: proposed (2026-09-29)

Plan: [实施计划](../plans/2026-09-29-rdp-clipboard.md)

## 背景

- 现状（`src/rdp_session/clipboard.rs`）：
  - 只支持 CF_UNICODETEXT。
  - 用 arboard 每秒读一次文本、算 hash 来判断本地有没有新复制。
  - 远端一复制就立即拉取文本写进 Mac 剪贴板（eager）。
  - 图片、富文本、文件都传不过去；文件只能走 `\\tsclient\NexShell` 共享盘（`rdpdr.rs`）。
- 键位：⌘ 映射为 Win 键（`rdp_view/keymap.rs`），⌘C / ⌘V 到远端变成 Win+C / Win+V，在远端复制粘贴只能按 ⌃C / ⌃V。
- 开关：所有会话无条件开剪贴板，没有逐主机开关（代码审阅 2026-07-23 P2-12）。
- IronRDP fork 的 cliprdr 协议层已经完整，缺的只是 macOS 后端（`cliprdr-native` 只有 Windows 实现）。已有的部分：
  - 长格式名；
  - FileGroupDescriptorW 双向（`initiate_file_copy` / `on_remote_file_list`）；
  - FileContents 请求、应答与下标校验；
  - Lock / Unlock 快照与超时回收（`drive_timeouts`）；
  - 分块下载 `ChunkedFetch`。

## Windows App 的做法（静态分析，2026-09-29）

- **格式**：
  - 文本（CF_UNICODETEXT）；
  - Rich Text Format；
  - 图片：PNG 和 DIB，DIB 含调色板版本（`PngFormatDataPacker`、`InitializeRGBDIB`、`InitializePaletteDIB`）；
  - 文件：FileGroupDescriptorW + FileContents，双向。

  没有 HTML Format。
- **远端 → Mac 全部延迟提供**：每类数据对应一个 `NSPasteboardItemDataProvider`，即 `SessionPasteboardItem{Text,Image,File}DataProvider`。别的程序粘贴时，才去向服务端要数据。
- **provider 不在主进程**：放在独立的 `pasteboard.xpc` 服务里，经 `PasteboardXpcProtocol` 与主进程通信。旧版用的是 `pasteboardproxy` 辅助进程加 CFMessagePort。
- **远端文件粘进 Finder**：provider 应答 `public.file-url`，文件先落到临时目录（导入了 `NSTemporaryDirectory`、`NSPasteboardTypeFileURL`，并有 `SessionPasteboardFileSaveOperation`）。
- **Mac 文件 → 远端**：按需读本地 file URL（`LocalPasteboardFile`）。
- **设置**：
  - `redirectclipboard` 共四档：0 关闭、1 双向、2 仅本地 → 远端、3 仅远端 → 本地。
  - `useCommandKeyForClipboard`：把 ⌘C / ⌘V 转成 Ctrl。配合 `CommandKeyDelay`，先把 ⌘ 按下压住，看下一个键再决定发什么。

## 本机实测（2026-09-29，Swift 探针）

- provider 回调总在**主线程**执行，即使 item 是在后台线程写进剪贴板的。
- 剪贴板服务没有 60 秒超时：provider 睡 75 秒，`pbpaste` 等了 72 秒照样拿到数据。
- 本机常驻 Alfred 和 ClipBridge。promise 一写进去，provider 在 t=0s 就被调用，也就是远端每复制一次都会马上被拉数据。
- 推论：在 NexShell 主进程里做延迟提供，远端每复制一次主线程就要等一次网络。大图走广域网能卡好几秒，文件更久。这正是 Windows App 把 provider 放进独立进程的原因。

## 决策

1. **macOS 直接调用 NSPasteboard，不再经过 arboard**。用 objc2-app-kit，已在依赖树里。其他平台保留 arboard 文本路径。本地剪贴板操作收拢成一组接口：变化标记、可用格式、按格式读、写。
2. **本地变化改为轮询 `changeCount`**（250 ms），替代每秒读文本算 hash。自己写入后记下新的 `changeCount`，避免把自己写的内容又发回远端。
3. **格式映射**（对齐 Windows App，不做 HTML）：

   | 内容 | Mac | RDP | 说明 |
   |---|---|---|---|
   | 文本 | `public.utf8-plain-text` | CF_UNICODETEXT（13） | UTF-16LE，CRLF |
   | 富文本 | `public.rtf` | `Rich Text Format` | RTF 原样透传 |
   | 图片 | 读 `public.png`，没有再读 `public.tiff`；写 PNG + TIFF | `PNG` + CF_DIB（8） | 远端来时 PNG 优先，其次 CF_DIBV5、CF_DIB |
   | 文件 | 多个 `public.file-url` | `FileGroupDescriptorW` + FileContents | 目录递归展开成相对路径 |

   Finder 复制文件时，剪贴板里还带着图标 TIFF，此时不广播图片。
4. **Mac → 远端：只广播格式清单，数据现取现转**。服务端请求哪种格式，才读剪贴板并转换。读取和转换放到工作线程，不占 RDP 事件循环。图片用 NSBitmapImageRep 解码，再画进 CGBitmapContext 得到 BGRA，垫白底后生成 CF_DIB；PNG 格式保留原始透明度。
5. **远端 → Mac：延迟提供，provider 放进辅助进程**。
   - 辅助进程就是同一个可执行文件，以 `--pasteboard-helper` 模式启动，通过 stdin / stdout 管道通信。
   - 每个 NexShell 进程只起一个；父进程的管道一关，它就退出。
   - 辅助进程的主线程可以放心阻塞，NexShell 界面不受影响。
   - 图片和文本数据经管道传过去；文件只传临时路径。
6. **远端文件 → Finder**：provider 应答时，先把文件下载到缓存目录，再交出 file URL（与 Windows App 相同）。大小阈值、进度提示、剪贴板管理器误触发下载的对策，等第 4 步实测后再定。
7. **Mac 文件 → 远端**：
   - 本端声明 `STREAM_FILECLIP_ENABLED | FILECLIP_NO_FILE_PATHS | CAN_LOCK_CLIPDATA | HUGE_FILE_SUPPORT_ENABLED`。
   - 服务端来 FileContents 请求时按范围读本地文件，读取不占事件循环。
8. **⌘C / ⌘X / ⌘V 转为 Ctrl 组合**，参照 `useCommandKeyForClipboard`，可以关掉。
9. **逐主机剪贴板模式**：四档，与 `redirectclipboard` 相同，默认双向。

## 风险

- **剪贴板管理器（Alfred 等）会立即读取 promise**。远端每复制一张图都会被拉一次。如果管理器读 file URL，就会触发整份文件下载。第 4 步实测 Alfred 读不读 file URL，并与 Windows App 对照。
- **两端都开着第三方剪贴板同步工具（如 ClipBridge）时互相覆盖**：Mac 复制文件后，工具把文件路径文本同步到 Windows，顶掉 cliprdr 发过去的文件清单；这段文本又经 cliprdr 回到 Mac，把 Finder 的复制也顶掉，表现为远端要粘好几次。这是两套同步机制在抢同一个剪贴板，NexShell 不做规避；遇到时关掉其中一端的同步工具。
- **后台线程读 NSPasteboard**：苹果没有明文保证线程安全。arboard 一直这样用，没出过问题。
- **辅助进程**：带来进程管理和 IPC 的复杂度。进程崩溃时，已写入的延迟数据取不到（粘贴为空）；下次远端复制时重新拉起。
- **CF_DIB 不压缩**：大图走广域网慢，所以服务端同时给 PNG 时优先取 PNG。

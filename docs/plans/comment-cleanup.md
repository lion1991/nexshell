# 注释与检查豁免清理清单

Status: 盘点 commit：`f4dd308a33ef7a3aa5f1d5eabe6c9f7d79c4e358`，日期 2026-11-21。逐条拍板与完成情况见各条「决定」「完成」栏。

这份清单是清理注释的依据：B（为捷径辩护，含检查豁免）、D（待办）、E（注释掉的代码）、U（不确定）、过期的 C 逐条列出，「决定」「完成」两栏由维护者填。A（叙述型噪音）这里只给数量和分布，逐条见附表 [`comment-cleanup-a.md`](comment-cleanup-a.md)。没过期的 C（有价值的「为什么」）不列，保留不动。

决定栏取值：B 填 修代码 / 记待办 / 保留；D 填 补引用 / 删除；E 填 删除 / 保留；U 填 当 A / 当 C / 保留（工具会读）；过期的 C 填 按建议改 / 不改。完成栏填完成日期（状态更新和改动在同一个提交里，写不进自己的 SHA）；对应提交用 `git log -S'<编号>' -- docs/plans/comment-cleanup.md` 查。

## 拍板记录

2026-11-21 由 Fable 代 Matt 拍板。这里的结论和各条「拍板备注」，与「决定」栏同等效力。

### 产品问题

1. **设置菜单的 What's New / Documentation / Feedback / View Logs（B-008）：去掉。** 四项从初始提交 8d80514 起就只关菜单，用户点了像坏了。否决「接外链」：项目没有文档站、反馈入口，也不写日志文件，接上等于新做四个功能，没人要过。菜单只留「设置」「快捷键」两项。
2. **主机编辑窗口的 SSH「最大失败次数」（B-064）：移除配置项。** `keepalive_max` 从初始提交 8d80514 起就写死为 0，这个输入框从来没生效过；唯一真用到这个值的 `ssh_pty_command_for_host`（拼系统 ssh 的 ServerAliveCountMax）在生产里没有调用方。否决「接上」：有些服务器和中间设备不回 keepalive 请求，接上会把健康的空闲会话断掉，当初就是为此关的；要接得在这类老设备上真机验证，属于新行为。连带影响：新版导出的主机文件少一个字段，旧版 NexShell 导不进去；旧版导出的文件新版照常能导入。
3. **`NEXSHELL_BAR_DEBUG` 调试打印（B-009）：删。** 它和标签底条滑动是同一个提交 1008863（2026-07-07）加进来的，要定位的毛病在那次提交里就修了：提交说明写了弹簧改半隐式欧拉子步积分（治空闲后大 dt 单步的镜像过冲）、提前落靶收敛（治取整后的静止补跳）、缺位帧沿用弹簧值（治闪断）。之后没有提交再碰过 `tab_accent_bar_*`（`git log 1008863..HEAD -G'tab_accent_bar' -- src` 为空），`SpringAnim` 的实现也没改过；7ac14af 修的是面板滑动串标签，是另一件事。
4. **从 Warp 抄来的上游注释（D-001 至 D-006、D-008 至 D-017、B-060）：保留，一个字不动。** 这 16 条 TODO 和 `_vertical_padding` 的注释，在 `../warp` 对应文件里逐字相同。删了没有收益（不是我们的待办，本仓不会做），却多出一批和上游的差异：以后对照上游摘修复要多绕这些 hunk，重新同步文件时这些 TODO 行又会作为新增行被捷径检查拦下（检查合入后）。否决「删除」；也否决「补引用」，给不会做的事编号是假跟踪。D 类的取值里没有「保留」，这 16 条的决定栏按本条写成「保留：Warp 上游原文」，清理时跳过。
   - 同样的理由，附表里下面 26 条 A 类也是上游原文，不删。清理对应批次前，先把附表里这些行的「状态」填成「保留（上游原文）」，`delete_a.py` 会跳过：
     - `src/themes/theme.rs`：361、546、556
     - `src/ui_components/avatar.rs`：62、76
     - `src/util/bindings.rs`：11
     - `src/util/grid.rs`：28、32、50、67–68
     - `src/view_components/action_button.rs`：488、700、703、712、735、787、807–810、837、927
     - `src/menu.rs`：423、428、436、1425、1599、1607
     - `src/time_format.rs`：5
   - 这一条超出了交办的范围（附表本来默认全删），等 Matt 确认；确认前这 26 条先别删。

### 真 bug 走哪条路

单独修的两条先做，各自带回归用例单独提交；其余留在第 3 步的批次里。

| 顺序 | 问题 | 条目 | 路径 | 优先级 | 理由 |
|---|---|---|---|---|---|
| 1 | SFTP 覆盖保存先删后改名 | B-063 | 单独修，先做 | P1 | 可能丢文件。OpenSSH 的 SFTP v3 rename 在目标已存在时会失败（按其实现推断，未真机实测，修复时先确认），所以覆盖上传和远程编辑保存每次都走「删原文件再改名」：这两步之间断线，原文件没了，新内容只剩在隐藏的 `.<名字>.nexshell-tmp` 里；第二次改名失败而会话还在，临时文件也被清掉。改的是核心保存路径，要选方案、要真机验证，不夹在注释清理批次里。 |
| 2 | 终端同步输出没有超时 | B-003、B-005 | 单独修，先做 | P1 | 标签页卡死。收到 `?2026h` 后没等到 `?2026l`（TUI 画到一半被杀、嵌套 ssh 在重绘中途断开、cat 到含这个序列的日志），后续输出连同按键回显全被缓冲，攒满 2 MiB 才出来，看上去整个标签页死了。修复跨 `pty_event_loop` 和 `terminal_runtime` 两个批次，本地和远程读循环都要改。 |
| 3 | `lib.rs` 里 11 个测试被 `#[cfg(any())]` 关掉 | B-059 | 第 3 步 core-2 批次 | P2 | 不改产品行为，只动 `lib.rs`，要编译和跑测试，批次流程正好覆盖。它是主机库改动的回归网，所以排在 B-064 之前。 |
| 4 | RDP 重连 fd 泄漏 | B-050 | 第 3 步 rdp-2 批次 | P2 | 每次重连漏一个 fd，到进程退出才还；fd 上限已提到 10240，短期撞不到。改动只在 `stats.rs`，能单测。 |
| 5 | git watcher 测试 `#[ignore]` 后没人跑 | B-055 | 第 3 步 core-1 批次 | P3 | 只是测试没接进验证入口，产品行为没问题。在 `scripts/validate.sh` 里补一步即可。 |

### 贯穿各条的口径

- **只修确认的正确性问题。** 效率、简化、风格类的不动代码：B-004、B-006、B-039、B-057、B-058 都因此从「修代码」改成「保留」，注释有错的只改注释。例外是 B-053，纯旧名兼容别名，按「没有必要不写兼容代码」删。
- **B-056 不修。** `history_suggester` 整个模块没接线，是死代码。
- **过期的 C 里引用 Warp 行号的**：能对上符号的改成「文件 + 符号」，不写行号；对不上符号、只剩一个行号的整句删。
- **C-166 / C-167 / C-168 先不改**，等 B-063 修完按实际行为一起改。
- **C-056、C-065 里「无自由函数」不改。** 那是约定，不符的是代码。

## 批次

| 批次 | 路径 | 注释组 | A | B | C（其中过期） | D | E | U |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| repo-ci-scripts | `.forgejo/`、`.cargo/`、`scripts/`、`examples/`、`locales/`、根目录文件 | 170 | 61 | 2 | 107（13） | 0 | 0 | 0 |
| views-misc | `src/` 下除 rdp、root_view、编辑器以外的子目录（herdr_bridge、host_management_view、view_components、pty_event_loop、util、settings_view、design_tokens、ui_components、themes、terminal_runtime、platform） | 741 | 93 | 4 | 638（41） | 6 | 0 | 0 |
| root-view | `src/root_view/` | 625 | 27 | 4 | 594（46） | 0 | 0 | 0 |
| rdp-1 | `src/rdp_session/`、`src/rdp_view/` 按文件名排序的第 1/2 段（dib.rs … vimage.rs） | 502 | 6 | 38 | 458（4） | 0 | 0 | 0 |
| rdp-2 | `src/rdp_session/`、`src/rdp_view/` 按文件名排序的第 2/2 段（wire_dump.rs … pointer.rs） | 473 | 18 | 4 | 451（16） | 0 | 0 | 0 |
| core-1 | `src/` 根目录的 .rs 文件 按文件名排序的第 1/3 段（click_through.rs … host_overview.rs） | 578 | 32 | 6 | 540（27） | 0 | 0 | 0 |
| core-2 | `src/` 根目录的 .rs 文件 按文件名排序的第 2/3 段（host_overview_fleet.rs … ssh_key_store.rs） | 498 | 18 | 5 | 464（22） | 11 | 0 | 0 |
| core-3 | `src/` 根目录的 .rs 文件 按文件名排序的第 3/3 段（ssh_password_prompt.rs … warp_tab_context_menu.rs） | 537 | 29 | 3 | 505（22） | 0 | 0 | 0 |
| **合计** | | 4124 | 284 | 66 | 3757（191） | 17 | 0 | 0 |

## A 类分布

按语言：rust 224、yaml 46、bash 10、embedded-script 2、hash-fallback 2。

最集中的 10 个文件：

- `locales/en.yml`：23
- `locales/zh-CN.yml`：23
- `src/host_management_view/key_manager_view.rs`：15
- `src/terminal_grid_element.rs`：14
- `src/terminal_runtime.rs`：11
- `scripts/build-dmg.sh`：9
- `src/host_management_view/host_card.rs`：9
- `src/view_components/action_button.rs`：9
- `src/host_management_view/constants.rs`：8
- `src/host_management_view/group_nav.rs`：8

## 批次 repo-ci-scripts

### B 为捷径辩护

#### B-001 `Cargo.toml:9` `(顶层)`

<!-- entry {"no": "B-001", "cls": "B", "file": "Cargo.toml", "key": "# text_editor 残留 cfg（voice 已砍 / settings 宏内部 integration_tests）：声明以消 check-cfg 警告，默认关、不激活。", "decision": "", "done": ""} -->
> # text_editor 残留 cfg（voice 已砍 / settings 宏内部 integration_tests）：声明以消 check-cfg 警告，默认关、不激活。

- 捷径：为消 check-cfg 警告，声明两个永不启用的空 feature（voice_input、integration_tests），而不是删掉依赖它们的死 cfg 代码
- 豁免：rustc unexpected_cfgs（check-cfg）警告：cfg(feature="voice_input") / cfg(feature="integration_tests") 引用了 Cargo.toml 未声明的 feature；理由：有：text_editor 残留 cfg（voice 已砍 / settings 宏内部 integration_tests），默认关、不激活；去掉后还报不报错：未核实（未编译）。按 rustc 1.94 行为，去掉 Cargo.toml 第 10-11 行后应报 unexpected_cfgs 警告。验证：在副本里删这两行，cargo check --lib 看有无 unexpected_cfgs。
- 是否仍然成立：是。证据：voice_input：src/text_editor/ 下 cfg(feature="voice_input") 引用约 71 处（view/mod.rs 44、element.rs 23、snapshot.rs 4），代码注释自称「voice 已砍」，这些分支是死代码。integration_tests：src/code_editor/view.rs:2363 有 cfg(feature="integration_tests") 的测试辅助 impl；另 ../warp/crates/settings/src/macros.rs:751 的 settings! 宏会在使用方 crate 里展开 cfg(any(test, feature="integration_tests"))，所以这一项去不掉。
- 建议：记待办
- 风险：低：只是留下死 cfg 分支和两个空 feature；风险是别人误开 voice_input 会得到编不过的代码。
- 备注：建议拆成两步：voice_input 在清理 text_editor 死分支后删 feature；integration_tests 因 warp 的 settings 宏展开而必须保留，注释可改成只讲这一条。
- 决定：保留
- 拍板备注：推翻「记待办」。`voice_input` 的 cfg 是 `src/text_editor/` 里的 Warp 上游原文（warp `app/src/editor/view/` 同样有），text_editor 已定为与上游保持一致、不清理，所以这两个空 feature 要一直留着，没有后续动作可记。
- 完成：2026-11-21（保留，不动）

#### B-002 `Cargo.toml:205`–206 `(顶层)`

<!-- entry {"no": "B-002", "cls": "B", "file": "Cargo.toml", "key": "# russh fork（docs/adr/0017）：分支 nexshell-0.46 = v0.46.0 + 上游 #514（接受 SSH-1.99 banner， # 老交换机 OpenSSH 6.x 用）；russh-keys 等兄弟 crate 仍走 crates.io。升级到含 #514 的 russh 后删掉本项。", "decision": "", "done": ""} -->
> # russh fork（docs/adr/0017）：分支 nexshell-0.46 = v0.46.0 + 上游 #514（接受 SSH-1.99 banner，
> # 老交换机 OpenSSH 6.x 用）；russh-keys 等兄弟 crate 仍走 crates.io。升级到含 #514 的 russh 后删掉本项。

- 捷径：用 fork（lion1991/russh 分支 nexshell-0.46，钉 rev 9e2d87f）把 russh 锁在 0.46 并打补丁接受 SSH-1.99，而不升级 russh
- 是否仍然成立：是。证据：Cargo.lock russh 0.46.0 来自 git+lion1991/russh rev 9e2d87fc；fork 提交 9e2d87f「接受 SSH-1.99 版本行（同上游 #514）」。上游 Eugeny/russh#514 已于 2025-04-24 合并（gh api repos/Eugeny/russh/pulls/514：state=closed、merged_at=2025-04-24），上游最新 v0.65.4（2026-11-21）。ADR 0017 与 docs/plans/2026-11-21-upstream-catch-up-backlog.md 已记录这是临时补丁、升级后撤销，等 Matt 拍板是否排期。
- 建议：记待办
- 风险：中：russh 落后上游约 19 个 minor（0.46 → 0.65），缺 2026-05 之后的安全修复，且升级成本随版本差增大（russh-keys 并入 russh、密钥类型换 ssh-key）；注释本身的退出条件写得清楚、无误。
- 备注：待办已存在于 docs/plans/2026-11-21-upstream-catch-up-backlog.md 与 ADR 0017「后续」，注释无需改；此条是「是否排期升级」的提醒。
- 决定：保留
- 拍板备注：推翻「记待办」。升级 russh、撤 fork 已经记在 ADR 0017 和 `docs/plans/2026-11-21-upstream-catch-up-backlog.md`，注释自带 ADR 引用和退出条件，不在 `docs/todo.md` 再登记一份。
- 完成：2026-11-21（保留，不动）

### C 过期的「为什么」

| 编号 | 位置 | 所在符号 | 过期的句子 | 依据 | 建议改成 | 决定 | 完成 |
|---|---|---|---|---|---|---|---|
| C-001 | `.cargo/config.toml:2`–4 | `(顶层)` | launched from sshtool/ or nexshell/ | 仓库目录现为 nexshell（../ 下无 sshtool/ 目录）；rg 'sshtool' 只命中测试夹具 id（src/lib.rs:160 等）。MACOSX_DEPLOYMENT_TARGET=10.14 与 ../warp/.cargo/config.toml:9 仍一致，「保持同步」本身成立。 | 改成「commands are launched from nexshell/」 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-001", "cls": "C", "file": ".cargo/config.toml", "key": "# Keep this in sync with warp/.cargo/config.toml. The nexshell crate # lives outside warp/, so Cargo does not inherit Warp's config when # commands are launched from sshtool/ or nexshell/.", "decision": "", "done": ""} -->
| C-002 | `.gitignore:18` | `(顶层)` | NEXSHELL_RDP_EGFX_DUMP 的 fork dump.rs 会把 env 值当目录写入 CWD | 当前 fork rev 538af770 的 crates/ironrdp-egfx/src 下没有 dump.rs、也没有读该环境变量（~/.cargo/git/checkouts/ironrdp-3a5c63e669e4aaee/538af77，rg 无命中）；ADR 0011:38 记录 raw-gfx-pdu-dump 补丁在 2026-08 追平时被延后、未移植，patches/ironrdp-egfx-raw-gfx-pdu-dump.patch 现在没被 fork 引用。NEXSHELL_RDP_EGFX_DUMP 目前只由本仓库 src/rdp_session/egfx/decoder_vt.rs:82 读取，写 {seq:04}_{label}_{len}B.bin，只对应 *_avc420_*B.bin 这一条模式；egfx*.log、meta.jsonl、*_progressive_*、*_clearcodec_* 四条模式在仓库内已无产出者（rg 无命中）。 | RDP/EGFX 调试产物（NEXSHELL_RDP_EGFX_DUMP 设为相对目录时，decoder_vt 会把 *_avc420_*B.bin 写进 CWD）。另外 egfx*.log / meta.jsonl / progressive / clearcodec 四条模式的生产者已不存在，确认本地没有旧产物后可一并删除。 | 按建议改：注释只改成「RDP/EGFX 调试产物（NEXSHELL_RDP_EGFX_DUMP 设为相对目录时，decoder_vt 会把 `*_avc420_*B.bin` 写进 CWD）」；建议里「另外……可一并删除」那句不进注释，ignore 模式不动 | 2026-11-21 |<!-- entry {"no": "C-002", "cls": "C", "file": ".gitignore", "key": "# RDP/EGFX 调试产物（NEXSHELL_RDP_EGFX_DUMP 的 fork dump.rs 会把 env 值当目录写入 CWD）", "decision": "", "done": ""} -->
| C-003 | `Cargo.toml:26`–27 | `(顶层)` | 与 Warp `grid_renderer.rs:653` 调 `width()` 的语义一致 | ../warp/app/src/terminal/grid_renderer.rs 现在 marked text 的 width() 调用在第 672 行和 1243 行，653 行是别的代码（Skip the cursor cell 注释之后）。同类行号引用还在 src/terminal_runtime.rs:2012（写 639-695）。 | 去掉行号：「与 Warp grid_renderer.rs 里 marked text 字符调 width() 的语义一致」（行号随 warp 追平必漂移） | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-003", "cls": "C", "file": "Cargo.toml", "key": "# 用于判断 IME marked text 字符的列宽（CJK 占 2 cell），与 Warp # `grid_renderer.rs:653` 调 `width()` 的语义一致。", "decision": "", "done": ""} -->
| C-004 | `Cargo.toml:37` | `(顶层)` | 版本对齐 warp | ../warp/Cargo.toml:155 enum-iterator = "1.1.3"（warp Cargo.lock 解析 1.4.1）、lazy_static = "1.4.0"（:199）；本文件写 1.5 / 1.5，并不「对齐」，Cargo.lock 解析 enum-iterator 1.5.0。功能依赖（derive(Sequence)）本身成立，src/util/bindings.rs:4,12,129。 | 删掉「版本对齐 warp」，只留「util::bindings 本地化所需：CustomAction derive(Sequence) + CUSTOM_TAG_TO_ACTION」 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-004", "cls": "C", "file": "Cargo.toml", "key": "# util::bindings 本地化所需：CustomAction derive(Sequence) + CUSTOM_TAG_TO_ACTION，版本对齐 warp。", "decision": "", "done": ""} -->
| C-005 | `Cargo.toml:68`–70 | `(顶层)` | 0.6.0 依赖 pdu 0.8/svc 0.7/core 0.2，与树内 connector 0.9/session 0.10 完全一致；自实现 CliprdrBackend + arboard 访问 Mac 剪贴板；见 docs/adr/0007 第⑤步 | Cargo.toml:71 现为 ironrdp-cliprdr = "0.7"（Cargo.lock 0.7.0，依赖 pdu 0.9 / svc 0.8 / core 0.2，见 fork crates/ironrdp-cliprdr/Cargo.toml:25-27），树内 connector 0.10 / session 0.11；macOS 剪贴板已改用 objc2-app-kit NSPasteboard（Cargo.toml:132-136，ADR 0016），arboard 只在非 macOS（Cargo.toml:141，src/rdp_session/clipboard/local_other.rs）；docs/adr/0007-rdp-via-ironrdp.md 只有 31 行、没有「第⑤步」，git 历史（-S'第⑤步'）也查不到。 | CLIPRDR 剪贴板通道（见 docs/adr/0016）。cliprdr-native 只有 windows 后端、macOS 仅 stub，故不引 native，自实现 CliprdrBackend；macOS 侧用 NSPasteboard，其他平台用 arboard。（版本对应关系删掉，Cargo.lock 已记录） | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-005", "cls": "C", "file": "Cargo.toml", "key": "# CLIPRDR 剪贴板通道（见 docs/adr/0007 第⑤步）。0.6.0 依赖 pdu 0.8/svc 0.7/core 0.2， # 与树内 connector 0.9/session 0.10 完全一致。cliprdr-native 只有 windows 后端、macOS 仅 stub， # 故不引 native，自实现 CliprdrBackend + arboard 访问 Ma", "decision": "", "done": ""} -->
| C-006 | `Cargo.toml:72`–73 | `(顶层)` | dvc 0.7 承载动态虚拟通道，egfx 0.2 是 EGFX DVC 处理器 | Cargo.toml:74-75 现为 ironrdp-dvc 0.8、ironrdp-egfx 0.3，Cargo.lock 同为 0.8.0 / 0.3.0。「不开 openh264 feature」仍成立（Cargo.toml:75 无 features；fork egfx 的 openh264 为可选 feature）。 | EGFX 图形管线（MS-RDPEGFX，见 docs/adr/0008）。dvc 承载动态虚拟通道，egfx 是 EGFX DVC 处理器。不开 openh264 feature（解码走 VideoToolbox）。 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-006", "cls": "C", "file": "Cargo.toml", "key": "# EGFX 图形管线（MS-RDPEGFX，见 docs/adr/0008）。dvc 0.7 承载动态虚拟通道， # egfx 0.2 是 EGFX DVC 处理器。不开 openh264 feature（解码走 VideoToolbox，第②步）。", "decision": "", "done": ""} -->
| C-007 | `Cargo.toml:77`–78 | `(顶层)` | 0.7.0 依赖 dvc 0.7/svc 0.7/pdu 0.8/core 0.2，与树内一致 | Cargo.toml:79 现为 ironrdp-displaycontrol = "0.8"（Cargo.lock 0.8.0，依赖 dvc 0.8 / svc 0.8 / pdu 0.9 / core 0.2，见 fork crates/ironrdp-displaycontrol/Cargo.toml:20-23）。 | 删掉版本对应那句，保留「窗口尺寸/全屏变化时实时重设远端分辨率，消除 letterbox 黑边」 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-007", "cls": "C", "file": "Cargo.toml", "key": "# Display Control 动态通道（MS-RDPEDISP，见 docs/adr/0008）：窗口尺寸/全屏变化时 # 实时重设远端分辨率，消除 letterbox 黑边。0.7.0 依赖 dvc 0.7/svc 0.7/pdu 0.8/core 0.2，与树内一致。", "decision": "", "done": ""} -->
| C-008 | `Cargo.toml:80`–81 | `(顶层)` | rdpsnd 0.8 协议层 + rdpsnd-native 0.6 cpal 播放后端；与树内 pdu 0.8/svc 0.7/core 0.2 一致 | Cargo.toml:82-83 现为 ironrdp-rdpsnd 0.9、ironrdp-rdpsnd-native 0.7（Cargo.lock 0.9.0 / 0.7.0）；fork rdpsnd 依赖 pdu 0.9 / svc 0.8。「默认 feature 含 Opus」仍成立（fork crates/ironrdp-rdpsnd-native/Cargo.toml:19）。 | 去掉所有版本号：rdpsnd 协议层 + rdpsnd-native 的 cpal 播放后端（默认 feature 含 Opus 解码）。 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-008", "cls": "C", "file": "Cargo.toml", "key": "# RDPSND 音频重定向（仅输出方向）：rdpsnd 0.8 协议层 + rdpsnd-native 0.6 cpal 播放后端 # （默认 feature 含 Opus 解码）。与树内 pdu 0.8/svc 0.7/core 0.2 一致。", "decision": "", "done": ""} -->
| C-009 | `Cargo.toml:96`–97 | `(顶层)` | git 面板用：监听仓库 .git/ 与工作区变化触发自动刷新 | notify_debouncer_full 现在有两个使用方：src/git_panel.rs:20 与 src/file_panel.rs:13。「用 upstream crates.io 版本、不引 fork」仍成立（Cargo.lock notify-debouncer-full 0.3.2 registry；../warp/Cargo.toml:208 用的是 warpdotdev/notify fork）。 | git 面板 / 文件面板用：监听目录变化触发自动刷新。Warp 同款思路，用 upstream crates.io 版本即可，不引入 fork。 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-009", "cls": "C", "file": "Cargo.toml", "key": "# git 面板用：监听仓库 .git/ 与工作区变化触发自动刷新。Warp 同款思路， # 用 upstream crates.io 版本即可，不引入 fork。", "decision": "", "done": ""} -->
| C-010 | `Cargo.toml:109` | `(顶层)` | mod.rs 用 mime_guess/async-fs | rg 'mime_guess\|async_fs\|async-fs' src 无任何命中（只有 view/mod.rs:1365 的字段 mime_type）；git log -S'async_fs' -- src 最近一次变动是 a54b498（text_editor 本地化里程碑1），此后源码中已无引用。concat-idents 由 ../warp/crates/settings/src/macros.rs:204,410 的宏展开使用，cfg-if 见 src/text_editor/view/element.rs:437，这两项仍成立。 | editor 本地化补依赖：settings 宏裸用 concat_idents::；element 用 cfg-if。（mime_guess / async-fs 已无引用，先用 cargo udeps 或删后 cargo check 核实，再连依赖一起删） | 按建议改：注释去掉「mod.rs 用 mime_guess/async-fs」，同一提交删掉这两个依赖（两行，加 warpui-app feature 里的两项）；以 cargo check（含 Windows 目标）通过为准，过不了就恢复依赖、这条不改 | 2026-11-21 |<!-- entry {"no": "C-010", "cls": "C", "file": "Cargo.toml", "key": "# editor 本地化补依赖：settings 宏裸用 concat_idents::；element/settings 用 cfg-if；mod.rs 用 mime_guess/async-fs。", "decision": "", "done": ""} -->
| C-011 | `Cargo.toml:154`–155 | `(顶层)` | this spike | Cargo.toml:3 version = "0.6.13"，已发布 v0.6.x，不再是 spike；整句「不需要把本 crate 加为 Warp workspace 成员」仍成立。 | Warp UI crates are consumed as path dependencies so the Warp workspace does not need to include NexShell as a member. | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-011", "cls": "C", "file": "Cargo.toml", "key": "# Warp UI crates are consumed as path dependencies so the Warp workspace does # not need to include this spike as a member.", "decision": "", "done": ""} -->
| C-012 | `Cargo.toml:187` | `(顶层)` | rdp_probe 探针把 framebuffer 落盘成 PNG 验证 | dev-dependency image 现在有两个使用方：examples/rdp_probe.rs:111,518 与 examples/egfx_replay.rs:81（image::save_buffer_with_format）。 | examples（rdp_probe / egfx_replay）把 framebuffer 落盘成 PNG 验证（仅验证用，运行期喂 warpui RGBA）。 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-012", "cls": "C", "file": "Cargo.toml", "key": "# rdp_probe 探针把 framebuffer 落盘成 PNG 验证（仅验证用，运行期喂 warpui RGBA）。", "decision": "", "done": ""} -->
| C-013 | `Cargo.toml:195`–197 | `(顶层)` | this spike is intentionally outside the Warp workspace | 同 0041：Cargo.toml:3 已是 0.6.13 正式版本。其余成立：../warp/Cargo.toml:568-571 把 core-foundation / core-graphics / core-text 钉到同一 rev 6f844cf1，本文件 198-201 行相同；Cargo.lock 的 core-graphics 0.25.0 来自该 git 源。「E0308 两个 core-graphics 版本」未编译复现，属原作者记录。 | Keep Warp's patched CoreText/CoreGraphics stack active even though NexShell is intentionally outside the Warp workspace. ……（其余不变） | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-013", "cls": "C", "file": "Cargo.toml", "key": "# Keep Warp's patched CoreText/CoreGraphics stack active even though this # spike is intentionally outside the Warp workspace. Without this, font-kit # sees CGFont from two core-graphics versions and ", "decision": "", "done": ""} -->

## 批次 views-misc

### B 为捷径辩护

#### B-003 `src/pty_event_loop/mod.rs:62`–70 `(顶层)`

<!-- entry {"no": "B-003", "cls": "B", "file": "src/pty_event_loop/mod.rs", "key": "// Note: `warp/app/src/terminal/local_tty/event_loop.rs:365` passes // `state.parser.sync_output_remaining_timeout()` to `mio::Poll::poll` // so it can call `finish_sync_output(...)` after BSU/ESU's i", "decision": "", "done": ""} -->
> // Note: `warp/app/src/terminal/local_tty/event_loop.rs:365` passes
> // `state.parser.sync_output_remaining_timeout()` to `mio::Poll::poll`
> // so it can call `finish_sync_output(...)` after BSU/ESU's internal
> // timeout. vte 0.15 doesn't expose that timeout (it's all hidden behind
> // `Processor::advance`'s own state machine), so we pass `None` and let
> // vte's internal hard timeout drive sync-output stop. The trade-off is
> // that a program that emits BSU then immediately stops sending bytes
> // will hold rendering until vte's internal timeout (~150 ms) — same
> // upper bound as Warp, just without the externally-visible early stop.

- 捷径：不接 vte 的同步输出（BSU/ESU）超时：poll 永远传 None，靠注释所称「vte 内部硬超时 ~150ms」兜底
- 是否仍然成立：否。证据：vte 0.15.0 确实暴露了超时：~/.cargo/registry/src/*/vte-0.15.0/src/ansi.rs:292 `Processor::sync_timeout()`，:451 `StdSyncHandler::sync_timeout() -> Option&lt;Instant>`，:315 `stop_sync()`。但 Processor 自己不会按时间结束同步：advance/advance_sync(:361-385) 只在遇到 ESU 或缓冲达到 SYNC_BUFFER_SIZE(2MB，:39) 时结束，150ms 超时(:36)只是交给调用方去 poll 并调 stop_sync。nexshell 实际用的是 alacritty_terminal 0.25 → vte 0.15.0（Cargo.lock:11372；terminal_runtime.rs:1428 `parser: ansi::Processor`）。全仓 rg 无 sync_timeout / stop_sync 调用；pty_event_loop/mod.rs:470-480 poll(None)。Warp 对应实现 ../warp crates/warp_terminal/src/local_tty/event_loop.rs:374-385 是 poll 超时后 finish_sync_output。
- 建议：修代码
- 风险：中：程序发出 ?2026h 后未发 ?2026l 且之后没有输出（崩溃/被 kill/Ctrl+C 打断）时，该 tab 画面会一直停住，而不是 ~150ms 恢复；注释给的上限是错的。
- 备注：修法：TerminalRuntimeState 暴露 parser.sync_timeout().sync_timeout() 剩余时间作为 poll 超时，超时后调 parser.stop_sync(&amp;mut term)。同时参见 0478、0481。
- 决定：修代码
- 拍板备注：确认是 bug，走单独的 bug 修复，不在 views-misc 批次里做（见拍板记录）。vte 0.15 的 `pending_timeout()` 只看有没有设过超时，不会自己到期，nexshell 又从不调 `stop_sync`：收到 `?2026h` 后没等到 `?2026l`，后续输出（包括按键回显）全被缓冲，攒满 2 MiB 才放出来。修复要同时覆盖本地 PTY 事件循环和远程（SSH / 串口 / Telnet）读循环。回归用例：喂 BSU 不喂 ESU，超时后画面应刷出。修完这条注释按实际实现重写，完成栏随修复提交填。
- 完成：2026-11-21（随同步输出超时修复提交）

#### B-004 `src/pty_event_loop/mod.rs:376`–379 `EventLoop.pty_read`

<!-- entry {"no": "B-004", "cls": "B", "file": "src/pty_event_loop/mod.rs", "key": "// vte 0.15 does not expose a sync_output buffer length, so we // wake the UI any time we processed bytes — equivalent to // Warp's `\u003e sync_output_buffer_len.unwrap_or(0)` when the inner // value is `", "decision": "", "done": ""} -->
> // vte 0.15 does not expose a sync_output buffer length, so we
> // wake the UI any time we processed bytes — equivalent to
> // Warp's `> sync_output_buffer_len.unwrap_or(0)` when the inner
> // value is `None`.

- 捷径：不区分同步缓冲，只要处理过字节就唤醒 UI（等价于 Warp 的 sync_output_buffer_len=None 分支）
- 是否仍然成立：否。证据：前提「vte 0.15 does not expose a sync_output buffer length」不成立：vte-0.15.0/src/ansi.rs:361 有 pub `Processor::sync_bytes_count()`。Warp 对应逻辑：../warp crates/warp_terminal/src/local_tty/event_loop.rs:280 `bytes_processed > sync_output_buffer_len().unwrap_or(0)`。
- 建议：修代码
- 风险：低：同步更新期间（缓冲未结束）也会白白唤醒 UI 重绘；画面不会错，只是多余唤醒。
- 备注：与 0437 一起改（要把 sync_bytes_count 经 PtySink 暴露出来）。
- 决定：保留
- 拍板备注：推翻「修代码」。只是同步缓冲期间多唤醒一次 UI，画面不会错，属效率类，不动代码。注释的前提是错的（vte 0.15 有 `sync_bytes_count()`），改成如实描述：缓冲期间也照常唤醒 UI，Warp 按缓冲长度跳过、这里没跳。B-003 修复时如果顺带对齐了 Warp 的判断，这条注释直接删。
- 完成：2026-11-21（随 B-003 修复如实改写，代码不动）

#### B-005 `src/pty_event_loop/mod.rs:470`–472 `EventLoop.run`

<!-- entry {"no": "B-005", "cls": "B", "file": "src/pty_event_loop/mod.rs", "key": "// See `event_loop.rs:363-379`. vte 0.15 hides its sync // state — see the SYNC_FALLBACK_TIMEOUT comment near the // top of this file — so we always wait indefinitely.", "decision": "", "done": ""} -->
> // See `event_loop.rs:363-379`. vte 0.15 hides its sync
> // state — see the SYNC_FALLBACK_TIMEOUT comment near the
> // top of this file — so we always wait indefinitely.

- 捷径：事件循环 poll 永远无限等待，不处理同步输出超时
- 是否仍然成立：否。证据：「vte 0.15 hides its sync state」不成立（见 0437 的 ansi.rs:292/451/315）；注释引用的「SYNC_FALLBACK_TIMEOUT comment near the top」在全仓不存在（rg SYNC_FALLBACK_TIMEOUT 只命中本行），顶部实际是 :62-70 的 Note；`event_loop.rs:363-379` 行号也已漂移（现 crates/warp_terminal/src/local_tty/event_loop.rs:374-385）。
- 建议：修代码
- 风险：中：同 0437，BSU 后无 ESU 时画面停住。
- 决定：修代码
- 拍板备注：同 B-003，一起修。
- 完成：2026-11-21（随 B-003 一起修）

#### B-006 `src/pty_event_loop/mod.rs:636`–637 `spawn_event_loop`

<!-- entry {"no": "B-006", "cls": "B", "file": "src/pty_event_loop/mod.rs", "key": "// EventLoop 在本线程退出后 kill；EventLoopHandle 保留一个独立 killer // 仅为不动 windows 构造/结构体签名（unix Drop 已不再用它）。", "decision": "", "done": ""} -->
> // EventLoop 在本线程退出后 kill；EventLoopHandle 保留一个独立 killer
> // 仅为不动 windows 构造/结构体签名（unix Drop 已不再用它）。

- 捷径：为了不改 Windows 分支的构造函数/结构体签名，EventLoopHandle 里留着一个 unix 下已不使用的 killer 字段
- 是否仍然成立：是。证据：属实：pty_event_loop/mod.rs:155 `pub killer` 全仓无读取（rg `\.killer` 只在 :566 读 EventLoop.killer，另有 :664 构造赋值）；unix 的 spawn_event_loop 第 638 行为此额外 clone_killer。Windows 分支(:660-675 / :810)仍把 killer 存入 handle，但同样无人读取。
- 建议：修代码
- 风险：低：多一个无用字段与一次 clone_killer，签名里的 killer 参数名存实亡；不影响运行。
- 备注：需要 Windows 目标才能确认移除无副作用：`cargo check --target x86_64-pc-windows-gnu`（本次未跑）。
- 决定：保留
- 拍板备注：推翻「修代码」。只是多一个没人读的字段和一次 clone_killer，属简化类，不动代码。
- 完成：2026-11-21（保留，不动）

### C 过期的「为什么」

| 编号 | 位置 | 所在符号 | 过期的句子 | 依据 | 建议改成 | 决定 | 完成 |
|---|---|---|---|---|---|---|---|
| C-014 | `src/host_management_view/status_view.rs:1`–3 | `(顶层)` | 点击卡片 = 快速连接该主机 | status_view.rs:274-299 与同文件 :274 注释：只有右上角终端快连按钮 on_click 派发 HostQuickConnect，「卡片其他区域不响应」；整卡 Hoverable(:161) 没有 on_click | 点击右上角终端快连按钮 = 快速连接该主机（卡片其他区域不响应） | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-014", "cls": "C", "file": "src/host_management_view/status_view.rs", "key": "// 多主机状态总览卡片网格（ServerCat 风格）。本文件只放纯渲染自由函数。 // 数据来自 RootView 的 HostOverviewFleet；点击卡片 = 快速连接该主机。 // CPU/Mem 用 RingGauge 环形仪表（GPU Ring 原语），sweep 走 FloatTransition 过渡。", "decision": "", "done": ""} -->
| C-015 | `src/pty_event_loop/mio_channel.rs:3`–4 | `(顶层)` | `warp/app/src/terminal/local_tty/mio_channel.rs` | ../warp 当前 HEAD(5bf047181) 中 warp/app/src/terminal/local_tty/event_loop.rs 与 mio_channel.rs 已不存在，迁到 crates/warp_terminal/src/local_tty/ 下（mio_channel.rs 现为 crates/warp_terminal/src/local_tty/mio_channel.rs） | 改成 `warp/crates/warp_terminal/src/local_tty/mio_channel.rs`，或只写文件名不带路径 | 按建议改：写完整路径 `warp/crates/warp_terminal/src/local_tty/mio_channel.rs` | 2026-11-21 |<!-- entry {"no": "C-015", "cls": "C", "file": "src/pty_event_loop/mio_channel.rs", "key": "//! Vendored verbatim (with module path adjustments) from Warp:", "decision": "", "done": ""} -->
| C-016 | `src/pty_event_loop/mio_channel.rs:4`–5 | `(顶层)` | `warp/app/src/terminal/local_tty/mio_channel.rs` | ../warp 当前 HEAD(5bf047181) 中 warp/app/src/terminal/local_tty/event_loop.rs 与 mio_channel.rs 已不存在，迁到 crates/warp_terminal/src/local_tty/ 下 | 同上，路径改为 crates/warp_terminal/src/local_tty/mio_channel.rs | 按建议改：写完整路径 `warp/crates/warp_terminal/src/local_tty/mio_channel.rs` | 2026-11-21 |<!-- entry {"no": "C-016", "cls": "C", "file": "src/pty_event_loop/mio_channel.rs", "key": "//! `warp/app/src/terminal/local_tty/mio_channel.rs`. The same trick the", "decision": "", "done": ""} -->
| C-017 | `src/pty_event_loop/mod.rs:3`–4 | `(顶层)` | `warp/app/src/terminal/local_tty/event_loop.rs`（及 mio_channel.rs）路径 | ../warp 当前 HEAD(5bf047181) 中 warp/app/src/terminal/local_tty/event_loop.rs 与 mio_channel.rs 已不存在，迁到 crates/warp_terminal/src/local_tty/ 下 | 路径改为 `warp/crates/warp_terminal/src/local_tty/event_loop.rs` | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-017", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "//! This module is a one-to-one Rust port of", "decision": "", "done": ""} -->
| C-018 | `src/pty_event_loop/mod.rs:4`–5 | `(顶层)` | `warp/app/src/terminal/local_tty/event_loop.rs`（及 mio_channel.rs）路径 | ../warp 当前 HEAD(5bf047181) 中 warp/app/src/terminal/local_tty/event_loop.rs 与 mio_channel.rs 已不存在，迁到 crates/warp_terminal/src/local_tty/ 下 | 路径改为 `warp/crates/warp_terminal/src/local_tty/event_loop.rs` | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-018", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "//! `warp/app/src/terminal/local_tty/event_loop.rs` (and its sibling", "decision": "", "done": ""} -->
| C-019 | `src/pty_event_loop/mod.rs:5`–6 | `(顶层)` | `warp/app/src/terminal/local_tty/event_loop.rs`（及 mio_channel.rs）路径 | ../warp 当前 HEAD(5bf047181) 中 warp/app/src/terminal/local_tty/event_loop.rs 与 mio_channel.rs 已不存在，迁到 crates/warp_terminal/src/local_tty/ 下 | 路径改为 `warp/crates/warp_terminal/src/local_tty/event_loop.rs` | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-019", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "//! `mio_channel.rs`), adapted to drive a [`portable_pty`] master file", "decision": "", "done": ""} -->
| C-020 | `src/pty_event_loop/mod.rs:21`–22 | `mio_channel` | `view.rs:644` | ../warp app/src/terminal/view.rs:616 才是 WAKEUP_THROTTLE_PERIOD 定义，:3763 使用；:644 已漂移 | 改成 `app/src/terminal/view.rs` 的 WAKEUP_THROTTLE_PERIOD，不写行号 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-020", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "//! (`view.rs:644`).", "decision": "", "done": ""} -->
| C-021 | `src/pty_event_loop/mod.rs:54`–55 | `(顶层)` | `event_loop.rs:28` | READ_BUFFER_SIZE 现在在 crates/warp_terminal/src/local_tty/event_loop.rs:26；原路径文件已迁移 | 改成 `crates/warp_terminal/src/local_tty/event_loop.rs` 的 READ_BUFFER_SIZE，不写行号 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-021", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "/// Mirrors `warp/app/src/terminal/local_tty/event_loop.rs:28`.", "decision": "", "done": ""} -->
| C-022 | `src/pty_event_loop/mod.rs:58`–59 | `(顶层)` | `event_loop.rs:32` | MAX_LOCKED_READ 现在在 crates/warp_terminal/src/local_tty/event_loop.rs:30；原路径文件已迁移 | 同上，指向 MAX_LOCKED_READ | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-022", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "/// Mirrors `warp/app/src/terminal/local_tty/event_loop.rs:32`.", "decision": "", "done": ""} -->
| C-023 | `src/pty_event_loop/mod.rs:160`–162 | `EventLoopHandle.drop` | `terminal_manager.rs:189-205 shutdown_event_loop` | ../warp app/src/terminal/local_tty/terminal_manager.rs:553-571 才是 shutdown_event_loop（只发 Shutdown + join，结论仍成立），:189-205 已漂移 | 改成 `terminal_manager.rs 的 shutdown_event_loop`，不写行号 | 按建议改，并更正后半句：子进程还活着时，是事件循环线程退出前把它 kill 掉的（EventLoop::run 末尾，同 Warp event_loop 的 `self.pty.kill()`），不是只靠关 master fd 的 SIGHUP | 2026-11-21 |<!-- entry {"no": "C-023", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "// 1:1 与 Warp `terminal_manager.rs:189-205 shutdown_event_loop` 对齐： // 只发 Shutdown + join，不自行 kill。子进程由 EventLoop drop 时关闭 // master fd → 内核给 slave 进程组发 SIGHUP 隐式回收（同 Warp）。", "decision": "", "done": ""} -->
| C-024 | `src/pty_event_loop/mod.rs:176`–177 | `State` | `warp/app/src/terminal/local_tty/event_loop.rs:66` | ../warp 当前 HEAD(5bf047181) 中 warp/app/src/terminal/local_tty/event_loop.rs 与 mio_channel.rs 已不存在，迁到 crates/warp_terminal/src/local_tty/ 下；State 现在在 :68 | 改成 `warp_terminal::local_tty::event_loop::State` | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-024", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "/// `EventLoop::State` from `warp/app/src/terminal/local_tty/event_loop.rs:66`.", "decision": "", "done": ""} -->
| C-025 | `src/pty_event_loop/mod.rs:312`–313 | `EventLoop` | `event_loop.rs:196 fn pty_read` | pty_read 现在在 crates/warp_terminal/src/local_tty/event_loop.rs:199 | 改成 `event_loop.rs 的 fn pty_read`，不写行号 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-025", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "/// Mirrors `event_loop.rs:196 fn pty_read`. The lock acquisition", "decision": "", "done": ""} -->
| C-026 | `src/pty_event_loop/mod.rs:387`–388 | `EventLoop` | `event_loop.rs:279 fn pty_write` | pty_write 现在在 crates/warp_terminal/src/local_tty/event_loop.rs:288 | 改成 `event_loop.rs 的 fn pty_write`，不写行号 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-026", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "/// Mirrors `event_loop.rs:279 fn pty_write`.", "decision": "", "done": ""} -->
| C-027 | `src/pty_event_loop/mod.rs:437`–438 | `EventLoop.run` | `event_loop.rs:342-350` | 对应的 poll 注册现在在 crates/warp_terminal/src/local_tty/event_loop.rs:350 附近（spawn 内），原行号已漂移 | 去掉行号 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-027", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "// Channel + PTY + SIGCHLD all share the same Poll, just like // `event_loop.rs:342-350`.", "decision": "", "done": ""} -->
| C-028 | `src/pty_event_loop/mod.rs:525`–528 | `EventLoop.run` | `event_loop.rs:447` | EIO 处理现在在 crates/warp_terminal/src/local_tty/event_loop.rs:458 附近 | 去掉行号，或改成 :458 | 按建议改：去掉行号，不改成 :458 | 2026-11-21 |<!-- entry {"no": "C-028", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "// Linux: `read` on the leader side can return // EIO if the follower hangs up. Warp's // `event_loop.rs:447` swallows it and waits for // SIGCHLD. We do the same.", "decision": "", "done": ""} -->
| C-029 | `src/pty_event_loop/mod.rs:564` | `EventLoop.run` | Warp `event_loop.rs:471-477` | 对应的 `if !child_exited { self.pty.kill() }` 现在在 crates/warp_terminal/src/local_tty/event_loop.rs:487-492 | 去掉行号 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-029", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "// 非本进程发起关闭时，在本线程终止 PTY 进程（Warp `event_loop.rs:471-477`）。", "decision": "", "done": ""} -->
| C-030 | `src/pty_event_loop/mod.rs:569` | `EventLoop.run` | `event_loop.rs:479` | 最终 `terminal.lock().exit(ExitReason::PtyDisconnected)` 现在在 crates/warp_terminal/src/local_tty/event_loop.rs:494 | 去掉行号 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-030", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "// Final notification mirrors `event_loop.rs:479`.", "decision": "", "done": ""} -->
| C-031 | `src/pty_event_loop/mod.rs:579`–580 | `spawn_event_loop` | `warp/app/src/terminal/local_tty/terminal_manager.rs:380` | event_loop.spawn() 现在在 ../warp app/src/terminal/local_tty/terminal_manager.rs:896 | 去掉行号 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-031", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "/// from `warp/app/src/terminal/local_tty/terminal_manager.rs:380`.", "decision": "", "done": ""} -->
| C-032 | `src/pty_event_loop/mod.rs:603`–607 | `spawn_event_loop` | `event_listener.rs:11` | 合并 wakeup 的注释现在在 ../warp crates/warp_terminal/src/event_listener.rs:19；app/src/terminal/local_tty/ 下没有 event_listener.rs | 改成 `crates/warp_terminal/src/event_listener.rs` | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-032", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "// Bounded(1) wakeup channel — coalesce multiple wakeups into one, // since the UI throttles at 60 Hz anyway. `async_channel::bounded(1) // .try_send` returns Full quietly when the slot is occupied, w", "decision": "", "done": ""} -->
| C-033 | `src/pty_event_loop/mod.rs:829` | `set_nonblocking` | `warp/app/src/terminal/local_tty/unix.rs:543` | set_nonblocking 现在在 ../warp crates/warp_terminal/src/local_tty/unix.rs:756；原路径不存在 | 改成 `crates/warp_terminal/src/local_tty/unix.rs` 的 set_nonblocking | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-033", "cls": "C", "file": "src/pty_event_loop/mod.rs", "key": "// Same trick as `warp/app/src/terminal/local_tty/unix.rs:543`.", "decision": "", "done": ""} -->
| C-034 | `src/settings_view/appearance_page.rs:128` | `render_appearance_page` | `warp: appearance_page.rs:2630-2713` | ../warp app/src/settings_view/appearance_page.rs 当前 5679 行，锚点已漂移："Current theme" 在 :2898，"Reset to default" 在 :4332，"View all available system fonts" 在 :4406，注释里写的 2630/3909/3861 等行号对不上 | 删掉行号，只留 `warp: appearance_page.rs` 与区块名（如 Text / Reset to default）；需要定位让读者在 warp 里搜区块名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-034", "cls": "C", "file": "src/settings_view/appearance_page.rs", "key": "// --- Theme --- warp: appearance_page.rs:2630-2713", "decision": "", "done": ""} -->
| C-035 | `src/settings_view/appearance_page.rs:216` | `render_appearance_page` | `warp: appearance_page.rs:3909-4054` | ../warp app/src/settings_view/appearance_page.rs 当前 5679 行，锚点已漂移："Current theme" 在 :2898，"Reset to default" 在 :4332，"View all available system fonts" 在 :4406，注释里写的 2630/3909/3861 等行号对不上 | 删掉行号，只留 `warp: appearance_page.rs` 与区块名（如 Text / Reset to default）；需要定位让读者在 warp 里搜区块名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-035", "cls": "C", "file": "src/settings_view/appearance_page.rs", "key": "// --- Text --- warp: appearance_page.rs:3909-4054", "decision": "", "done": ""} -->
| C-036 | `src/settings_view/appearance_page.rs:377` | `render_text_section` | `warp: appearance_page.rs:3909-4054` | ../warp app/src/settings_view/appearance_page.rs 当前 5679 行，锚点已漂移："Current theme" 在 :2898，"Reset to default" 在 :4332，"View all available system fonts" 在 :4406，注释里写的 2630/3909/3861 等行号对不上 | 删掉行号，只留 `warp: appearance_page.rs` 与区块名（如 Text / Reset to default）；需要定位让读者在 warp 里搜区块名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-036", "cls": "C", "file": "src/settings_view/appearance_page.rs", "key": "// warp: appearance_page.rs:3909-4054 — Text 区块四列布局", "decision": "", "done": ""} -->
| C-037 | `src/settings_view/appearance_page.rs:397` | `render_text_section` | `appearance_page.rs:3927-3971` | ../warp app/src/settings_view/appearance_page.rs 当前 5679 行，锚点已漂移："Current theme" 在 :2898，"Reset to default" 在 :4332，"View all available system fonts" 在 :4406，注释里写的 2630/3909/3861 等行号对不上 | 删掉行号，只留 `warp: appearance_page.rs` 与区块名（如 Text / Reset to default）；需要定位让读者在 warp 里搜区块名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-037", "cls": "C", "file": "src/settings_view/appearance_page.rs", "key": "// (1) Terminal font — warp: appearance_page.rs:3927-3971", "decision": "", "done": ""} -->
| C-038 | `src/settings_view/appearance_page.rs:416` | `render_text_section` | `appearance_page.rs:3932-3968` | ../warp app/src/settings_view/appearance_page.rs 当前 5679 行，锚点已漂移："Current theme" 在 :2898，"Reset to default" 在 :4332，"View all available system fonts" 在 :4406，注释里写的 2630/3909/3861 等行号对不上 | 删掉行号，只留 `warp: appearance_page.rs` 与区块名（如 Text / Reset to default）；需要定位让读者在 warp 里搜区块名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-038", "cls": "C", "file": "src/settings_view/appearance_page.rs", "key": "// warp: appearance_page.rs:3932-3968 — \"View all available system fonts\" checkbox", "decision": "", "done": ""} -->
| C-039 | `src/settings_view/appearance_page.rs:456` | `render_text_section` | `appearance_page.rs:3988-3992` | ../warp app/src/settings_view/appearance_page.rs 当前 5679 行，锚点已漂移："Current theme" 在 :2898，"Reset to default" 在 :4332，"View all available system fonts" 在 :4406，注释里写的 2630/3909/3861 等行号对不上 | 删掉行号，只留 `warp: appearance_page.rs` 与区块名（如 Text / Reset to default）；需要定位让读者在 warp 里搜区块名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-039", "cls": "C", "file": "src/settings_view/appearance_page.rs", "key": "// (2) Font weight — warp: appearance_page.rs:3988-3992", "decision": "", "done": ""} -->
| C-040 | `src/settings_view/appearance_page.rs:477` | `render_text_section` | `appearance_page.rs:3996-4050` | ../warp app/src/settings_view/appearance_page.rs 当前 5679 行，锚点已漂移："Current theme" 在 :2898，"Reset to default" 在 :4332，"View all available system fonts" 在 :4406，注释里写的 2630/3909/3861 等行号对不上 | 删掉行号，只留 `warp: appearance_page.rs` 与区块名（如 Text / Reset to default）；需要定位让读者在 warp 里搜区块名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-040", "cls": "C", "file": "src/settings_view/appearance_page.rs", "key": "// (3) Font size (px) — warp: appearance_page.rs:3996-4050", "decision": "", "done": ""} -->
| C-041 | `src/settings_view/appearance_page.rs:513` | `render_text_section` | `appearance_page.rs:3806-3893` | ../warp app/src/settings_view/appearance_page.rs 当前 5679 行，锚点已漂移："Current theme" 在 :2898，"Reset to default" 在 :4332，"View all available system fonts" 在 :4406，注释里写的 2630/3909/3861 等行号对不上 | 删掉行号，只留 `warp: appearance_page.rs` 与区块名（如 Text / Reset to default）；需要定位让读者在 warp 里搜区块名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-041", "cls": "C", "file": "src/settings_view/appearance_page.rs", "key": "// (4) Line height — warp: appearance_page.rs:3806-3893", "decision": "", "done": ""} -->
| C-042 | `src/settings_view/appearance_page.rs:545` | `render_text_section` | `appearance_page.rs:3861-3891` | ../warp app/src/settings_view/appearance_page.rs 当前 5679 行，锚点已漂移："Current theme" 在 :2898，"Reset to default" 在 :4332，"View all available system fonts" 在 :4406，注释里写的 2630/3909/3861 等行号对不上 | 删掉行号，只留 `warp: appearance_page.rs` 与区块名（如 Text / Reset to default）；需要定位让读者在 warp 里搜区块名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-042", "cls": "C", "file": "src/settings_view/appearance_page.rs", "key": "// warp: appearance_page.rs:3861-3891 — \"Reset to default\"", "decision": "", "done": ""} -->
| C-043 | `src/settings_view/appearance_page.rs:677` | `render_current_theme_row` | `appearance_page.rs:2630-2713` | ../warp app/src/settings_view/appearance_page.rs 当前 5679 行，锚点已漂移："Current theme" 在 :2898，"Reset to default" 在 :4332，"View all available system fonts" 在 :4406，注释里写的 2630/3909/3861 等行号对不上 | 删掉行号，只留 `warp: appearance_page.rs` 与区块名（如 Text / Reset to default）；需要定位让读者在 warp 里搜区块名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-043", "cls": "C", "file": "src/settings_view/appearance_page.rs", "key": "// warp: appearance_page.rs:2630-2713 — \"Current theme\" 行", "decision": "", "done": ""} -->
| C-044 | `src/settings_view/mod.rs:93` | `render_settings_view` | `warp/app/src/settings_view/mod.rs:2265-2432` | ../warp app/src/settings_view/mod.rs 的 SettingsView::render 现在在 :2460，注释里的 2265-2432 已漂移 | 删掉行号，只留 `warp settings_view/mod.rs 的 render` | 按建议改：写 `SettingsView::render` | 2026-11-21 |<!-- entry {"no": "C-044", "cls": "C", "file": "src/settings_view/mod.rs", "key": "// warp/app/src/settings_view/mod.rs:2265-2432", "decision": "", "done": ""} -->
| C-045 | `src/settings_view/mod.rs:225`–226 | `render_settings_view` | `warp settings_view/mod.rs:2427-2432` | ../warp app/src/settings_view/mod.rs 的 SettingsView::render 现在在 :2460，注释里的 2265-2432 已漂移 | 删掉行号，只留 `warp settings_view/mod.rs 的 render` | 按建议改：写 `SettingsView::render` | 2026-11-21 |<!-- entry {"no": "C-045", "cls": "C", "file": "src/settings_view/mod.rs", "key": "// --- Compose --- // warp settings_view/mod.rs:2427-2432", "decision": "", "done": ""} -->
| C-046 | `src/settings_view/mod.rs:244` | `render_theme_chooser_panel` | `theme_chooser.rs:855-869` | 文件实为 ../warp app/src/themes/theme_chooser.rs（共 997 行，898-1001 已越界；render 在 :851，render_title_row 在 :633，render_thumbnail 在 :890），且 nexshell 注释没写目录，容易误找 settings_view/ | 删行号，写成 `warp app/src/themes/theme_chooser.rs 的 render / render_title_row / render_list` | 按建议改：只写与本条对应的 `render` | 2026-11-21 |<!-- entry {"no": "C-046", "cls": "C", "file": "src/settings_view/mod.rs", "key": "// warp: theme_chooser.rs:855-869 — 主题选择器侧面板", "decision": "", "done": ""} -->
| C-047 | `src/settings_view/mod.rs:255`–256 | `render_theme_chooser_panel` | `theme_chooser.rs:637-688` | 文件实为 ../warp app/src/themes/theme_chooser.rs（共 997 行，898-1001 已越界；render 在 :851，render_title_row 在 :633，render_thumbnail 在 :890），且 nexshell 注释没写目录，容易误找 settings_view/ | 删行号，写成 `warp app/src/themes/theme_chooser.rs 的 render / render_title_row / render_list` | 按建议改：只写与本条对应的 `render_title_row` | 2026-11-21 |<!-- entry {"no": "C-047", "cls": "C", "file": "src/settings_view/mod.rs", "key": "// --- Header: \"Themes\" 标题 + 关闭按钮 --- // warp: theme_chooser.rs:637-688", "decision": "", "done": ""} -->
| C-048 | `src/settings_view/mod.rs:300`–301 | `render_theme_chooser_panel` | `theme_chooser.rs:898-1001` | 文件实为 ../warp app/src/themes/theme_chooser.rs（共 997 行，898-1001 已越界；render 在 :851，render_title_row 在 :633，render_thumbnail 在 :890），且 nexshell 注释没写目录，容易误找 settings_view/ | 删行号，写成 `warp app/src/themes/theme_chooser.rs 的 render / render_title_row / render_list` | 按建议改：只写与本条对应的 `render_list` | 2026-11-21 |<!-- entry {"no": "C-048", "cls": "C", "file": "src/settings_view/mod.rs", "key": "// --- 主题卡片列表 --- // warp: theme_chooser.rs:898-1001", "decision": "", "done": ""} -->
| C-049 | `src/settings_view/mod.rs:351` | `render_theme_chooser_card` | `theme_chooser.rs:898-1001` | 文件实为 ../warp app/src/themes/theme_chooser.rs（共 997 行，898-1001 已越界；render 在 :851，render_title_row 在 :633，render_thumbnail 在 :890），且 nexshell 注释没写目录，容易误找 settings_view/ | 删行号，写成 `warp app/src/themes/theme_chooser.rs 的 render / render_title_row / render_list` | 按建议改：对应函数拿不准，只写文件 `warp app/src/themes/theme_chooser.rs` | 2026-11-21 |<!-- entry {"no": "C-049", "cls": "C", "file": "src/settings_view/mod.rs", "key": "// warp: theme_chooser.rs:898-1001 — 单个主题卡片", "decision": "", "done": ""} -->
| C-050 | `src/settings_view/render_helpers.rs:22` | `render_page_title` | `warp settings_page.rs:738-751` | ../warp app/src/settings_view/settings_page.rs：render_page_title 现在在 :859，注释行号已漂移 | 删掉行号，写对应函数名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-050", "cls": "C", "file": "src/settings_view/render_helpers.rs", "key": "// warp settings_page.rs:738-751", "decision": "", "done": ""} -->
| C-051 | `src/settings_view/render_helpers.rs:42` | `render_category_header` | `warp settings_page.rs:279-296` | ../warp app/src/settings_view/settings_page.rs：render_sub_header 现在在 :257，注释行号已漂移 | 删掉行号，写对应函数名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-051", "cls": "C", "file": "src/settings_view/render_helpers.rs", "key": "// warp settings_page.rs:279-296", "decision": "", "done": ""} -->
| C-052 | `src/settings_view/render_helpers.rs:63` | `render_setting_row` | `warp settings_page.rs:780-835` | ../warp app/src/settings_view/settings_page.rs：render_body_item 现在在 :867，注释行号已漂移 | 删掉行号，写对应函数名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-052", "cls": "C", "file": "src/settings_view/render_helpers.rs", "key": "// warp settings_page.rs:780-835 (simplified)", "decision": "", "done": ""} -->
| C-053 | `src/settings_view/render_helpers.rs:86` | `render_separator` | `warp settings_page.rs:380-385` | ../warp app/src/settings_view/settings_page.rs：render_separator 现在在 :396，注释行号已漂移 | 删掉行号，写对应函数名 | 按建议改 | 2026-11-21 |<!-- entry {"no": "C-053", "cls": "C", "file": "src/settings_view/render_helpers.rs", "key": "// warp settings_page.rs:380-385 — 分隔线", "decision": "", "done": ""} -->
| C-054 | `src/ui_components/mod.rs:1`–2 | `blended_colors` | menu 本地化所需：buttons + blended_colors + icons | src/ui_components/mod.rs 还含 avatar、red_notification_dot（avatar 被 text_editor/view/mod.rs:100 使用），子集范围已超出 menu | 改成「Warp ui_components 本地子集：buttons / blended_colors / avatar / red_notification_dot / icons」，或删掉「menu 所需」限定 | 按建议改：取后一种，只写「Warp ui_components 本地子集」，不列清单（列了还会再过期） | 2026-11-21 |<!-- entry {"no": "C-054", "cls": "C", "file": "src/ui_components/mod.rs", "key": "//! Warp ui_components 子集（menu 本地化所需：buttons + blended_colors + icons）。", "decision": "", "done": ""} -->

### D 待办

| 编号 | 位置 | 所在符号 | 原文 | 待办内容 | 现状与依据 | 建议 | 决定 | 完成 |
|---|---|---|---|---|---|---|---|---|
| D-001 | `src/ui_components/buttons.rs:114`–115 | `combo_inner_button` | /// TODO(CORE-2300): Evaluate whether or not this helper makes sense in this<br> | 评估 combo_inner_button 放在 ui_components/buttons.rs 是否合适（因其只被 workspace/view.rs 使用，放这里只是为了访问非 pub 字段） | 这是从 Warp 拷来的上游工单（../warp app/src/ui_components/buttons.rs:114 同文）。nexshell 没有 workspace/view.rs，且 rg 显示 combo_inner_button 除自身定义(buttons.rs:117)和样式函数外无任何调用方：该函数本身是死代码，TODO 指向的上下文在本仓不存在。建议连函数一起评估删除，否则删掉这条上游 TODO。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-21（保留，不动） |<!-- entry {"no": "D-001", "cls": "D", "file": "src/ui_components/buttons.rs", "key": "/// TODO(CORE-2300): Evaluate whether or not this helper makes sense in this", "decision": "", "done": ""} -->
| D-002 | `src/util/bindings.rs:250`–252 | `custom_tag_to_keystroke` | // Note: The base character '/' is used instead of '?' as mac registers keybindings<br>// differently compared to the app which saves the resulting character used with shift<br>// TODO: resolve these keybinding differences | 解决 Mac 与应用对 shift 组合键基字符（"/" vs "?"）注册方式不一致的问题 | 未做：util/bindings.rs:253 仍是 `ctrl-shift-/`；该 TODO 抄自 Warp，无工单号。ToggleResourceCenter 在 nexshell 里只有枚举(bindings.rs:81)与映射(:253)，rg 无其他使用，可能是死绑定，维护者可决定删 TODO 或整个绑定。 | 补引用 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-21（保留，不动） |<!-- entry {"no": "D-002", "cls": "D", "file": "src/util/bindings.rs", "key": "// Note: The base character '/' is used instead of '?' as mac registers keybindings // differently compared to the app which saves the resulting character used with shift // TODO: resolve these keybin", "decision": "", "done": ""} -->
| D-003 | `src/util/grid.rs:71`–73 | `calculate_grid_baseline_position` | // TODO(advait): Consider having this simply return a f32 instead of a vec2f (need to change entire callstack).<br>// Adjust descent to be scaled up by the line height ratio to move up the baseline<br>// and approximately maintain the same position within a line. | 让 calculate_grid_baseline_position 返回 f32 而非 Vec2F（需要改整条调用链） | 未做：util/grid.rs:67-80 仍返回 Vector2F；TODO 抄自 Warp 且署名他人。调用方在 text_editor/view/mod.rs:6632 与 terminal_grid_element.rs（自带实现），本仓无意改签名；同组另含 C 说明（leading/descent 含义）。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-21（保留，不动） |<!-- entry {"no": "D-003", "cls": "D", "file": "src/util/grid.rs", "key": "// TODO(advait): Consider having this simply return a f32 instead of a vec2f (need to change entire callstack). // Adjust descent to be scaled up by the line height ratio to move up the baseline // an", "decision": "", "done": ""} -->
| D-004 | `src/view_components/action_button.rs:389` | `ActionButton` | // TODO: Remove when we have use cases for with_height outside find bar (not compiled for WASM) | ActionButton::with_height 等到有 find bar 之外的用例时删除（"not compiled for WASM"） | 抄自 Warp（../warp app/src/view_components/action_button.rs:390 同文）。注释的位置其实贴在 with_width 的 #[allow(dead_code)] 上（action_button.rs:389-391），指代错位；本仓不编译 WASM；with_height 已被 code_editor/find/view.rs:180、190 使用（find bar），所以 `#[allow(dead_code)]` 是否还需要未核实（需去掉后 cargo check）。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-21（保留，不动） |<!-- entry {"no": "D-004", "cls": "D", "file": "src/view_components/action_button.rs", "key": "// TODO: Remove when we have use cases for with_height outside find bar (not compiled for WASM)", "decision": "", "done": ""} -->
| D-005 | `src/view_components/action_button.rs:391`–392 | `ActionButton` | // TODO(varoon): If the total width of child elements exceeds the button width, layout issues can occur. Fix this in a follow-on PR.<br>// When using this API, ensure the width is large enough to accommodate all child elements with some buffer. | 子元素总宽超过按钮宽度时会出现布局问题，后续 PR 修复 | 未做：with_width 仍是简单赋值(action_button.rs:393-396)；抄自 Warp，署名他人，无工单。留一句「须保证 width 足够容纳子元素」的使用约束即可，TODO 部分可删。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-21（保留，不动） |<!-- entry {"no": "D-005", "cls": "D", "file": "src/view_components/action_button.rs", "key": "// TODO(varoon): If the total width of child elements exceeds the button width, layout issues can occur. Fix this in a follow-on PR. // When using this API, ensure the width is large enough to accommo", "decision": "", "done": ""} -->
| D-006 | `src/view_components/action_button.rs:405` | `ActionButton` | // TODO: Remove when we have use cases for with_height outside find bar (not compiled for WASM) | ActionButton::with_height 等到有 find bar 之外的用例时删除（not compiled for WASM） | 与 0663 同文，贴在 with_height（action_button.rs:405-411）上；with_height 被 code_editor/find/view.rs 使用，WASM 不适用于本仓。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-21（保留，不动） |<!-- entry {"no": "D-006", "cls": "D", "file": "src/view_components/action_button.rs", "key": "// TODO: Remove when we have use cases for with_height outside find bar (not compiled for WASM)", "decision": "", "done": ""} -->

## 批次 root-view

### B 为捷径辩护

#### B-007 `src/root_view/host_library_section/transfer.rs:331` `RootView.cancel_host_password`

<!-- entry {"no": "B-007", "cls": "B", "file": "src/root_view/host_library_section/transfer.rs", "key": "// 加密/解密在后台跑，没有 cancel token；让任务跑完后 callback 会自然清状态。", "decision": "", "done": ""} -->
> // 加密/解密在后台跑，没有 cancel token；让任务跑完后 callback 会自然清状态。

- 捷径：密码栏在加密/解密后台任务运行时，取消按钮直接 return，不支持取消（没有 cancel token）
- 是否仍然成立：是。证据：src/root_view/host_library_section/transfer.rs:331-336 `if self.host_password_busy { return; }`；spawn_host_export（:136 起）、spawn_host_import（:226 起）的 ctx.spawn 异步块里没有任何取消检查。
- 建议：保留
- 风险：低：PBKDF2/AES 只跑几秒，期间点取消无反应；任务结束后回调会清状态（transfer.rs:209、:317 clear_host_password_state）。
- 决定：保留
- 完成：2026-11-21（保留，不动）

#### B-008 `src/root_view/settings_section.rs:112` `RootView`

<!-- entry {"no": "B-008", "cls": "B", "file": "src/root_view/settings_section.rs", "key": "// What's New / Documentation / Feedback / View Logs 暂仅关闭菜单（占位，后续接外链）。", "decision": "", "done": ""} -->
> // What's New / Documentation / Feedback / View Logs 暂仅关闭菜单（占位，后续接外链）。

- 捷径：设置菜单里 What's New / Documentation / Feedback / View Logs 四项只关闭菜单，是占位实现，注释自称「后续接外链」
- 是否仍然成立：是。证据：src/root_view/settings_section.rs:112-119 handle_settings_menu_dismiss 只置 settings_menu_open=false；src/root_view/mod.rs:2586-2589 四个 action 全部路由到它；菜单项仍在 settings_section.rs:66-83 创建并展示（locales 里有 menu_whats_new / menu_documentation / menu_feedback / menu_view_logs）。仓库 docs/ 里没有这四项的决策记录。
- 建议：记待办
- 风险：中：用户看得到、点得了，但点了没反应，像坏掉；要么去掉这四项，要么接外链（属新功能，需 Matt 拍板），拍板结果记进 docs。
- 决定：修代码
- 拍板备注：推翻「记待办」。去掉这四项，不接外链（理由见拍板记录）。要删：`settings_section.rs` 的四个菜单项、分隔线和 `handle_settings_menu_dismiss`，`root_view/mod.rs` 的路由，`src/terminal_grid_element.rs` 的四个 `SettingsMenu*` action，`locales/en.yml` 与 `locales/zh-CN.yml` 的 `menu_whats_new` / `menu_documentation` / `menu_feedback` / `menu_view_logs`。后两处在本批目录之外，已同意连带修改；菜单少四项这个可见变化已同意。
- 完成：2026-11-22

#### B-009 `src/root_view/tab_bar_section/title_bar.rs:239` `RootView.tab_accent_bar_layout`

<!-- entry {"no": "B-009", "cls": "B", "file": "src/root_view/tab_bar_section/title_bar.rs", "key": "// NEXSHELL_BAR_DEBUG=1 时打印弹簧轨迹（定位滑动异常用，临时观测件）。", "decision": "", "done": ""} -->
> // NEXSHELL_BAR_DEBUG=1 时打印弹簧轨迹（定位滑动异常用，临时观测件）。

- 捷径：NEXSHELL_BAR_DEBUG 调试打印常驻生产路径，注释自称「临时观测件」
- 是否仍然成立：否。证据：src/root_view/tab_bar_section/title_bar.rs:239-247 与 :264-268 两处 eprintln；引入于 1008863（2026-07-07，Phase 2 动效），至快照已 4 个多月，后续 7ac14af（2026-10-10）已「修面板滑动串标签」，说明当初要定位的滑动异常多半已处理；docs/ 无 NEXSHELL_BAR_DEBUG 的记载。滑动异常是否已彻底解决：未核实（需 Matt 确认）。tab_accent_bar_layout 每帧调用，两处 `std::env::var_os` 每帧都会读环境变量。
- 建议：修代码
- 风险：低：不影响功能，但「临时」已名不副实；每帧两次读环境变量 + 无文档的隐藏开关。确认异常已解决后删掉这两块 eprintln 与这句注释。
- 决定：修代码
- 拍板备注：删掉两处 eprintln 和这句注释。它要定位的底条滑动毛病，在引入它的同一个提交 1008863 里就修掉了，之后这段代码没再动过（见拍板记录）。
- 完成：2026-11-21

#### B-010 `src/root_view/terminal_section/split.rs:150`–152 `RootView.split_active_pane`

<!-- entry {"no": "B-010", "cls": "B", "file": "src/root_view/terminal_section/split.rs", "key": "// Split pane: 新 PTY/SSH 是新 pane 上的，file_panel 当前按 tab 维度（per-tab） // 关联 ssh_handle，多 pane 场景的 handle 选择策略后续再细化。 // 这里不接 ssh_handle_stream，receiver 自然 drop。", "decision": "", "done": ""} -->
> // Split pane: 新 PTY/SSH 是新 pane 上的，file_panel 当前按 tab 维度（per-tab）
> // 关联 ssh_handle，多 pane 场景的 handle 选择策略后续再细化。
> // 这里不接 ssh_handle_stream，receiver 自然 drop。

- 捷径：分屏新开的 SSH pane 不接 ssh_handle_stream，文件面板只认 tab 级 handle，多 pane 的 handle 选择「后续再细化」（推迟）
- 是否仍然成立：是。证据：src/root_view/terminal_section/split.rs:148 `Self::attach_terminal_streams(&amp;mut new_terminal, None, ctx)`；ssh_handle_rx 只在 mod.rs:943 起的 attach_ssh_handle_stream 里被取走（按 tab 维度，src/main.rs:591 TerminalSessionTab.ssh_handle 是 tab 级字段），split.rs 全文没有再接。docs/ 中未发现对应待办或 ADR 记录。
- 建议：记待办
- 风险：低：分屏里新开的远程 pane 不产生 SFTP handle，文件面板 / 远程编辑仍绑 tab 原 handle（首个 pane）；用户在新 pane 里期待文件面板跟随会落空。现状是否有用户反馈：未核实。
- 决定：记待办
- 拍板备注：待办内容：分屏后关掉原 pane（tab 级 handle 属于第一个连接），确认文件面板和远程编辑还能不能用；不能用再定 handle 跟哪个 pane。注释里说明「为什么传 None」的部分保留，只把「后续再细化」换成待办编号。
- 完成：2026-11-22（T-001）

### C 过期的「为什么」

| 编号 | 位置 | 所在符号 | 过期的句子 | 依据 | 建议改成 | 决定 | 完成 |
|---|---|---|---|---|---|---|---|
| C-055 | `src/root_view/code_viewer_section.rs:840`–841 | `RootView` | 渲染只读代码查看器主体 | ADR 0003 已反转 0002，内置查看器就地可编辑：src/root_view/code_viewer_section.rs:218-219 set_interaction_state(Editable)，同文件第 1 行头注释也写「可编辑可存」。 | 渲染内置代码编辑器主体（不含侧栏）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-055", "cls": "C", "file": "src/root_view/code_viewer_section.rs", "key": "/// 渲染只读代码查看器主体（不含侧栏）。侧栏由 render_active_tab_body_with_side_panels 包裹，", "decision": "", "done": ""} -->
| C-056 | `src/root_view/connect_overlay_section.rs:1`–3 | `(顶层)` | 本文件只含 impl RootView，无自由函数 | src/root_view/connect_overlay_section.rs:34 有自由函数 `fn faded(color, opacity)`（0066 就是它的文档注释）。 | 改为「除 faded 外只含 impl RootView」。 | 不改：约定（conventions.md「边界」第 1 条）要求 section 文件不放自由函数，不符的是代码——`faded` 和 `ui_anim::scale_alpha` 重复；不为它在头注释里开例外，代码问题另报 | 2026-11-22（不改） |<!-- entry {"no": "C-056", "cls": "C", "file": "src/root_view/connect_overlay_section.rs", "key": "// connect_overlay_section — SSH 连接中盖在终端上的过渡层：进度环、主机、三个阶段、进度条和计时。 // 本文件只含 impl RootView，无自由函数；淡入淡出状态机在 connect_overlay_helpers.rs， // 进度环元件在 nexshell::connect_spinner。连接阶段由 SSH runtime 写进快照（ssh_conne", "decision": "", "done": ""} -->
| C-057 | `src/root_view/context_menus_section.rs:1`–6 | `(顶层)` | show_* / toggle_* 由 mod.rs handle_action 分发——均 pub(in crate::root_view) | src/root_view/context_menus_section.rs:135/170/305/352/359/404/411/509/599/689：show_* / toggle_* 是 pub(crate)，只有 render_* 是 pub(in crate::root_view)（:34-54, :594）。 | render_* 用 pub(in crate::root_view)，show_* / toggle_* 为 pub(crate)。 | 按建议改，改法更正：不写新的可见性清单，把「——均 pub(in crate::root_view)」直接删掉（可见性看代码） | 2026-11-22 |<!-- entry {"no": "C-057", "cls": "C", "file": "src/root_view/context_menus_section.rs", "key": "// context_menus_section — RootView 的右键 / 上下文菜单：tab 右键、终端、文件面板、git 面板、 // 主机卡片、进程列表。render_* 由 mod.rs View::render 调用、show_* / toggle_* 由 mod.rs // handle_action 分发——均 pub(in crate::root_view)。*_items", "decision": "", "done": ""} -->
| C-058 | `src/root_view/context_menus_section.rs:141` | `RootView.toggle_tab_right_click_menu` | warp/app/src/workspace/view.rs:6488-6507 | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，该范围是 open file / terminal_view_from_pane_id 一类无关代码；对应的 toggle_tab_right_click_menu 现位于 workspace/view.rs:7905。 | 改为引用符号名 `workspace/view.rs 的 toggle_tab_right_click_menu`，不写行号，或删除。 | 按建议改：写成符号引用，不删 | 2026-11-22 |<!-- entry {"no": "C-058", "cls": "C", "file": "src/root_view/context_menus_section.rs", "key": "// warp/app/src/workspace/view.rs:6488-6507.", "decision": "", "done": ""} -->
| C-059 | `src/root_view/context_menus_section.rs:169` | `RootView` | warp: view.rs:16064-16113 (rebuild_alt_screen_context_menu_items) | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:16064 起是 pane_group::Event 分发；rebuild_alt_screen_context_menu_items 在 app/src/terminal/view.rs:17931，不在 workspace/view.rs。 | 改为 `warp terminal/view.rs 的 rebuild_alt_screen_context_menu_items`，不写行号。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-059", "cls": "C", "file": "src/root_view/context_menus_section.rs", "key": "// warp: view.rs:16064-16113 (rebuild_alt_screen_context_menu_items)", "decision": "", "done": ""} -->
| C-060 | `src/root_view/context_menus_section.rs:207` | `RootView.terminal_context_menu_items` | warp: view.rs:16074-16080 — Copy（有选区时） | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，同 0075，原范围已漂移；对应逻辑在 app/src/terminal/view.rs rebuild_alt_screen_context_menu_items（:17931 起）。 | 改为引用该函数名，或删除。 | 按建议改：写成符号引用，保留「Copy（有选区时）」，不删 | 2026-11-22 |<!-- entry {"no": "C-060", "cls": "C", "file": "src/root_view/context_menus_section.rs", "key": "// warp: view.rs:16074-16080 — Copy（有选区时）", "decision": "", "done": ""} -->
| C-061 | `src/root_view/file_panel_section/body.rs:374`–375 | `RootView.render_local_file_panel_tree_row` | 文件双击 → 内置只读查看器 … 编辑仍走右键「编辑」 | 双击打开的是可编辑的内置编辑器（ADR 0003/0005；code_viewer_section.rs:218-219）；右键「编辑」走外部编辑器（file_panel_section/actions.rs:555 handle_file_panel_open_in_editor）。 | 文件双击 → 内置编辑器（二进制/超大由 handler 回退「用外部程序打开」）；目录双击在树里靠单击展开，不处理。右键「编辑」走配置的外部编辑器。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-061", "cls": "C", "file": "src/root_view/file_panel_section/body.rs", "key": "// 文件双击 → 内置只读查看器（二进制/超大由 handler 回退「用外部程序打开」）； // 目录双击在树里靠单击展开，不处理。编辑仍走右键「编辑」。", "decision": "", "done": ""} -->
| C-062 | `src/root_view/file_panel_section/mod.rs:1`–10 | `actions` | mod.rs：… + 文件 / git 面板滑动收尾 | file_panel_section/mod.rs:401-455 的 finalize_panel_closes / 切标签落定同时处理概览面板（:440 起，ADR 0023 新增）；同函数上方的 0135 注释已写「文件 / git / 概览面板共用」。 | …+ 文件 / git / 概览面板滑动收尾（落闸、切标签落定）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-062", "cls": "C", "file": "src/root_view/file_panel_section/mod.rs", "key": "// file_panel_section — RootView 的文件面板（SFTP / 本地 Project explorer）方法集合。 // // 详见 docs/adr/0001-root-view-multi-file-impl.md。本目录只含 impl RootView，无自由函数； // 新增 helper 一律加到 src/file_panel_view_helpers.rs。", "decision": "", "done": ""} -->
| C-063 | `src/root_view/file_panel_section/mod.rs:140`–141 | `RootView` | 参考 warp pane_group/mod.rs:7943 在外层包 dragged 的同样目的 | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，app/src/pane_group/mod.rs:7943 现在是 visible_terminal_views 等无关代码，EventHandler 外包在 :8333 一带。 | 改为 `warp pane_group/mod.rs 的 render（外层 EventHandler）` 或删除行号。 | 按建议改：写「warp pane_group/mod.rs 的 render（外层 EventHandler）」 | 2026-11-22 |<!-- entry {"no": "C-063", "cls": "C", "file": "src/root_view/file_panel_section/mod.rs", "key": "/// 全局接收，避免鼠标超出 6px 命中区时事件丢失（参考 warp pane_group/mod.rs:7943", "decision": "", "done": ""} -->
| C-064 | `src/root_view/file_panel_section/mod.rs:143`–147 | `RootView.render_file_panel_divider` | Draggable 在 mouse_down 时硬写 set_cursor(PointingHand)（draggable.rs:611） | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，该行为仍成立，但行号变了：crates/warpui_core/src/elements/gui/drag/draggable.rs:630（另在 :722），不是 :611。 | 把 `draggable.rs:611` 改为 `draggable.rs 的 mouse_down 处理`（不写行号）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-064", "cls": "C", "file": "src/root_view/file_panel_section/mod.rs", "key": "// warpui::Draggable 提供\"按住-离开元素也持续派发\"的 capture flow // （ui_components/slider.rs 同款）。EventHandler.on_mouse_dragged 是 hit-test // 派发，鼠标移出 6px 命中区就停了。 // Draggable 在 mouse_down 时硬写 set_cursor(PointingHan", "decision": "", "done": ""} -->
| C-065 | `src/root_view/find_section.rs:1`–6 | `(顶层)` | 本文件只含 impl RootView，无自由函数 … 均用 pub(in crate::root_view) | src/root_view/find_section.rs:360 有自由函数 find_editor_terminal_input_bytes；create_find_editor（:27）与 close_find_bar（:101）是 pub(crate)，不是 pub(in crate::root_view)。其余（find_match_label 已迁 terminal_view_helpers.rs:219）仍成立。 | 改为「除 find_editor_terminal_input_bytes 外只含 impl RootView」，并更正 close_find_bar / create_find_editor 的可见性说明。 | 按建议改，改法更正：只删可见性那句和「已于 step 10 归……」；「无自由函数」保留，不写「除……外」的例外——`find_editor_terminal_input_bytes` 按约定该挪去 helpers，代码问题另报 | 2026-11-22 |<!-- entry {"no": "C-065", "cls": "C", "file": "src/root_view/find_section.rs", "key": "// find_section — RootView 的终端查找栏：查找编辑器、查找 action、查找栏 render。 // // 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。 // create_find_editor 由 RootView::new() 调用；handle_* / close", "decision": "", "done": ""} -->
| C-066 | `src/root_view/find_section.rs:82` | `RootView` | warp: view.rs:18177-18210 (show_find_bar) | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:18177 是 WindowSettingsChangedEvent 分支；show_find_bar 在 app/src/terminal/view.rs:19797（另 code/editor/view.rs:511）。 | 改为 `warp terminal/view.rs 的 show_find_bar`，不写行号。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-066", "cls": "C", "file": "src/root_view/find_section.rs", "key": "// warp: view.rs:18177-18210 (show_find_bar)", "decision": "", "done": ""} -->
| C-067 | `src/root_view/git_panel_section/diff_section.rs:1`–4 | `(顶层)` | select_diff handler + GitDiff tab 打开 + diff preview 渲染 | diff_section.rs 的 fn 只有 open_git_diff_tab / render_git_diff_page / render_git_panel_diff_preview / render_git_file_diff / render_git_diff_hunk_header / render_git_diff_line；点选条目的 handler 是 status_section.rs:34 handle_git_panel_select_entry，没有 select_diff。 | GitDiff tab 打开 + diff 页 / preview 渲染。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-067", "cls": "C", "file": "src/root_view/git_panel_section/diff_section.rs", "key": "// diff section — RootView 的 git diff 视图：select_diff handler + GitDiff tab 打开 + diff preview 渲染。 // // 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。 // 跨 section 调用：open_git", "decision": "", "done": ""} -->
| C-068 | `src/root_view/host_library_section/actions.rs:85`–86 | `RootView` | （无 toast 机制，见 ADR 0009 Step3） | 底部提示条已存在：src/root_view/toast_section.rs（提交 7ac14af，2026-10-10），host_state.notice 会被淡入淡出显示；且 docs/adr/0009-container-management.md 里没有「Step3」，引用悬空。 | 删除括号内这句；保留「失败写回该主机容器快照的 status 让卡片区显示」。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-068", "cls": "C", "file": "src/root_view/host_library_section/actions.rs", "key": "/// 失败写回该主机容器快照的 status 让卡片区显示（无 toast 机制，见 ADR 0009 Step3）。", "decision": "", "done": ""} -->
| C-069 | `src/root_view/mod.rs:1`–7 | `code_viewer_section` | step 1-9 抽出 9 个 section（git_panel / file_panel / host_library / host_monitor / settings / tab_bar / terminal / find / context_menus） | src/root_view/ 现有 14 个 section：另有 code_viewer、connect_overlay、host_list、rdp、toast（ls src/root_view）；步骤编号 step N 是 ADR 0001 迁移期用语。 | 把 section 清单交给 docs/adr/0001 与目录本身，头注释只保留「RootView 多文件 impl 容器；struct / new / View / TypedActionView 在本文件」。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-069", "cls": "C", "file": "src/root_view/mod.rs", "key": "// RootView 多文件 impl 容器。 // 详细决策见 docs/adr/0001-root-view-multi-file-impl.md。 // // step 1-9 抽出 9 个 section（git_panel / file_panel / host_library / host_monitor / // settings / tab_bar / terminal / fi", "decision": "", "done": ""} -->
| C-070 | `src/root_view/mod.rs:329` | `RootView` | warp: appearance_page.rs:499-500 | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，settings_view/appearance_page.rs:499-500 现在是 action 枚举变体；font_family_dropdown / font_weight_dropdown 字段在 :545-546。 | 改为 `warp appearance_page.rs 的 font_family_dropdown / font_weight_dropdown 字段`。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-070", "cls": "C", "file": "src/root_view/mod.rs", "key": "// warp: appearance_page.rs:499-500", "decision": "", "done": ""} -->
| C-071 | `src/root_view/mod.rs:394` | `RootView.new` | Falls back to monospace if Helvetica Neue 不可用 | src/font_enumeration.rs:265-272 / 317-342：按平台候选列表依次试（macOS/Linux 为 Helvetica Neue、Helvetica、Arial、Segoe UI；Windows 为微软雅黑等），都不行再用内置 Roboto，只有全部失败才 unwrap_or(monospace_font)（mod.rs:395）。 | UI 字体：按 font_enumeration::ui_font_families 候选 → 内置 Roboto，全部失败才回退等宽字体。（并改成中文，与全文件一致） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-071", "cls": "C", "file": "src/root_view/mod.rs", "key": "// UI font for chrome (tabs / titlebar). Falls back to monospace if Helvetica Neue 不可用。", "decision": "", "done": ""} -->
| C-072 | `src/root_view/mod.rs:420` | `RootView.new` | Only drives blink and bell decay while PTY is idle. | schedule_idle_refresh（src/root_view/mod.rs:1536-1556）现在驱动：录制红点闪烁、git push 动画 tick、UI 过渡 tick 兜底自愈（≤100ms）、flush_notice_toast；函数里已没有 bell 相关逻辑（rg bell src/root_view/mod.rs 仅命中本注释）。 | 100ms 空闲定时：录制红点闪烁 / push 动画 / 过渡 tick 兜底 / 提示条接走 notice。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-072", "cls": "C", "file": "src/root_view/mod.rs", "key": "// Only drives blink and bell decay while PTY is idle.", "decision": "", "done": ""} -->
| C-073 | `src/root_view/mod.rs:474` | `RootView.new` | warp: appearance_page.rs:995-1015 — FilterableDropdown for font family | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，appearance_page.rs:995-1015 已无对应内容；font_family_dropdown 创建在 :1118-1128。且该句只复述下面的 FilterableDropdown 构造。 | 删除这句（行号已漂移，内容仅复述）。 | 按建议改：整句删，C-070 已在字段处留了 Warp 出处 | 2026-11-22 |<!-- entry {"no": "C-073", "cls": "C", "file": "src/root_view/mod.rs", "key": "// warp: appearance_page.rs:995-1015 — FilterableDropdown for font family", "decision": "", "done": ""} -->
| C-074 | `src/root_view/mod.rs:500` | `RootView.new` | warp: appearance_page.rs:1017-1035 — Dropdown for font weight | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，appearance_page.rs:1017-1035 现为 opacity slider 重置代码；font_weight_dropdown 创建在 :1140。 | 删除这句。 | 按建议改：整句删，这句本来就贴错了位置（紧跟的是 open_file_editor_dropdown） | 2026-11-22 |<!-- entry {"no": "C-074", "cls": "C", "file": "src/root_view/mod.rs", "key": "// warp: appearance_page.rs:1017-1035 — Dropdown for font weight", "decision": "", "done": ""} -->
| C-075 | `src/root_view/mod.rs:669` | `RootView.new` | warp/app/src/workspace/view.rs:1744-1750. | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:1744-1750 现为 welcome tips / build_ai_assistant_panel_view 附近，与标签右键菜单创建无关。 | 删除这句。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-075", "cls": "C", "file": "src/root_view/mod.rs", "key": "// warp/app/src/workspace/view.rs:1744-1750.", "decision": "", "done": ""} -->
| C-076 | `src/root_view/mod.rs:684` | `RootView.new` | warp: view.rs:3696-3702 | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:3696-3702 现为 update_transcript_details_panel_data 一带，与终端右键菜单创建无关。 | 删除这句。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-076", "cls": "C", "file": "src/root_view/mod.rs", "key": "// warp: view.rs:3696-3702", "decision": "", "done": ""} -->
| C-077 | `src/root_view/mod.rs:1948` | `RootView.render` | 照 ProcessList/CodeViewer 全页先例 | CodeViewer 现在带侧栏：src/root_view/mod.rs:1936-1943 用 render_active_tab_body_with_side_panels 包裹（同文件 0436 注释也这么写）；只有 ProcessList/NetworkList/SystemInfo 与 RDP 是无侧栏整页。 | RDP：整页，无文件 / git / 概览侧栏，不参与 split。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-077", "cls": "C", "file": "src/root_view/mod.rs", "key": "// RDP：整页，无侧栏（照 ProcessList/CodeViewer 全页先例，不参与 split）。", "decision": "", "done": ""} -->
| C-078 | `src/root_view/mod.rs:1991` | `RootView.render` | warp: workspace/view.rs:22848-22875 — 背景图片渲染 | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:22848-22875 现为 input_mode / OffsetPositioning 代码，与背景图无关。 | 删除这句（下面代码本身说明是背景图渲染）。 | 按建议改，改法更正：不整句删，改成「背景图片渲染（对照 warp workspace/view.rs 里 `theme.background_image()` 的处理）」 | 2026-11-22 |<!-- entry {"no": "C-078", "cls": "C", "file": "src/root_view/mod.rs", "key": "// warp: workspace/view.rs:22848-22875 — 背景图片渲染", "decision": "", "done": ""} -->
| C-079 | `src/root_view/mod.rs:2068`–2069 | `RootView.render` | warp: view.rs:26674-26679 | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:26674-26679 现为 render_detail_sidecar 调用，与根容器背景色无关。 | 删除行号前缀，保留「无背景图主题给根容器铺主题背景色兜底，否则 bootstrap 占位、面板滑动等未着色帧会透出桌面」。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-079", "cls": "C", "file": "src/root_view/mod.rs", "key": "// warp: view.rs:26674-26679 — 无背景图主题给根容器铺主题背景色兜底， // 否则 bootstrap 占位、面板滑动等未着色帧会透出桌面。", "decision": "", "done": ""} -->
| C-080 | `src/root_view/mod.rs:2109` | `RootView.render` | warp/app/src/workspace/view.rs:22172-22187. | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:22172-22187 现为 render_autoupdate_banner_element。 | 删除这句。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-080", "cls": "C", "file": "src/root_view/mod.rs", "key": "// warp/app/src/workspace/view.rs:22172-22187.", "decision": "", "done": ""} -->
| C-081 | `src/root_view/mod.rs:2122` | `RootView.render` | warp: view.rs:16543-16576 (show_context_menu) | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:16543-16576 现为 DirectoryTabColors / pane_group 逻辑；终端 show_context_menu 在 app/src/terminal/view/context_menu.rs:503。 | 改为 `warp terminal/view/context_menu.rs 的 show_context_menu` 或删除。 | 按建议改：写成符号引用，不删 | 2026-11-22 |<!-- entry {"no": "C-081", "cls": "C", "file": "src/root_view/mod.rs", "key": "// warp: view.rs:16543-16576 (show_context_menu)", "decision": "", "done": ""} -->
| C-082 | `src/root_view/mod.rs:3195` | `RootView` | warp: workspace/view.rs:11886-11950 | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:11886-11950 现为 RightPanelSnapshot 构造，与关闭标签无关；warp 里关标签相关函数现为 close_tab_group 等（:7306 起）。 | 删除这句。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-082", "cls": "C", "file": "src/root_view/mod.rs", "key": "// warp: workspace/view.rs:11886-11950", "decision": "", "done": ""} -->
| C-083 | `src/root_view/rdp_section.rs:696`–697 | `RootView` | 无已连接 RDP tab 即停表 | src/root_view/rdp_section.rs:700-710：停表条件是没有「已连接 + EGFX + 非固定分辨率」的 tab（any_pollable），已连接但固定分辨率 / 非 EGFX 的 tab 同样不会让定时器继续跑。 | 无可轮询的 RDP tab（已连接 + EGFX + 非固定分辨率）即停表。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-083", "cls": "C", "file": "src/root_view/rdp_section.rs", "key": "/// 动态分辨率检测定时器（~100ms tick）：内容区尺寸稳定 ~400ms 且换算 ≠ 当前会话分辨率", "decision": "", "done": ""} -->
| C-084 | `src/root_view/settings_section.rs:1`–8 | `(顶层)` | 横切/infra（save_ui_settings 持久化、set_terminal_font_size 字号、apply_window_opacity 透明度）留 main.rs | 这三个函数现都在 src/root_view/mod.rs（apply_window_opacity :1758、set_terminal_font_size :3459、save_ui_settings :3463），main.rs 里没有。 | 改为「…留 root_view/mod.rs」，或删除这句。 | 按建议改：写「留 root_view/mod.rs」 | 2026-11-22 |<!-- entry {"no": "C-084", "cls": "C", "file": "src/root_view/settings_section.rs", "key": "// settings_section — 设置页 render 入口 + 设置项 action handler（主题/字体/光标/语言/帮助菜单）。 // // 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。 // 每个 handle_* 由 root_view/mod.rs handle_acti", "decision": "", "done": ""} -->
| C-085 | `src/root_view/settings_section.rs:64` | `RootView.handle_toggle_settings_menu` | warp/app/src/workspace/view.rs:8282-8413 user_menu_items | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，user_menu_items 现在在 workspace/view.rs:9800，8282-8413 是 suggested agent mode workflow modal 代码。 | 改为 `warp workspace/view.rs 的 user_menu_items`，不写行号。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-085", "cls": "C", "file": "src/root_view/settings_section.rs", "key": "// warp/app/src/workspace/view.rs:8282-8413 user_menu_items", "decision": "", "done": ""} -->
| C-086 | `src/root_view/settings_section.rs:335` | `RootView` | warp: appearance_page.rs:2268-2271 | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，该处仍在 set_font_family（:2248 起）内部，行号接近但内容已不是「toggle view all fonts」；位置不稳。 | 改为引用 `appearance_page.rs 的 set_font_family` 或删除。 | 按建议改，改法更正：对应的是 appearance_page.rs 的 `toggle_all_available_fonts`，不是 `set_font_family` | 2026-11-22 |<!-- entry {"no": "C-086", "cls": "C", "file": "src/root_view/settings_section.rs", "key": "// warp: appearance_page.rs:2268-2271", "decision": "", "done": ""} -->
| C-087 | `src/root_view/tab_bar_section/tab_render.rs:133` | `RootView.render_tab` | warp tab.rs:1458 — SavePosition ID for the draggable tab. | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，app/src/tab.rs:1458 现为 active_block().git_branch()；draggable tab 的 SavePosition ID 在 :2142 附近的函数。 | 删除行号，改为 `warp tab.rs 的 draggable tab SavePosition ID`。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-087", "cls": "C", "file": "src/root_view/tab_bar_section/tab_render.rs", "key": "// warp tab.rs:1458 — SavePosition ID for the draggable tab.", "decision": "", "done": ""} -->
| C-088 | `src/root_view/tab_bar_section/tab_render.rs:136` | `RootView.render_tab` | warp tab.rs:922 — tooltip 定位锚点 ID | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，app/src/tab.rs:922 现为 color-picker 相关类型；tab_text_position_id 在 :1464。 | 删除行号，改为 `warp tab.rs 的 tab_text_position_id`。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-088", "cls": "C", "file": "src/root_view/tab_bar_section/tab_render.rs", "key": "// warp tab.rs:922 — tooltip 定位锚点 ID", "decision": "", "done": ""} -->
| C-089 | `src/root_view/tab_bar_section/tab_render.rs:181` | `RootView.render_tab` | warp tab.rs:1272-1275 — SavePosition 包裹文字用于 tooltip 定位 | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，app/src/tab.rs:1272-1275 现为 for_drag_ghost；对应 SavePosition 包裹在 :1882。 | 删除行号。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-089", "cls": "C", "file": "src/root_view/tab_bar_section/tab_render.rs", "key": "// warp tab.rs:1272-1275 — SavePosition 包裹文字用于 tooltip 定位", "decision": "", "done": ""} -->
| C-090 | `src/root_view/tab_bar_section/tab_render.rs:247`–249 | `RootView.render_tab` | warp tab.rs:1295-1313 — compact tab switches to icon-only below COMPACT_TAB_WIDTH_THRESHOLD. | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，app/src/tab.rs:1295-1313 现为多选高亮方法；COMPACT_TAB_WIDTH_THRESHOLD 仍存在（tab.rs:271，用于 :2050/:2064）。 | 保留「紧凑模式没地方放红点，改为按相位把终端图标染红」，删除 warp 行号前缀。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-090", "cls": "C", "file": "src/root_view/tab_bar_section/tab_render.rs", "key": "// warp tab.rs:1295-1313 — compact tab switches to icon-only // below COMPACT_TAB_WIDTH_THRESHOLD. // 紧凑模式没地方放红点，改为按相位把终端图标染红。", "decision": "", "done": ""} -->
| C-091 | `src/root_view/tab_bar_section/tab_render.rs:273`–274 | `RootView.render_tab` | warp tab.rs:1371-1407 — close button is overlaid on full tabs, and only on the active tab in compact mode. | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，app/src/tab.rs:1371-1407 现为 original_title 相关代码；close 按钮 SavePosition 在 :1629。 | 删除这句。 | 按建议改，改法更正：只删行号前缀，说明句保留（同 C-093、C-094），不整句删 | 2026-11-22 |<!-- entry {"no": "C-091", "cls": "C", "file": "src/root_view/tab_bar_section/tab_render.rs", "key": "// warp tab.rs:1371-1407 — close button is overlaid on full // tabs, and only on the active tab in compact mode.", "decision": "", "done": ""} -->
| C-092 | `src/root_view/tab_bar_section/tab_render.rs:400` | `RootView.render_tab` | warp tab.rs:1594-1603 | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，app/src/tab.rs:1594-1603 现为 on_click / TabHoverWidthEnd 处理，内容与 nexshell 对应代码已无从核对；整句只有行号，没有说明。 | 删除这句。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-092", "cls": "C", "file": "src/root_view/tab_bar_section/tab_render.rs", "key": "// warp tab.rs:1594-1603", "decision": "", "done": ""} -->
| C-093 | `src/root_view/tab_bar_section/tab_render.rs:418`–419 | `RootView.render_tab` | warp tab.rs:1647-1656 — fixed width while close hover is active; otherwise each tab can shrink from max width 200. | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，app/src/tab.rs:1647-1656 现为主文本颜色代码，与宽度约束无关。 | 删除行号，保留英文说明或译成中文。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-093", "cls": "C", "file": "src/root_view/tab_bar_section/tab_render.rs", "key": "// warp tab.rs:1647-1656 — fixed width while close hover is // active; otherwise each tab can shrink from max width 200.", "decision": "", "done": ""} -->
| C-094 | `src/root_view/tab_bar_section/tab_render.rs:443`–444 | `RootView.render_tab` | warp tab.rs:1673-1678 — save the draggable tab position and make the whole tab participate in flex shrinking. | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，app/src/tab.rs:1673-1678 现为 Indicator::Maximized 图标代码。 | 删除行号，保留说明。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-094", "cls": "C", "file": "src/root_view/tab_bar_section/tab_render.rs", "key": "// warp tab.rs:1673-1678 — save the draggable tab position and // make the whole tab participate in flex shrinking.", "decision": "", "done": ""} -->
| C-095 | `src/root_view/tab_bar_section/title_bar.rs:68`–69 | `RootView.render_title_bar` | warp/app/src/workspace/view.rs:17539-17549: whole tab bar is a drop target after the last tab. | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:17539-17549 现为 coding panel 启用状态代码；标签栏渲染相关函数现在 :20702-21380（render_tab_bar_hover_area / render_tab_bar_hoverable / render_tab_bar_contents / render_tab_bar）。 | 改为引用 `workspace/view.rs 的 render_tab_bar`，不写行号。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-095", "cls": "C", "file": "src/root_view/tab_bar_section/title_bar.rs", "key": "// warp/app/src/workspace/view.rs:17539-17549: // whole tab bar is a drop target after the last tab.", "decision": "", "done": ""} -->
| C-096 | `src/root_view/tab_bar_section/title_bar.rs:82`–83 | `RootView.render_title_bar` | warp/app/src/workspace/view.rs:17555-17566: EventHandler wraps Clipped(render_tab_bar_hoverable(contents)). | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:17555-17566 现为 code review 面板注释；render_tab_bar_hoverable 在 :20711。 | 改为引用 `render_tab_bar_hoverable`。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-096", "cls": "C", "file": "src/root_view/tab_bar_section/title_bar.rs", "key": "// warp/app/src/workspace/view.rs:17555-17566: // EventHandler wraps Clipped(render_tab_bar_hoverable(contents)).", "decision": "", "done": ""} -->
| C-097 | `src/root_view/tab_bar_section/title_bar.rs:102` | `RootView.render_title_bar` | warp/app/src/workspace/view.rs:17574-17584 saves TAB_BAR_POSITION_ID. | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:17574-17584 现为 right_panel_view 更新代码；TAB_BAR_POSITION_ID 定义在 :599，使用在 :21428。 | 改为引用 `TAB_BAR_POSITION_ID`。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-097", "cls": "C", "file": "src/root_view/tab_bar_section/title_bar.rs", "key": "// warp/app/src/workspace/view.rs:17574-17584 saves TAB_BAR_POSITION_ID.", "decision": "", "done": ""} -->
| C-098 | `src/root_view/tab_bar_section/title_bar.rs:107` | `RootView.render_tab_bar_hoverable` | warp/app/src/workspace/view.rs:16946-16953. | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:16946-16953 现为 terminal_view_from_pane_id 上传映射代码；render_tab_bar_hoverable 在 :20711。 | 删除这句。 | 按建议改：整句删，上面 C-096 已引用 render_tab_bar_hoverable | 2026-11-22 |<!-- entry {"no": "C-098", "cls": "C", "file": "src/root_view/tab_bar_section/title_bar.rs", "key": "// warp/app/src/workspace/view.rs:16946-16953.", "decision": "", "done": ""} -->
| C-099 | `src/root_view/tab_bar_section/title_bar.rs:173` | `RootView.render_tab_bar_contents` | warp view.rs:17462-17465 settings button 在 shrinkable 之后。 | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:17462-17465 现为 refresh_working_directories_for_pane_group；渲染顺序现在在 render_tab_bar_contents（:20819 起）。 | 改为「settings 按钮放在 shrinkable 之后（同 warp render_tab_bar_contents）」。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-099", "cls": "C", "file": "src/root_view/tab_bar_section/title_bar.rs", "key": "// warp view.rs:17462-17465 settings button 在 shrinkable 之后。", "decision": "", "done": ""} -->
| C-100 | `src/root_view/tab_bar_section/title_bar.rs:588`–591 | `RootView` | warp/app/src/workspace/view.rs:17617-17758 render_new_session_button | warp 本地仓库 HEAD 5bf047181（= warp-compatibility.toml 的 integration_mirror）下，workspace/view.rs:17617-17758 现为 DrivePanelEvent 分发；render_new_session_button 在 :21461。 | 删除行号，保留「复合按钮 = [Plus] + [ChevronDown]…」的几何说明。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-100", "cls": "C", "file": "src/root_view/tab_bar_section/title_bar.rs", "key": "// warp/app/src/workspace/view.rs:17617-17758 render_new_session_button: // 复合按钮 = [Plus button] + [ChevronDown menu button], // 整体外层圆角 4，左半 with_left 圆角，右半 with_right 圆角； // 外层 hover 时整体提亮 (neutral_1", "decision": "", "done": ""} -->

## 批次 rdp-1

### B 为捷径辩护

#### B-011 `src/rdp_session/clipboard/dib.rs:4` `(顶层)`

<!-- entry {"no": "B-011", "cls": "B", "file": "src/rdp_session/clipboard/dib.rs", "key": "// 解析只有 macOS 的辅助进程在用。", "decision": "", "done": ""} -->
> // 解析只有 macOS 的辅助进程在用。

- 捷径：为 #![cfg_attr(not(target_os = "macos"), allow(dead_code))]（dib.rs:5）辩护：非 macOS 构建下 dib_to_bitmap 及其辅助函数没有调用方，用 allow(dead_code) 压掉警告
- 豁免：dead_code（仅非 macOS 构建）；理由：解析只有 macOS 的辅助进程在用；去掉后还报不报错：未核实：去掉后非 macOS 构建应对 dib_to_bitmap/shift_of/常量 BI_BITFIELDS 报 dead_code；验证命令 cargo check --target x86_64-pc-windows-gnu（本批次禁止编译）
- 是否仍然成立：是。证据：dib_to_bitmap 只在 helper_process.rs:389 被调用，而 helper_process 模块仅在 clipboard/mod.rs:13-14 的 #[cfg(target_os = "macos")] 下编译；bitmap_to_dib 在 mod.rs:505 无 cfg 使用。因此 dib_to_bitmap / shift_of / BI_BITFIELDS 在非 macOS 下确实无人调用（测试里有调用，cfg(test) 下不报）
- 建议：保留
- 风险：低：理由成立；仅是整模块级豁免，范围比 #[cfg(any(target_os="macos", test))] 标在 dib_to_bitmap 与 shift_of 上略粗，将来别处新增死代码在非 macOS 下会被一并压住
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-012 `src/rdp_session/clipboard/download.rs:409` `process_alive`

<!-- entry {"no": "B-012", "cls": "B", "file": "src/rdp_session/clipboard/download.rs", "key": "// SAFETY: 信号 0 只检查进程是否存在，不发送信号。", "decision": "", "done": ""} -->
> // SAFETY: 信号 0 只检查进程是否存在，不发送信号。

- 捷径：unsafe { libc::kill(pid, 0) } 探测进程是否存在（download.rs:410）
- 是否仍然成立：是。证据：kill(pid, 0) 不发信号、不涉及内存；pid 经 libc::pid_t::try_from 转换，失败时已返回 false（download.rs:406）；ESRCH 以外 EPERM 视为存活（download.rs:411）。理由与 POSIX 语义一致
- 建议：保留
- 风险：低：理由成立。pid 为 0 时 kill(0,0) 检查进程组，但目录名 pid 来自 std::process::id() 写入的名字，实际不会为 0
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-013 `src/rdp_session/clipboard/helper_process.rs:41` `(顶层)`

<!-- entry {"no": "B-013", "cls": "B", "file": "src/rdp_session/clipboard/helper_process.rs", "key": "// SAFETY: NSObject 没有子类化要求；Provider 不实现 Drop。", "decision": "", "done": ""} -->
> // SAFETY: NSObject 没有子类化要求；Provider 不实现 Drop。

- 捷径：define_class! 的 #[unsafe(super(NSObject))]：声明继承 NSObject 的子类 Provider（helper_process.rs:42）
- 是否仍然成立：是。证据：objc2 0.6.4（Cargo.lock）define_class 文档 # Safety 只要求：遵守父类不变量、若实现 Drop 则不得调用被覆盖的方法/延长 retain。NSObject 无子类化限制；Provider（helper_process.rs:40-60）没有 Drop 实现，ivars 只有 u64/u32，满足。同一宏内 provide_data_for_type 的参数类型与 NSPasteboardItemDataProvider 的 pasteboard:item:provideDataForType: 签名一致
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-014 `src/rdp_session/clipboard/helper_process.rs:396` `bitmap_rep`

<!-- entry {"no": "B-014", "cls": "B", "file": "src/rdp_session/clipboard/helper_process.rs", "key": "// SAFETY: data 传空由 CG 分配并持有，行距 width*4。", "decision": "", "done": ""} -->
> // SAFETY: data 传空由 CG 分配并持有，行距 width*4。

- 捷径：unsafe CGBitmapContextCreate，传 data=null 让 CG 自己分配，bytesPerRow=width*4（helper_process.rs:397）
- 是否仍然成立：是。证据：CGBitmapContextCreate 允许 data 为 NULL（CG 分配并持有），bytesPerRow 必须不小于 width*每像素字节；8 位 + PremultipliedFirst\|Order32Little 是 4 字节/像素，width*4 合法；参数非法时返回 None 已由 ? 处理（helper_process.rs:407）。width/height 来自 dib_to_bitmap（dib.rs:45-47，已 checked_mul）
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-015 `src/rdp_session/clipboard/helper_process.rs:412` `bitmap_rep`

<!-- entry {"no": "B-015", "cls": "B", "file": "src/rdp_session/clipboard/helper_process.rs", "key": "// SAFETY: 上下文按上面的尺寸分配了 width*height*4 字节，与 bgra 等长。", "decision": "", "done": ""} -->
> // SAFETY: 上下文按上面的尺寸分配了 width*height*4 字节，与 bgra 等长。

- 捷径：unsafe slice::from_raw_parts_mut(dst, bitmap.bgra.len())，把 CG 分配的缓冲当 bgra 等长的可写切片（helper_process.rs:413）
- 是否仍然成立：是。证据：CG 缓冲大小 = bytesPerRow*height = width*4*height；dib_to_bitmap（dib.rs:71）构造 bgra 时恰好 width*height*4，bitmap_rep 唯一调用方是 dib_rep（helper_process.rs:389）。context 在切片使用期间一直存活。但 bitmap_rep 本身没校验 bgra.len() == width*height*4：若将来有调用方传入更长的 bgra（Bitmap 字段是 pub(super)，bitmap_to_dib 就允许 bgra 更长，dib.rs:17），会越界写
- 建议：保留
- 风险：低：现状成立，依赖「唯一调用方保证等长」这个隐含不变量；要自包含可把长度改成 width*height*4 再与 bgra 比较（那是改代码，由维护者决定）
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-016 `src/rdp_session/clipboard/local_mac.rs:31` `string_type`

<!-- entry {"no": "B-016", "cls": "B", "file": "src/rdp_session/clipboard/local_mac.rs", "key": "// AppKit 导出的常量字符串，进程内常驻。", "decision": "", "done": ""} -->
> // AppKit 导出的常量字符串，进程内常驻。

- 捷径：读取 AppKit 的 extern static（NSPasteboardTypeString 等）要 unsafe，这条注释是它们的安全理由，但没写 SAFETY: 前缀（local_mac.rs:32-46）
- 是否仍然成立：是。证据：objc2-app-kit 0.3.2 中这些常量是 extern static &amp;'static NSPasteboardType，读取需 unsafe；它们是 AppKit 导出的常量字符串，进程内常驻，返回 &amp;'static 合理（local_mac.rs:33,36,39,42,45 五处 unsafe）
- 建议：保留
- 风险：低：理由成立；只是格式不合 SAFETY: 惯例，一条注释覆盖五处 unsafe，若后续检查（clippy::undocumented_unsafe_blocks）要求逐块 SAFETY 会报
- 备注：不是 // SAFETY: 前缀但功能上是 unsafe 的理由，按 B 归
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-017 `src/rdp_session/clipboard/local_mac.rs:172` `decode_bitmap`

<!-- entry {"no": "B-017", "cls": "B", "file": "src/rdp_session/clipboard/local_mac.rs", "key": "// SAFETY: bgra 恰好 width*height*4 字节，context 在 bgra 之前释放。", "decision": "", "done": ""} -->
> // SAFETY: bgra 恰好 width*height*4 字节，context 在 bgra 之前释放。

- 捷径：unsafe CGBitmapContextCreate，把 Rust 的 bgra Vec 缓冲交给 CG 作为位图上下文的后备存储（local_mac.rs:173）
- 是否仍然成立：是。证据：bgra = vec![0u8; width*height*4]（local_mac.rs:166，checked_mul），bytesPerRow=width*4，8 位 PremultipliedFirst\|Order32Little 为 4 字节/像素，缓冲够大；context 声明在 bgra 之后，且 drop(context)（local_mac.rs:191）在 bgra 被移入 Bitmap 之前，CG 不会在缓冲释放/移动后再写。context 创建之后到 drop 之间没有 ? 提前返回，bgra 不会先于 context 释放
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-018 `src/rdp_session/clipboard/wire.rs:3` `(顶层)`

<!-- entry {"no": "B-018", "cls": "B", "file": "src/rdp_session/clipboard/wire.rs", "key": "// 编解码只有 macOS 的辅助进程两端在用，其他平台只用到类型。", "decision": "", "done": ""} -->
> // 编解码只有 macOS 的辅助进程两端在用，其他平台只用到类型。

- 捷径：为 #![cfg_attr(not(target_os = "macos"), allow(dead_code))]（wire.rs:4）辩护：编解码函数只有 macOS 辅助进程两端调用，非 macOS 构建下是死代码
- 豁免：dead_code（仅非 macOS 构建，整个 wire 模块）；理由：编解码只有 macOS 的辅助进程两端在用，其他平台只用到类型；去掉后还报不报错：未核实：去掉后非 macOS 构建应对 write_to_helper/read_to_helper/write_request/read_request 等报 dead_code；验证命令 cargo check --target x86_64-pc-windows-gnu（本批次禁止编译）
- 是否仍然成立：是。证据：write_to_helper/read_request 只在 helper_mac.rs:95/104 调用（该文件仅 macOS 编译，mod.rs:10-11），read_to_helper/write_request 只在 helper_process.rs:112/315（mod.rs:13 仅 macOS）。非 macOS 的 helper_other.rs:3 只引用 Data/Offer/Request 类型，remote.rs/download.rs 也只用类型。因此非 macOS 下编解码函数及 ToHelper 部分变体确实未使用
- 建议：保留
- 风险：低：理由成立；整模块级豁免会把将来非 macOS 下真正多余的代码一起压住
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-019 `src/rdp_session/egfx/decoder_vt.rs:72`–73 `unsafe impl Send for VtH264Decoder {}`

<!-- entry {"no": "B-019", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: VTDecompressionSession/CMFormatDescription 均为线程安全的 CF 对象， // 且本解码器只在单一 RDP 会话线程内被调用（DVC 同步处理），满足 Send 契约。", "decision": "", "done": ""} -->
> // SAFETY: VTDecompressionSession/CMFormatDescription 均为线程安全的 CF 对象，
> // 且本解码器只在单一 RDP 会话线程内被调用（DVC 同步处理），满足 Send 契约。

- 捷径：unsafe impl Send for VtH264Decoder（decoder_vt.rs:74）：持有 CFRetained&lt;VTDecompressionSession/CMFormatDescription>，手动声明可跨线程移动
- 是否仍然成立：是。证据：VtH264Decoder 由 build_dvc_client 创建（egfx/mod.rs:101-102）后随 DVC 客户端交给 RDP 会话线程，且 Avc444Stream 内也持有一个（avc444_stream.rs:17-20），所有调用在同一线程串行进行（decode 经 &amp;mut self）。CF 对象的 retain/release 线程安全；VT 解码会话可被任意线程使用但不应并发调用，这里没有并发。Apple 文档对 VTDecompressionSession 的线程安全说明较含糊，未找到明文保证，不过按 &amp;mut self 独占使用是稳的
- 建议：保留
- 风险：低：理由成立
- 备注：理由第二句「只在单一线程内被调用」与 Send（可换线程）的诉求措辞略混，实质是「同一时刻只有一个线程用」
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-020 `src/rdp_session/egfx/decoder_vt.rs:147`–148 `VtH264Decoder.build_current_session`

<!-- entry {"no": "B-020", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: sps/pps 为本结构体持有的存活切片，长度非零；建 format desc + session // 均按 Create 规则取回 +1 引用，失败返回状态码。", "decision": "", "done": ""} -->
> // SAFETY: sps/pps 为本结构体持有的存活切片，长度非零；建 format desc + session
> // 均按 Create 规则取回 +1 引用，失败返回状态码。

- 捷径：unsafe build_session(&amp;self.sps,&amp;self.pps)（decoder_vt.rs:149）
- 是否仍然成立：是。证据：build_current_session 已在 :144 检查 sps/pps 非空；build_session 内 NonNull::new(.as_ptr()) 对切片恒非空，ptrs/sizes 为栈上数组（:354-358）；Create 规则返回 +1 由 CFRetained::from_raw 接管（:377,:401）。理由成立
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-021 `src/rdp_session/egfx/decoder_vt.rs:202`–203 `VtH264Decoder.decode_pixel_buffer`

<!-- entry {"no": "B-021", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// 首次尝试。SAFETY: vcl 在本调用期间存活；block buffer 用 kCFAllocatorNull 零拷贝 // 引用它、不负责释放；sample buffer + 同步 decode_frame 全程持有引用，回调同步写出。", "decision": "", "done": ""} -->
> // 首次尝试。SAFETY: vcl 在本调用期间存活；block buffer 用 kCFAllocatorNull 零拷贝
> // 引用它、不负责释放；sample buffer + 同步 decode_frame 全程持有引用，回调同步写出。

- 捷径：unsafe decode_sync(session,&amp;vcl)：block buffer 用 kCFAllocatorNull 零拷贝引用 vcl，依赖 VT 在 decode_frame 返回前用完输入（decoder_vt.rs:206）
- 是否仍然成立：是。证据：vcl 是 decode_pixel_buffer 的局部 Vec（:174），活过 decode_sync 调用；decode_sync 内 block/sample 在函数结束时释放（:419-465），flags=0 即同步解码（:467-480，VT 文档：不设 EnableAsynchronousDecompression 则回调在返回前触发）。vcl.is_empty 已在 :195 拦掉，data.as_ptr 非悬空。未发现 VT 在返回后继续读取输入的依据
- 建议：保留
- 风险：低：理由成立
- 备注：首句「首次尝试。」是叙述，另含对重试路径的说明（见 0242）
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-022 `src/rdp_session/egfx/decoder_vt.rs:215` `VtH264Decoder.decode_pixel_buffer`

<!-- entry {"no": "B-022", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: 同上，vcl/会话均存活。", "decision": "", "done": ""} -->
> // SAFETY: 同上，vcl/会话均存活。

- 捷径：同 0241，重试时再次 unsafe decode_sync（decoder_vt.rs:216）
- 是否仍然成立：是。证据：同 0241：vcl 仍是同一局部变量，session 是 build_current_session 刚建好的 self.session，存活
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-023 `src/rdp_session/egfx/decoder_vt.rs:256` `VtH264Decoder.decode_nv12_ranged`

<!-- entry {"no": "B-023", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: pixel_buffer 存活；with_locked_nv12 锁定期间借出平面、结束前解锁。", "decision": "", "done": ""} -->
> // SAFETY: pixel_buffer 存活；with_locked_nv12 锁定期间借出平面、结束前解锁。

- 捷径：unsafe with_locked_nv12(&amp;pixel_buffer, …)（decoder_vt.rs:257）
- 是否仍然成立：是。证据：pixel_buffer 是 CFRetained 局部变量，活过整个调用；with_locked_nv12 在返回前 Unlock（:562-565）；闭包 f 的参数是高阶生命周期，R 无法借用平面，切片不会逃逸（with_locked_nv12 签名 :509-512）
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-024 `src/rdp_session/egfx/decoder_vt.rs:282` `VtH264Decoder.decode`

<!-- entry {"no": "B-024", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: pixel_buffer 存活；读平面前锁定基址、读后解锁。", "decision": "", "done": ""} -->
> // SAFETY: pixel_buffer 存活；读平面前锁定基址、读后解锁。

- 捷径：unsafe read_pixel_buffer(&amp;pixel_buffer)（decoder_vt.rs:283）
- 是否仍然成立：是。证据：pixel_buffer 活过调用；read_pixel_buffer 内部走 with_locked_nv12 锁定/解锁（:570-582）。read_pixel_buffer 的参数本来是引用，「存活」由借用检查保证，unsafe 的必要性主要来自内部 FFI
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-025 `src/rdp_session/egfx/decoder_vt.rs:339` `unsafe extern "C-unwind" fn output_callb`

<!-- entry {"no": "B-025", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: 同步解码，source_frame_ref_con 指向 decode_sync 栈上仍存活的 DecodeOutput。", "decision": "", "done": ""} -->
> // SAFETY: 同步解码，source_frame_ref_con 指向 decode_sync 栈上仍存活的 DecodeOutput。

- 捷径：unsafe { &amp;mut *(source_frame_ref_con as *mut DecodeOutput) }（decoder_vt.rs:340）
- 是否仍然成立：是。证据：ref_con 来自 decode_sync 传入的 &amp;mut output（:468-479），同步模式下回调在 decode_frame 返回前触发，output 在 decode_sync 栈上仍存活；:336 已判空。依赖「flags=0 即同步」这一点（VT 文档）。若将来改成异步解码此假设会失效，应同步修改
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-026 `src/rdp_session/egfx/decoder_vt.rs:344` `unsafe extern "C-unwind" fn output_callb`

<!-- entry {"no": "B-026", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: 成功时 VT 传入有效 image buffer；retain +1 保活到 decode_sync 读取后释放。", "decision": "", "done": ""} -->
> // SAFETY: 成功时 VT 传入有效 image buffer；retain +1 保活到 decode_sync 读取后释放。

- 捷径：unsafe CFRetained::retain(buf)，把回调里借来的 image buffer 提升为 +1（decoder_vt.rs:345）
- 是否仍然成立：是。证据：仅 status==0 且 NonNull 时执行（:342-343）；VT 回调传入的 imageBuffer 在回调期间有效，retain 后由 into_raw 持有，decode_sync 再 from_raw 接管（:493-494）或失败时释放（:483-485）。所有权闭环成立
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-027 `src/rdp_session/egfx/decoder_vt.rs:361` `build_session`

<!-- entry {"no": "B-027", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: ptrs/sizes 为本函数栈上存活数组，长度 2；format_out 为有效可写指针。", "decision": "", "done": ""} -->
> // SAFETY: ptrs/sizes 为本函数栈上存活数组，长度 2；format_out 为有效可写指针。

- 捷径：unsafe CMVideoFormatDescriptionCreateFromH264ParameterSets（decoder_vt.rs:362）
- 是否仍然成立：是。证据：ptrs/sizes 是函数栈上长度 2 的数组，sps/pps 切片在调用期间存活（参数是 &amp;[u8] 借用），format_out 是栈上局部；nalUnitHeaderLength=4 与 AVCC 一致。API 只读指针内容，as *mut 转换是 objc2 签名所迫
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-028 `src/rdp_session/egfx/decoder_vt.rs:376` `build_session`

<!-- entry {"no": "B-028", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: Create 规则返回 +1，from_raw 接管所有权。", "decision": "", "done": ""} -->
> // SAFETY: Create 规则返回 +1，from_raw 接管所有权。

- 捷径：unsafe CFRetained::from_raw(format)（decoder_vt.rs:377）
- 是否仍然成立：是。证据：CMVideoFormatDescriptionCreate… 遵守 Create 规则返回 +1，且 status==0 且已做 NonNull 判空（:372-375），所有权转移正确
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-029 `src/rdp_session/egfx/decoder_vt.rs:384`–385 `build_session`

<!-- entry {"no": "B-029", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: format 存活；callback 指向本栈上记录；session_out 有效可写。 // 目标像素属性传 None → 默认输出 NV12（bi-planar 4:2:0）。", "decision": "", "done": ""} -->
> // SAFETY: format 存活；callback 指向本栈上记录；session_out 有效可写。
> // 目标像素属性传 None → 默认输出 NV12（bi-planar 4:2:0）。

- 捷径：unsafe VTDecompressionSession::create(...&amp;callback...)（decoder_vt.rs:386）
- 是否仍然成立：是。证据：format 是 CFRetained 局部，存活；callback 是栈上 VTDecompressionOutputCallbackRecord，refCon 为空，只含函数指针（:379-382）。该调用返回后栈上 record 即失效，这里依赖 VT 在 create 时复制 record；Apple 文档未明文保证，但 Swift/ObjC 示例普遍用栈上 record 传入，未发现问题
- 建议：保留
- 风险：低：理由成立
- 备注：另含一句「目标像素属性传 None → 默认输出 NV12」，是对参数选择的说明（C 性质）
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-030 `src/rdp_session/egfx/decoder_vt.rs:400` `build_session`

<!-- entry {"no": "B-030", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: Create 规则返回 +1，from_raw 接管。", "decision": "", "done": ""} -->
> // SAFETY: Create 规则返回 +1，from_raw 接管。

- 捷径：unsafe CFRetained::from_raw(session)（decoder_vt.rs:401）
- 是否仍然成立：是。证据：VTDecompressionSessionCreate 遵守 Create 规则返回 +1，status==0 且已判空（:396-399）
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-031 `src/rdp_session/egfx/decoder_vt.rs:403`–404 `build_session`

<!-- entry {"no": "B-031", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// RealTime 提示（低延迟优先）；失败仅影响延迟，忽略。 // SAFETY: session Deref 到 &CFType；key/value 为静态 CF 对象。", "decision": "", "done": ""} -->
> // RealTime 提示（低延迟优先）；失败仅影响延迟，忽略。
> // SAFETY: session Deref 到 &amp;CFType；key/value 为静态 CF 对象。

- 捷径：unsafe VTSessionSetProperty(&amp;session, RealTime, true)（decoder_vt.rs:405-411），并 let _ = 忽略返回值
- 是否仍然成立：是。证据：key 是 extern static，value 是 kCFBooleanTrue（静态 CF 对象），session 存活，签名要求的 &amp;CFType 经 Deref 得到。「失败仅影响延迟，忽略」成立：RealTime 只是低延迟提示，失败不影响正确性；代价是失败时不会有任何日志
- 建议：保留
- 风险：低：理由成立
- 备注：同时含 C 性质的一句「RealTime 提示（低延迟优先）」；忽略返回值本身是个小捷径但理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-032 `src/rdp_session/egfx/decoder_vt.rs:421` `decode_sync`

<!-- entry {"no": "B-032", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: data 存活于整个 decode_sync；block_out 可写。", "decision": "", "done": ""} -->
> // SAFETY: data 存活于整个 decode_sync；block_out 可写。

- 捷径：unsafe CMBlockBuffer::create_with_memory_block 引用 data 内存（decoder_vt.rs:422）
- 是否仍然成立：是。证据：data 是参数 &amp;[u8]，活过整个 decode_sync；kCFAllocatorNull 表示不释放；data.as_ptr() as *mut 是签名所迫，VT 只读。调用方保证 data 非空（decoder_vt.rs:195）
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-033 `src/rdp_session/egfx/decoder_vt.rs:439` `decode_sync`

<!-- entry {"no": "B-033", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: Create 规则 +1。", "decision": "", "done": ""} -->
> // SAFETY: Create 规则 +1。

- 捷径：unsafe CFRetained::from_raw(block)（decoder_vt.rs:440）
- 是否仍然成立：是。证据：CMBlockBufferCreateWithMemoryBlock 遵守 Create 规则 +1，status==0 且已判空（:435-438）
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-034 `src/rdp_session/egfx/decoder_vt.rs:446` `decode_sync`

<!-- entry {"no": "B-034", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: block/format 存活；sizes 为栈上数组；sample_out 可写。timing 传 0/null。", "decision": "", "done": ""} -->
> // SAFETY: block/format 存活；sizes 为栈上数组；sample_out 可写。timing 传 0/null。

- 捷径：unsafe CMSampleBuffer::create_ready（decoder_vt.rs:447）
- 是否仍然成立：是。证据：block、vt.format 均为存活的 CFRetained；sizes 是长度 1 的栈数组，numSamples=1、numSampleSizeEntries=1 匹配（:444-456）；timing 传 0/null 对单帧同步解码可接受
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-035 `src/rdp_session/egfx/decoder_vt.rs:464` `decode_sync`

<!-- entry {"no": "B-035", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: Create 规则 +1。", "decision": "", "done": ""} -->
> // SAFETY: Create 规则 +1。

- 捷径：unsafe CFRetained::from_raw(sample)（decoder_vt.rs:465）
- 是否仍然成立：是。证据：CMSampleBufferCreateReady 遵守 Create 规则 +1，已判空（:460-463）
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-036 `src/rdp_session/egfx/decoder_vt.rs:472` `decode_sync`

<!-- entry {"no": "B-036", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: sample 存活；ref_con 指向本栈 output；info_out 传 null。", "decision": "", "done": ""} -->
> // SAFETY: sample 存活；ref_con 指向本栈 output；info_out 传 null。

- 捷径：unsafe session.decode_frame(&amp;sample, flags 0, &amp;mut output as ref_con)（decoder_vt.rs:473）
- 是否仍然成立：是。证据：sample 存活；ref_con 指向 decode_sync 栈上的 output，同步模式下回调返回前完成，之后不再被 VT 使用；infoFlagsOut 传 null 合法
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-037 `src/rdp_session/egfx/decoder_vt.rs:484` `decode_sync`

<!-- entry {"no": "B-037", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: 若回调已 retain，取回并释放。", "decision": "", "done": ""} -->
> // SAFETY: 若回调已 retain，取回并释放。

- 捷径：unsafe CFRetained::from_raw(img) 释放回调留下的 image（decoder_vt.rs:485）
- 是否仍然成立：是。证据：output.image 只会由 output_callback 经 CFRetained::into_raw 写入（:346），为 +1 的 CVPixelBuffer 指针，此处取回恰好释放一次；失败路径之后 output 不再使用
- 建议：保留
- 风险：低：理由成立
- 备注：紧邻上一行注释 0272 的措辞与此处相反，见 0272
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-038 `src/rdp_session/egfx/decoder_vt.rs:493` `decode_sync`

<!-- entry {"no": "B-038", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: 回调 retain 的 +1，from_raw 接管，调用方用完释放。", "decision": "", "done": ""} -->
> // SAFETY: 回调 retain 的 +1，from_raw 接管，调用方用完释放。

- 捷径：unsafe CFRetained::from_raw(image) 接管回调 retain 的引用（decoder_vt.rs:494）
- 是否仍然成立：是。证据：image 来自 output.image，非空已判（:492）；回调 +1 的引用转给返回值，调用方 drop 时释放；此后 output 不再访问 image，无双重释放
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-039 `src/rdp_session/egfx/decoder_vt.rs:532`–533 `with_locked_nv12`

<!-- entry {"no": "B-039", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// 只读锁（flags=0 即可读写，用 0 简单稳妥）。 // SAFETY: pb 有效；锁定后必配对解锁。", "decision": "", "done": ""} -->
> // 只读锁（flags=0 即可读写，用 0 简单稳妥）。
> // SAFETY: pb 有效；锁定后必配对解锁。

- 捷径：CVPixelBufferLockBaseAddress 用 flags=0（读写锁）而不用 ReadOnly，注释里说「只读锁（flags=0 即可读写，用 0 简单稳妥）」（decoder_vt.rs:532-534，对应解锁 :564）
- 是否仍然成立：是。证据：注释自相矛盾：只读锁对应 CVPixelBufferLockFlags::ReadOnly（objc2-core-video 0.3.2 generated/CVPixelBuffer.rs:229，值 0x1），而这里传的是 0，即读写锁。三处平面读取都只读（with_locked_nv12 的调用方 decoder_vt.rs:257-263、:572-580）。Apple 文档建议只读访问时传 ReadOnly，以免触发缓存失效；读写锁不会错，只是可能多一点开销。SAFETY 部分（pb 有效、锁后配对解锁）成立：解锁在 :563-565，且无提前返回（f 内 panic 除外）
- 建议：修代码
- 风险：低：功能正确，只是 flags 与注释说法不符，且 ReadOnly 才是语义准确的写法；改动时 Lock 与 Unlock 的 flags 必须一致
- 备注：SAFETY 部分成立；「简单稳妥」是为省事辩护的那半句
- 决定：保留
- 拍板备注：推翻「修代码」。传 0 是读写锁，功能正确；换成 ReadOnly 只是语义更准，属风格类，又在 EGFX 解码热路径上、要真机回归，不动代码。注释首句自相矛盾，改成如实描述：「flags=0 是读写锁，只读访问同样适用」；SAFETY 行不变。
- 完成：2026-11-22（保留，不动）

#### B-040 `src/rdp_session/egfx/decoder_vt.rs:539` `with_locked_nv12`

<!-- entry {"no": "B-040", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: 已锁定，基址/stride 在解锁前有效；平面切片只在 f 内使用，不逃逸出锁定区间。", "decision": "", "done": ""} -->
> // SAFETY: 已锁定，基址/stride 在解锁前有效；平面切片只在 f 内使用，不逃逸出锁定区间。

- 捷径：unsafe 构造平面切片 slice::from_raw_parts（decoder_vt.rs:540-559）
- 是否仍然成立：是。证据：已锁定，y_base/uv_base 判空，长度取 stride*plane_height（saturating_mul）；切片的生命周期由 LockedNv12&lt;'_> 绑在闭包内，f 的返回值 R 无法借走它（with_locked_nv12 签名），解锁在其后 :563。理由成立；Nv12Frame::new 还会再校验平面长度（avc444.rs:21-45）
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-041 `src/rdp_session/egfx/decoder_vt.rs:562` `with_locked_nv12`

<!-- entry {"no": "B-041", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: 与上面的 lock 配对解锁。", "decision": "", "done": ""} -->
> // SAFETY: 与上面的 lock 配对解锁。

- 捷径：unsafe CVPixelBufferUnlockBaseAddress，与 Lock 配对（decoder_vt.rs:563）
- 是否仍然成立：是。证据：Lock 成功才会走到这里（失败时 :535-537 已提前返回），之间无提前返回；Unlock 的 flags 与 Lock 一致（均为 0）
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-042 `src/rdp_session/egfx/decoder_vt.rs:571` `read_pixel_buffer`

<!-- entry {"no": "B-042", "cls": "B", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// SAFETY: 由调用方保证 pb 存活。", "decision": "", "done": ""} -->
> // SAFETY: 由调用方保证 pb 存活。

- 捷径：unsafe with_locked_nv12(pb, …)（decoder_vt.rs:572）
- 是否仍然成立：是。证据：pb 是 &amp;CVPixelBuffer 借用，存活由借用检查保证；「调用方保证」其实是空话，read_pixel_buffer 标 unsafe fn 的必要性很弱（内部 FFI 已各自带 SAFETY）。不影响正确性
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-043 `src/rdp_session/egfx/mod.rs:397` `EgfxHandler.on_map_surface_to_scaled_output`

<!-- entry {"no": "B-043", "cls": "B", "file": "src/rdp_session/egfx/mod.rs", "key": "// 缩放输出：v1 只取原点，忽略 target 缩放（少见；缩放渲染后续再做）。", "decision": "", "done": ""} -->
> // 缩放输出：v1 只取原点，忽略 target 缩放（少见；缩放渲染后续再做）。

- 捷径：MapSurfaceToScaledOutput 只取原点、忽略 target 缩放，「缩放渲染后续再做」（egfx/mod.rs:396-413）
- 是否仍然成立：是。证据：on_map_surface_to_scaled_output 只调用 compositor.map_surface(surface_id, output_origin_x, output_origin_y)，target_width/target_height 仅用于 trace 打印（mod.rs:396-413）；未找到其他缩放处理。「少见」未核实（需用真机或 wire dump 统计服务端是否发过 MapSurfaceToScaledOutput / MapSurfaceToScaledWindow）
- 建议：记待办
- 风险：低：服务端发缩放映射时（高 DPI 缩放场景可能）画面按 1:1 贴到原点，会错位/大小不对；少见所以影响面小
- 决定：记待办
- 拍板备注：待办内容：先在真机开 `NEXSHELL_RDP_EGFX_TRACE`，确认服务端会不会发 MapSurfaceToScaledOutput（日志里的 `map_scaled`）；发了再做缩放渲染，一直没见过就关掉这条待办。
- 完成：2026-11-22（T-004）

#### B-044 `src/rdp_session/egfx/surfaces.rs:620`–623 `progressive_paint_plan`

<!-- entry {"no": "B-044", "cls": "B", "file": "src/rdp_session/egfx/surfaces.rs", "key": "// 这里再解一遍 progressive stream，是为取本 PDU 的 REGION rects 与全量 tile 坐标： // 库返回的 DecodedTile.update_rectangles 只覆盖本 PDU 解出的 tile，裁不了 ADR 0008 第⑤步 // 要求的\"用本 PDU rects 裁剪先前 PDU 累积 tile（frame.tile_pixels）\"，删掉会回", "decision": "", "done": ""} -->
> // 这里再解一遍 progressive stream，是为取本 PDU 的 REGION rects 与全量 tile 坐标：
> // 库返回的 DecodedTile.update_rectangles 只覆盖本 PDU 解出的 tile，裁不了 ADR 0008 第⑤步
> // 要求的"用本 PDU rects 裁剪先前 PDU 累积 tile（frame.tile_pixels）"，删掉会回归拖窗描边残留。
> // 改进方向：让 fork 的 decode_bitmap 顺带把 REGION rects 一起返回。

- 捷径：write_progressive 之外再用 decode_progressive_stream 把 PDU 解析一遍（progressive_paint_plan，surfaces.rs:624-639），为拿 REGION rects 和 tile 坐标；注释把原因记作「库返回的 update_rectangles 只覆盖本 PDU 解出的 tile」，并留了「让 fork 的 decode_bitmap 顺带返回 REGION rects」的改进方向
- 是否仍然成立：未核实。证据：部分过期：当前钉的 IronRDP fork（Cargo.lock 538af770）的 ironrdp-graphics progressive.rs:1148-1158 DecodedTile 已有 update_rectangles（REGION rects ∩ tile，逐 tile 裁好），progressive.rs:1487-1532 还会按 frame_tiles 把先前 REGION 的 tile 一起重建并返回，begin_frame/end_frame 见 :1310-1319；这正是注释想要的「顺带返回 REGION rects」，提交来自上游 #1443（fork 提交 11b10531）。但 nexshell 自己持有 ProgressiveDecoder 且从不调用 begin_frame/end_frame（grep src/rdp_session 无命中），frame_active=false 时库在 :1441-1443 每次清掉 frame_tiles，所以「只覆盖本 PDU」对现用法仍成立；docs/adr/0008-egfx-graphics-pipeline.md:112-113 记了 ADR 0011 追平后刻意保留由 nexshell 侧裁剪。需要确认的是：改成在 EndFrame 处调 end_frame、在帧开始调 begin_frame 并直接用 tile.update_rectangles，能否去掉 progressive_paint_plan 和 tile_pixels 缓存而不复现拖窗描边残留（ADR 0008 第⑤步）。验证方式：真机键盘移窗 7 轮 + 鼠标拖动 7 轮（ADR 0008 记录的复测方法）
- 建议：记待办
- 风险：低：现状功能正确，代价是每个 Progressive PDU 多解析一遍块头（不含 IDWT）以及一份重复的 tile 像素缓存；风险在于以后升级 fork 时两套裁剪逻辑可能不一致
- 决定：记待办
- 拍板备注：ADR 0008 把这段注释指定为「nexshell 侧裁剪」的说明：解释为什么要再解一遍的前三行保留，只把末句「改进方向」换成待办编号。待办内容：下一轮 IronRDP 追平时评估改用库的 `begin_frame` / `end_frame` 加 `DecodedTile.update_rectangles`，去掉二次解析和 `tile_pixels` 缓存；验收用 ADR 0008 第⑤步的复测（键盘移窗 7 轮 + 鼠标拖动 7 轮零残留）。
- 完成：2026-11-22（T-005）

#### B-045 `src/rdp_session/egfx/vimage.rs:141` `generate`

<!-- entry {"no": "B-045", "cls": "B", "file": "src/rdp_session/egfx/vimage.rs", "key": "// SAFETY: 三个指针都指向有效结构，vImage 只写 out。", "decision": "", "done": ""} -->
> // SAFETY: 三个指针都指向有效结构，vImage 只写 out。

- 捷径：unsafe 调 vImageConvert_YpCbCrToARGB_GenerateConversion（vimage.rs:142）
- 是否仍然成立：是。证据：matrix/range 是 &amp;T 引用（指针必有效），out 是栈上 Conversion([u8;128], align 16)，对应 vImage_YpCbCrToARGB 的不透明 128 字节结构；返回值非 0 即 Err（vimage.rs:147-151）。理由成立
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-046 `src/rdp_session/egfx/vimage.rs:169` `nv12_to_rgba`

<!-- entry {"no": "B-046", "cls": "B", "file": "src/rdp_session/egfx/vimage.rs", "key": "// vImage 只读源平面；Nv12Frame::new 已校验平面覆盖 w×h。", "decision": "", "done": ""} -->
> // vImage 只读源平面；Nv12Frame::new 已校验平面覆盖 w×h。

- 捷径：frame.y/uv 的 *const 经 cast_mut() 放进 vImage 的 Buffer（vimage.rs:170-181），注释是这次转换的安全理由（无 SAFETY: 前缀）
- 是否仍然成立：是。证据：vImage 的源缓冲在 C 签名里是 const vImage_Buffer*，只读；Nv12Frame::new（avc444.rs:21-45）校验 y_stride>=width、uv_stride>=half_w*2、平面长度覆盖整帧；nv12_to_rgba 又拒绝奇数宽高、校验 dst 长度（vimage.rs:157-162）。crop() 出的子帧也经 Nv12Frame::new（avc444.rs:52-59）。理由成立
- 建议：保留
- 风险：低：理由成立；若要符合 SAFETY: 惯例可补前缀，属风格问题
- 备注：不是 // SAFETY: 前缀，但功能上是 cast_mut + 后续 unsafe 调用的理由
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-047 `src/rdp_session/egfx/vimage.rs:188` `nv12_to_rgba`

<!-- entry {"no": "B-047", "cls": "B", "file": "src/rdp_session/egfx/vimage.rs", "key": "// SAFETY: 三个缓冲的尺寸与行距都在对应切片界内，info 由 GenerateConversion 生成。", "decision": "", "done": ""} -->
> // SAFETY: 三个缓冲的尺寸与行距都在对应切片界内，info 由 GenerateConversion 生成。

- 捷径：unsafe vImageConvert_420Yp8_CbCr8ToARGB8888（vimage.rs:189）
- 是否仍然成立：是。证据：Y 缓冲 h×w、行距 y_stride；CbCr 缓冲 h/2×w/2（每元素 2 字节）、行距 uv_stride，均由 Nv12Frame::new 校验覆盖；dest 长度 >= w*h*4 已检查（vimage.rs:160）；宽高为偶数已检查；info 来自 GenerateConversion（conversions()，vimage.rs:125-137）
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-048 `src/rdp_session/egfx/vimage.rs:286` `ayuv_to_rgba`

<!-- entry {"no": "B-048", "cls": "B", "file": "src/rdp_session/egfx/vimage.rs", "key": "// SAFETY: 两个缓冲的尺寸与行距都已按切片长度校验，info 由 GenerateConversion 生成。", "decision": "", "done": ""} -->
> // SAFETY: 两个缓冲的尺寸与行距都已按切片长度校验，info 由 GenerateConversion 生成。

- 捷径：unsafe vImageConvert_444AYpCbCr8ToARGB8888（vimage.rs:287）
- 是否仍然成立：是。证据：src.len()>=width*height*4、dst_stride>=width*4、dst.len()>=dst_stride*(height-1)+width*4 在 vimage.rs:267-272 校验（width/height 为 0 时已提前 Ok 返回）；info 为 ayuv_full。理由成立
- 建议：保留
- 风险：低：理由成立
- 决定：保留
- 完成：2026-11-22（保留，不动）

### C 过期的「为什么」

| 编号 | 位置 | 所在符号 | 过期的句子 | 依据 | 建议改成 | 决定 | 完成 |
|---|---|---|---|---|---|---|---|
| C-101 | `src/rdp_session/egfx/decoder_vt.rs:482` | `decode_sync` | 回调未触发或提前失败，无需释放 image。 | decoder_vt.rs:481-487：紧接着的代码正是「若 output.image 非空就 from_raw 释放」。decode_frame 返回非 0 时回调仍可能已执行并 retain 了 image（0273 的 SAFETY 也是这么说的），所以不是「无需释放」 | decode_frame 失败时回调可能已经 retain 了 image，要取回释放；回调没触发则 image 为空，什么都不用做。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-101", "cls": "C", "file": "src/rdp_session/egfx/decoder_vt.rs", "key": "// 回调未触发或提前失败，无需释放 image。", "decision": "", "done": ""} -->
| C-102 | `src/rdp_session/egfx/mod.rs:71`–72 | `build_dvc_client` | 现有 capabilities()（V8.1+V8）生效，服务端将选 AVC420（与下一行 0352 合起来读） | egfx/mod.rs:282-327：capabilities() 现经 advertised_capabilities(small_cache, avc444_enabled())，macOS 默认把 V10.7 排最前再带 V8.1、V8（ADR 0015）；同文件头注释 0339 也写 V10.7+V8.1+V8。库侧「decoder 接上后 start() 不再过滤 AVC 能力集」仍成立（ironrdp-egfx 538af77 client.rs:1285-1301，无 decoder 才过滤） | decoder 接上后，库 start() 不再过滤 AVC 能力集，capabilities() 原样发出（macOS 默认 V10.7+V8.1+V8，见 advertised_capabilities） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-102", "cls": "C", "file": "src/rdp_session/egfx/mod.rs", "key": "/// decoder 接上后，库 `start()` 不再过滤 AVC 能力集，现有 capabilities()（V8.1+V8）", "decision": "", "done": ""} -->
| C-103 | `src/rdp_session/egfx/mod.rs:72`–73 | `(顶层)` | 服务端将选 AVC420 | 同 0351：macOS 默认声明 V10.7，服务端可选 AVC444（egfx/mod.rs:282-287、avc444_stream 路径 mod.rs:678-705） | 服务端按能力选 AVC420 或 AVC444（仍会混发 ClearCodec 小块与 Progressive）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-103", "cls": "C", "file": "src/rdp_session/egfx/mod.rs", "key": "/// 生效，服务端将选 AVC420（仍会混发 ClearCodec 小块与 Progressive）。", "decision": "", "done": ""} -->
| C-104 | `src/rdp_session/egfx/surfaces.rs:235`–236 | `Compositor` | lib 已解好的 Uncompressed/AVC420 输出（RGBA） | write_bitmap 是 on_bitmap_updated 的落点（egfx/mod.rs:417-455），ClearCodec 同样经此写入（mod.rs:416 注释、surfaces.rs:4 文件头 0389 都把 ClearCodec 算进来） | lib 已解好的 Uncompressed/AVC420/ClearCodec 输出（RGBA）→ 写入 surface。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-104", "cls": "C", "file": "src/rdp_session/egfx/surfaces.rs", "key": "/// lib 已解好的 Uncompressed/AVC420 输出（RGBA）→ 写入 surface。", "decision": "", "done": ""} -->

## 批次 rdp-2

### B 为捷径辩护

#### B-049 `src/rdp_session/frame_marker.rs:40` `peek_frame_markers`

<!-- entry {"no": "B-049", "cls": "B", "file": "src/rdp_session/frame_marker.rs", "key": "// 分片/带压缩标志的 update 内容不完整或需解压，跳过 marker 扫描。", "decision": "", "done": ""} -->
> // 分片/带压缩标志的 update 内容不完整或需解压，跳过 marker 扫描。

- 捷径：只读 peek 遇到分片（fragmentation!=Single）或带压缩标志的 SurfaceCommands update 时直接 continue，不扫 FrameMarker
- 是否仍然成立：是。证据：src/rdp_session/frame_marker.rs:40-44 的跳过条件；peek 只读、不能重组分片或解压，理由成立。丢失 End 信号时由 src/rdp_session/mod.rs:894-897 的 50ms frame_deadline 兜底发布（mod.rs:1074 起 marker 模式分支）。连接配置 compression_type: None（mod.rs:480 附近），压缩分支实际走不到；分片分支是否会吞掉 End marker 未用真机抓包核实。
- 建议：保留
- 风险：低：最坏是该帧晚 ≤50ms 发布（延迟，不丢画面）；要确认分片是否真会携带 End marker，需用 NEXSHELL_RDP_EGFX_DUMP 或抓包看大帧的 FrameMarker 位置。
- 备注：行为是降级而非错误处理，归 B 供维护者确认是否接受
- 决定：保留
- 完成：

#### B-050 `src/rdp_session/stats.rs:90` `RdpStats.capture_fd`

<!-- entry {"no": "B-050", "cls": "B", "file": "src/rdp_session/stats.rs", "key": "// SAFETY: dup 一个当前有效的 socket fd；失败返回 -1，由 rtt_ms 忽略。", "decision": "", "done": ""} -->
> // SAFETY: dup 一个当前有效的 socket fd；失败返回 -1，由 rtt_ms 忽略。

- 捷径：SAFETY：声称 dup 的是当前有效的 socket fd，失败返回 -1 由 rtt_ms 忽略
- 是否仍然成立：是。证据：内存安全层面成立：src/rdp_session/stats.rs:90 的 libc::dup 失败只返回 -1，调用方 src/rdp_session/mod.rs:629 传的是存活 TcpStream 的 fd，rtt_ms 对 fd&lt;0 返回 None（stats.rs:97-99）。但同一 Arc&lt;RdpStats> 在整个会话共用（mod.rs:358 只建一次），而 connect_and_run 在重连循环里每轮都调用（mod.rs:538-541 -> 629），stats.rs:92 的 raw_fd.store 直接覆盖旧值、不关闭上一轮的 dup。每次自动重连漏一个 fd，并且 dup 让旧 TCP socket 在会话结束前不会真正关闭。
- 建议：修代码
- 风险：中：自动重连最多 20 次/轮（reconnect.rs MAX_ATTEMPTS），每次漏一个 fd 且旧连接 socket 不释放，长时间挂着网络不稳的会话会累积；改成 swap 取出旧 fd 并 close 即可（注意与 rtt_ms 读取的并发）。
- 备注：注释本身不删，只审查理由；理由对但没覆盖重复调用的情形，修代码后可补一句「重复调用先关旧 fd」
- 决定：修代码
- 拍板备注：确认是 bug，留在 rdp-2 批次修（见拍板记录）。同一个 `RdpStats` 整个会话共用，每次重连 `capture_fd` 覆盖 `raw_fd` 却不关上一轮的 dup：漏一个 fd，旧 socket 也被这个 dup 吊着，直到进程退出。回归用例：连续 `capture_fd` 两次后，第一次 dup 出来的 fd 已关闭。关旧 fd 和 UI 线程 `rtt_ms` 并发读怎么处理，由实现者定。
- 完成：

#### B-051 `src/rdp_session/stats.rs:102`–103 `RdpStats.rtt_ms`

<!-- entry {"no": "B-051", "cls": "B", "file": "src/rdp_session/stats.rs", "key": "// SAFETY: fd 为本会话 dup 的有效 TCP socket；info 为 POD、zeroed 合法， // len 恰为其尺寸，符合 getsockopt 契约。", "decision": "", "done": ""} -->
> // SAFETY: fd 为本会话 dup 的有效 TCP socket；info 为 POD、zeroed 合法，
> // len 恰为其尺寸，符合 getsockopt 契约。

- 捷径：SAFETY：getsockopt 读 TCP_CONNECTION_INFO，声称 fd 有效、info 零值合法、len 正确
- 是否仍然成立：是。证据：src/rdp_session/stats.rs:102-124：fd&lt;0 已提前返回；libc::tcp_connection_info 是纯整数 C 结构，mem::zeroed 合法；len 取 size_of 同类型；tcpi_srtt 单位按 macOS SDK netinet/tcp.h:269 为 ms。fd 仅在 Drop（需 &amp;mut）里 close，不会与 rtt_ms 并发关闭。唯一遗留是 233 条说的重连覆盖：若将来改成在 capture_fd 里关旧 fd，需要处理与 rtt_ms 的并发（读到被复用的 fd）。
- 建议：保留
- 风险：低：理由成立。
- 决定：保留
- 完成：

#### B-052 `src/rdp_session/stats.rs:133` `RdpStats.drop`

<!-- entry {"no": "B-052", "cls": "B", "file": "src/rdp_session/stats.rs", "key": "// SAFETY: fd 为本 stats 独占持有的 dup fd，仅此处 close 一次。", "decision": "", "done": ""} -->
> // SAFETY: fd 为本 stats 独占持有的 dup fd，仅此处 close 一次。

- 捷径：SAFETY：Drop 里 close 独占持有的 dup fd，称仅此处 close 一次
- 是否仍然成立：是。证据：src/rdp_session/stats.rs:133-140：raw_fd 只在 Drop 中 close，且 Drop 需要独占。字面成立，但只关得到最后一次 capture_fd 存入的 fd，重连前几轮存入的 dup 已被覆盖而漏掉（见 rdp-2#0233）。
- 建议：保留
- 风险：低：本条注释自身没错，问题在 capture_fd 一侧（#0233）。
- 决定：保留
- 完成：

### C 过期的「为什么」

| 编号 | 位置 | 所在符号 | 过期的句子 | 依据 | 建议改成 | 决定 | 完成 |
|---|---|---|---|---|---|---|---|
| C-105 | `src/rdp_session/mod.rs:4`–5 | `(顶层)` | 本步只做：TCP → NLA(CredSSP) → 能力协商 → 收图形更新 → 合成 framebuffer。 | 模块已含输入（mod.rs:935 input 分支）、剪贴板（clipboard 模块）、音频、驱动器、UDP、自动重连、EGFX。 | 删除这句，或改成「事件循环：TCP → TLS → NLA → 能力协商 → 收图形更新 → 合成 framebuffer」 | 按建议改：取前一种，删除这句 |  |<!-- entry {"no": "C-105", "cls": "C", "file": "src/rdp_session/mod.rs", "key": "//! 本步只做：TCP → NLA(CredSSP) → 能力协商 → 收图形更新 → 合成 framebuffer。", "decision": "", "done": ""} -->
| C-106 | `src/rdp_session/mod.rs:5`–6 | `(顶层)` | 输入/剪贴板留占位（RdpInputEvent + input_tx），后续步骤实现。 | input_tx 已在 mod.rs:935 消费并编码成 FastPath（to_fastpath_input），剪贴板见 src/rdp_session/clipboard/、mod.rs:1259 起 cliprdr 写回。 | 删除这句 | 按建议改 |  |<!-- entry {"no": "C-106", "cls": "C", "file": "src/rdp_session/mod.rs", "key": "//! 输入/剪贴板留占位（RdpInputEvent + input_tx），后续步骤实现。", "decision": "", "done": ""} -->
| C-107 | `src/rdp_session/mod.rs:77`–86 | `RdpSessionConfig（字段 enable_egfx 等）` | 第①步临时用 NEXSHELL_RDP_EGFX 环境变量门控，出画面后（第②步）改默认开。 | 全仓已无 NEXSHELL_RDP_EGFX（grep 为 0 命中）；现为 default_enable_egfx（mod.rs:93-96）默认开，用 NEXSHELL_RDP_DISABLE_EGFX 关。 | 只留「开 EGFX 图形管线（MS-RDPEGFX，docs/adr/0008）。默认开，见 default_enable_egfx。」 | 按建议改 |  |<!-- entry {"no": "C-107", "cls": "C", "file": "src/rdp_session/mod.rs", "key": "/// 第①步临时用 NEXSHELL_RDP_EGFX 环境变量门控，出画面后（第②步）改默认开。 /// RDPSND 音频重定向开关（仅输出方向，cpal 播放 + Opus 解码）。 /// RDPDR 驱动器重定向开关（`~/NexShell RDP` ↔ 远端 \\\\tsclient\\NexShell 文件互拷）。 /// 远端 DPI 缩放百分比（[100,500] 有效，0=不请求，", "decision": "", "done": ""} -->
| C-108 | `src/rdp_session/mod.rs:117`–118 | `DirtyRect` | 本步图形更新统一按整帧上报。 | outputs.rs 的 inclusive_to_dirty / union_dirty / publish_frame 按真实脏区累积发布；framebuffer.rs 的 dirty_since 按帧代号取脏区上传。 | 删除这句 | 按建议改 |  |<!-- entry {"no": "C-108", "cls": "C", "file": "src/rdp_session/mod.rs", "key": "/// 脏矩形（左上原点，像素）。本步图形更新统一按整帧上报。", "decision": "", "done": ""} -->
| C-109 | `src/rdp_session/mod.rs:314`–323 | `RdpSessionHandle` | 输入通道占位（本步不消费）。 | input_tx 由 spawn_rdp_session 建（mod.rs:353）、connect_and_run 消费（mod.rs:935），UI 在 root_view/rdp_section.rs:409 起发送。 | 输入通道：UI 发键鼠事件，会话线程消费编码成 FastPath。 | 按建议改 |  |<!-- entry {"no": "C-109", "cls": "C", "file": "src/rdp_session/mod.rs", "key": "/// framebuffer，generation 为权威），UI 只触发重绘，渲染器按帧代号只上传脏区。 /// 共享 framebuffer，UI 重绘时读快照。 /// 输入通道占位（本步不消费）。 /// 分辨率重设请求通道（动态分辨率）。UI 防抖后发，会话侧只做去重。 /// 运行时统计（Arc 与协议线程共享），连接信息面板只读差分。", "decision": "", "done": ""} -->
| C-110 | `src/rdp_session/mod.rs:475`–476 | `build_connector_config` | accelerated 模式（下一行 false） | 下一行实为 autologon: false（mod.rs:477），pointer_software_rendering: false 在 mod.rs:482。 | accelerated 模式（见下方 pointer_software_rendering: false）→ …… | 按建议改 |  |<!-- entry {"no": "C-110", "cls": "C", "file": "src/rdp_session/mod.rs", "key": "// 开启服务端光标：本地做「远端光标接管」。accelerated 模式（下一行 false）→ // IronRDP 只发 PointerBitmap 事件、不把光标合成进 framebuffer（避免双光标）。", "decision": "", "done": ""} -->
| C-111 | `src/rdp_session/mod.rs:493` | `build_connector_config` | 自动探测（mstsc 默认）：服务端实测 RTT/带宽后调整画质与帧率，弱网不再按局域网画质硬推。 | 代码 connection_type_from_env（mod.rs:98-107）默认返回 ConnectionType::Lan，只有设 NEXSHELL_RDP_AUTODETECT 才报 Autodetect；docs/adr/0014 决策 1 明确默认 LAN。 | 默认报 LAN；NEXSHELL_RDP_AUTODETECT=1 才报自动探测（理由见 docs/adr/0014 决策 1）。 | 按建议改 |  |<!-- entry {"no": "C-111", "cls": "C", "file": "src/rdp_session/mod.rs", "key": "// 自动探测（mstsc 默认）：服务端实测 RTT/带宽后调整画质与帧率，弱网不再按局域网画质硬推。", "decision": "", "done": ""} -->
| C-112 | `src/rdp_session/mod.rs:505`–506 | `needs_rdpsnd` | IronRDP rdpdr/src/lib.rs:29 | 当前锁定 rev 538af77 中该说明在 crates/ironrdp-rdpdr/src/lib.rs:34-36（287f337 为 34），行号 29 已错位。 | 去掉行号：「MS-RDPEFS（IronRDP ironrdp-rdpdr 的 Rdpdr 类型文档）——rdpdr 须与 rdpsnd 同时 advertise，」 | 按建议改 |  |<!-- entry {"no": "C-112", "cls": "C", "file": "src/rdp_session/mod.rs", "key": "/// MS-RDPEFS（IronRDP rdpdr/src/lib.rs:29）——rdpdr 须与 rdpsnd 同时 advertise，", "decision": "", "done": ""} -->
| C-113 | `src/rdp_session/mouse_cursor.rs:246` | `MouseCursorClient.process` | 发送失败（通道满） | event_tx 来自 async_channel::unbounded（mod.rs:351），try_send 只会在接收端关闭时失败，不会「满」。 | 发送失败（接收端已关）回滚去重键，下次同指针可重试。 | 按建议改 |  |<!-- entry {"no": "C-113", "cls": "C", "file": "src/rdp_session/mouse_cursor.rs", "key": "// 发送失败（通道满）回滚去重键，下次同指针可重试。", "decision": "", "done": ""} -->
| C-114 | `src/rdp_session/outputs.rs:109` | `drain_outputs` | 发送失败（通道满） | 同 #0154：event_tx 是 unbounded（mod.rs:351）。 | 发送失败（接收端已关）回滚去重键，下次同指针可重试。 | 按建议改 |  |<!-- entry {"no": "C-114", "cls": "C", "file": "src/rdp_session/outputs.rs", "key": "// 发送失败（通道满）回滚去重键，下次同指针可重试，否则光标永久丢失。", "decision": "", "done": ""} -->
| C-115 | `src/rdp_session/rdpdr.rs:3`–4 | `(顶层)` | 后端用 fork 内建的 NixRdpdrBackend，见 docs/adr/0007。 | docs/adr/0007 全文没有 rdpdr / Nix 的内容；NixRdpdrBackend 在 ironrdp-rdpdr-native（src/rdp_session/rdpdr.rs:77 引用），fork 对它的补丁记在 docs/adr/0011（路径沙箱、LockControl）。 | 改为「见 docs/adr/0011」。 | 按建议改 |  |<!-- entry {"no": "C-115", "cls": "C", "file": "src/rdp_session/rdpdr.rs", "key": "//! \\\\tsclient\\NexShell 网盘。后端用 fork 内建的 NixRdpdrBackend，见 docs/adr/0007。", "decision": "", "done": ""} -->
| C-116 | `src/rdp_session/rdpdr.rs:4`–5 | `(顶层)` | 其他平台仍提供 RDPDR 通道，但不广告当地共享盘。 | rdpdr.rs build_channel（84-108）：非 macOS/Linux 时 build_drive_channel 返回 None，仅在 enable_audio 为真才退回 Noop 通道（rdpdr.rs:97 的 or_else）；只开 drive 而不开 audio 时不注册 rdpdr。 | 其他平台只在开了 audio 时挂 Noop rdpdr 满足 rdpsnd 依赖，不广告共享盘。 | 按建议改 |  |<!-- entry {"no": "C-116", "cls": "C", "file": "src/rdp_session/rdpdr.rs", "key": "//! 其他平台仍提供 RDPDR 通道，但不广告当地共享盘。", "decision": "", "done": ""} -->
| C-117 | `src/rdp_view/geometry.rs:8`–9 | `RdpViewport` | 供第 ④ 步鼠标坐标反算用。 | 鼠标反算已实现（geometry.rs viewport_device_coords，page_element.rs 使用），分步实施阶段编号已无意义。 | `scale` = 逻辑像素 / 远端像素，供鼠标坐标反算用。 | 按建议改 |  |<!-- entry {"no": "C-117", "cls": "C", "file": "src/rdp_view/geometry.rs", "key": "/// 加缩放比。`scale` = 逻辑像素 / 远端像素，供第 ④ 步鼠标坐标反算用。", "decision": "", "done": ""} -->
| C-118 | `src/rdp_view/keymap.rs:1`–6 | `(顶层)` | ⌘C/X/V/A/Z 转 Ctrl（ADR 0016 决策 8） | ADR 0016 决策 8 只写 ⌘C/⌘X/⌘V（docs/adr/0016-rdp-clipboard-windows-app-parity.md:85），A/Z 是后加（提交 b2f69f8，docs/plans/2026-09-29-rdp-clipboard.md 第 5 步），ADR 还写「可以关掉」但代码里没有开关（COMMAND_AS_CTRL 常量，keymap.rs:174）。 | 引用改为「ADR 0016 决策 8 + docs/plans/2026-09-29-rdp-clipboard.md 第 5 步」；ADR 也该同步 A/Z 与「可关」的现状 | 按建议改：只改注释里的引用；ADR 0016 决策 8 与代码不一致（没写 A/Z，写了「可以关掉」而代码没有开关）另报，不在这条里动 |  |<!-- entry {"no": "C-118", "cls": "C", "file": "src/rdp_view/keymap.rs", "key": "// rdp_view::keymap — macOS 按键 → RDP PC set-1 scancode 映射（纯函数，无 &self、无 UI 状态）。 // warpui 的 KeyDown 不带硬件 keycode，只给归一化字符串 key（见 mac/utils.rs unicode_char_to_key） // 与 key_without_modifiers（基础字符）；故普通键走", "decision": "", "done": ""} -->
| C-119 | `src/rdp_view/page_element.rs:27`–42 | `RdpPageElement` | 满/断开则丢弃。 | input_tx 来自 async_channel::unbounded（mod.rs:353），try_send 只会在会话端关闭时失败；见 page_element.rs:72-75 的 try_send。 | 断开则丢弃。 | 按建议改 |  |<!-- entry {"no": "C-119", "cls": "C", "file": "src/rdp_view/page_element.rs", "key": "/// 会话画面；None = 还没收到帧，只画背景。 /// 远端桌面像素尺寸（= framebuffer 宽高），letterbox 的源尺寸。 /// 无帧 / letterbox 黑边填充色。 /// 回写当前 letterbox 几何（绝对坐标），供鼠标→远端坐标反算。 /// 键鼠事件出口（会话线程消费编码成 FastPath）。满/断开则丢弃。 /// 上次发出的鼠标远端坐标（跨帧共", "decision": "", "done": ""} -->
| C-120 | `src/rdp_view/page_element.rs:72`–73 | `RdpPageElement` | 满或断开则丢弃（丢新的） | input_tx 是 unbounded（mod.rs:353），不会满；try_send 失败只意味着会话线程已退出。 | 断开（会话已退出）则丢弃，绝不阻塞 UI 线程。 | 按建议改 |  |<!-- entry {"no": "C-120", "cls": "C", "file": "src/rdp_view/page_element.rs", "key": "/// 发一个输入事件；满或断开则丢弃（丢新的），绝不阻塞 UI 线程。返回 try_send 是否成功。", "decision": "", "done": ""} -->

## 批次 core-1

### B 为捷径辩护

#### B-053 `src/file_panel.rs:173` `(顶层)`

<!-- entry {"no": "B-053", "cls": "B", "file": "src/file_panel.rs", "key": "// 旧名兼容：上传场景沿用历史命名", "decision": "", "done": ""} -->
> // 旧名兼容：上传场景沿用历史命名

- 捷径：为「旧名兼容」保留类型别名 UploadProgress = TransferProgress，而不是把使用点改成新名
- 是否仍然成立：是。证据：file_panel.rs:174 别名仍在；全仓 grep 只有 file_panel.rs 内 2 处构造用它（:1731、:1842），其余是同名枚举变体 SftpEvent::UploadProgress（不受影响）；无其他 crate / bin / examples 引用，没有外部兼容对象
- 建议：修代码
- 风险：低：多一个无人依赖的别名，新读者会疑惑 Upload/Transfer 两套名字；删除只需改 2 处并删这两行
- 决定：修代码
- 拍板备注：别名没有任何外部使用方，是纯粹的旧名兼容，按「没有必要不写兼容代码」删掉；只动 `file_panel.rs` 里两处构造和别名本身。
- 完成：2026-11-22

#### B-054 `src/git_ops.rs:1814`–1815 `tests`

<!-- entry {"no": "B-054", "cls": "B", "file": "src/git_ops.rs", "key": "/// 系统没装 git 时跳过。", "decision": "", "done": ""} -->
> /// 系统没装 git 时跳过。

- 捷径：没装 git 时测试直接 return（静默通过）而不是失败或标 ignore
- 是否仍然成立：是。证据：git_ops.rs:1818-1820 `if Command::new("git").arg("--version").output().is_err() { return; }`；同样写法在 git_ops.rs 另 6 处（:1616 等）和 git_panel.rs 多处（:1223、:1312、:1381 等），这些没有注释说明
- 建议：保留
- 风险：低：无 git 的环境里这批端到端用例会空跑显示通过；产品本身依赖 git、开发机都有，实际风险小
- 备注：doc 注释，两行里这一行是辩护；另一行（端到端说明）不是
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-055 `src/git_panel.rs:1767`–1768 `tests`

<!-- entry {"no": "B-055", "cls": "B", "file": "src/git_panel.rs", "key": "/// 用 ignore 标记是因为 2s+ debounce 会拖慢整套测试套件；CI 单跑 `--ignored` 验。", "decision": "", "done": ""} -->
> /// 用 ignore 标记是因为 2s+ debounce 会拖慢整套测试套件；CI 单跑 `--ignored` 验。

- 捷径：用 #[ignore] 把 watcher 端到端测试排除出默认测试集，理由是「CI 单跑 --ignored 验」
- 豁免：#[ignore]（git_panel.rs:1769，test watcher_triggers_auto_refresh_on_workspace_change）；理由：需要 ~6s 真实 fs watch；跑 `cargo test -- --ignored`（attribute 里有理由；doc 里另说 CI 单跑 --ignored 验）；去掉后还报不报错：未核实：去掉 ignore 测试应通过（只是慢，约 6s），验证命令 `cargo test -p nexshell git_panel::tests::watcher_triggers_auto_refresh_on_workspace_change -- --ignored`（按维护者禁令未编译运行）
- 是否仍然成立：否。证据：仓库里没有 CI 跑它：.forgejo/workflows 只有 windows-release.yml（只做构建发版，无 cargo test 步骤）；verify-on-stop.sh 也不含 --ignored。git_panel.rs:1769 的 #[ignore] 理由文本「跑 cargo test -- --ignored」只是手动命令。结果是该测试实际无人定期跑
- 建议：记待办
- 风险：中：fs watcher 自动刷新（debounce、噪声过滤）回归没有任何自动守护；注释里「CI 单跑」不实
- 备注：同组 doc 另含端到端描述（C）
- 决定：修代码
- 拍板备注：推翻「记待办」。`#[ignore]` 保留，在 `scripts/validate.sh` 的 cargo test 之后加一步单跑这条测试（`-- --ignored` 加测试名），把注释和 ignore 理由里的「CI 单跑」改成 `scripts/validate.sh`。`scripts/validate.sh` 在本批目录之外，已同意连带修改。先单跑确认它现在能过；不过就停下报告，不改断言。
- 完成：2026-11-22

#### B-056 `src/history_suggester.rs:47` `read_history`

<!-- entry {"no": "B-056", "cls": "B", "file": "src/history_suggester.rs", "key": "// zsh 历史可能是 metafied encoding，先按 UTF-8 lossy 读取", "decision": "", "done": ""} -->
> // zsh 历史可能是 metafied encoding，先按 UTF-8 lossy 读取

- 捷径：读 zsh 历史时只按 UTF-8 lossy 解码，不做 zsh 的 unmetafy（0x83 转义还原）
- 是否仍然成立：是。证据：history_suggester.rs:44-48 read_history 对 fs::read 的字节直接 String::from_utf8_lossy，无 0x83 处理。zsh 在 .zsh_history 里会把 0x83..0xA2 范围的字节写成 0x83 + (byte^0x20)，非 ASCII（如中文命令）的 UTF-8 续字节会落入该范围——此点据 zsh 源码 metafy 规则推断，未用真实历史文件复现，需验证：往 ~/.zsh_history 写入含中文的命令后看 suggestion 是否乱码
- 建议：修代码
- 风险：低：仅影响含非 ASCII 字节的 zsh 历史条目，autosuggestion 出现乱码或匹配失败
- 决定：保留
- 拍板备注：推翻「修代码」。`history_suggester` 整个模块没有调用方：`HistorySuggester` 没人构造，`TerminalInputEditor::set_suggestion` 没人调用，历史建议功能没接线。zsh 历史的 metafy 问题是真的（0x83–0xA2 的字节会写成 0x83 加异或 0x20，中文命令读出来是乱码），但修的是死代码，不修也不记待办。这个模块是接线还是删掉是另一个决定，已报给 Matt。
- 完成：2026-11-22（保留，不动）

#### B-057 `src/host_management.rs:989` `migrate_add_rdp_display_quality`

<!-- entry {"no": "B-057", "cls": "B", "file": "src/host_management.rs", "key": "// 给 hosts 补 rdp_display_quality 列（幂等；已存在则 ALTER 失败被忽略）。", "decision": "", "done": ""} -->
> // 给 hosts 补 rdp_display_quality 列（幂等；已存在则 ALTER 失败被忽略）。

- 捷径：用 `let _ = conn.execute("ALTER TABLE ... ADD COLUMN")` 靠失败来表达「列已存在」，吞掉所有错误（锁、只读、磁盘满同样被吞）
- 是否仍然成立：是。证据：host_management.rs:990-995 migrate_add_rdp_display_quality；同文件 migrate_add_sort_order（:1005-1017）也是 .is_ok() 吞错误。失败时不会报告，后续 SELECT/INSERT 才报「no such column」
- 建议：修代码
- 风险：低：仅在迁移因非『列已存在』原因失败时，错误延后到更难定位的地方；可改为先查 PRAGMA table_info(hosts) 再 ALTER，或只忽略 duplicate column name
- 决定：保留
- 拍板备注：推翻「修代码」。要出问题，得是 ALTER 因「列已存在」以外的原因失败（库被另一个进程锁住、磁盘满、只读）；这时紧接着的查询会报 no such column，错误没被吞掉、只是晚一步，下次启动迁移还会重试。属报错质量，不是正确性问题，不动代码。
- 完成：2026-11-22（保留，不动）

#### B-058 `src/host_management.rs:997` `migrate_add_rdp_resolution`

<!-- entry {"no": "B-058", "cls": "B", "file": "src/host_management.rs", "key": "// 给 hosts 补 rdp_resolution 列（幂等）：\"auto\" 或 \"宽x高\"。", "decision": "", "done": ""} -->
> // 给 hosts 补 rdp_resolution 列（幂等）："auto" 或 "宽x高"。

- 捷径：用 `let _ = conn.execute("ALTER TABLE ... ADD COLUMN")` 靠失败来表达「列已存在」，吞掉所有错误（锁、只读、磁盘满同样被吞）
- 是否仍然成立：是。证据：host_management.rs:998-1003 migrate_add_rdp_resolution；同文件 migrate_add_sort_order（:1005-1017）也是 .is_ok() 吞错误。失败时不会报告，后续 SELECT/INSERT 才报「no such column」
- 建议：修代码
- 风险：低：仅在迁移因非『列已存在』原因失败时，错误延后到更难定位的地方；可改为先查 PRAGMA table_info(hosts) 再 ALTER，或只忽略 duplicate column name
- 备注：comment 只写「（幂等）」，但与 0505 同一个吞错误写法，建议一起处理
- 决定：保留
- 拍板备注：推翻「修代码」。同 B-057。
- 完成：2026-11-22（保留，不动）

### C 过期的「为什么」

| 编号 | 位置 | 所在符号 | 过期的句子 | 依据 | 建议改成 | 决定 | 完成 |
|---|---|---|---|---|---|---|---|
| C-121 | `src/container_overview.rs:231`–232 | `ContainerAction` | 本步只造命令 | 执行已落地：root_view/host_library_section/actions.rs:93 用 container_action_command + host_overview::spawn_remote_exec 执行；「本步」是实施步骤叙述 | 执行走 host_overview::spawn_remote_exec，本函数只负责生成命令。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-121", "cls": "C", "file": "src/container_overview.rs", "key": "/// 一次性操作（start/stop/restart）。执行走 host_overview::spawn_remote_exec，本步只造命令。", "decision": "", "done": ""} -->
| C-122 | `src/file_panel.rs:131`–138 | `SftpRequest` | worker 会先 canonicalize 再 read_dir | file_panel.rs:2123 远程 worker 才 canonicalize；本地 worker 的 run_local_list_without_loading（file_panel.rs:1236-1239）明确不 canonicalize，而 List 两边共用 | 远程 worker 会先 canonicalize 再 read_dir；本地 worker 直接列逻辑路径，不 canonicalize。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-122", "cls": "C", "file": "src/file_panel.rs", "key": "/// 列出 path 目录；worker 会先 canonicalize 再 read_dir。 /// 列出树状面板的子目录，不改变 worker 的刷新基准目录。 /// 列出 cwd（worker 复用上次 path）。 /// 上传一组本地文件到远端目录。完成后会触发一次 Refresh。", "decision": "", "done": ""} -->
| C-123 | `src/file_panel_view_helpers.rs:114`–115 | `(顶层) 实际挂在 PANEL_BORDER_W（应属 panel_slide_in_from_right）` | 整段（面板滑动包装的说明）挂在 PANEL_BORDER_W 常量上 | file_panel_view_helpers.rs:114-121 的 /// 夹在 `pub(crate) const PANEL_BORDER_W`（:122）之上，而前 6 行说明的是其后的 panel_slide_in_from_right（:124）；rustdoc 里常量背了函数的文档、函数反而没有 | 把这 6 行移到 panel_slide_in_from_right 之上；PANEL_BORDER_W 只留最后两行（1px 左边框那句）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-123", "cls": "C", "file": "src/file_panel_view_helpers.rs", "key": "/// 面板滑动包装：定宽透明占位子撑住槽位（布局零影响、不抢跑露底板），", "decision": "", "done": ""} -->
| C-124 | `src/file_panel_view_helpers.rs:115`–116 | `(顶层)` | 整段（面板滑动包装的说明）挂在 PANEL_BORDER_W 常量上 | file_panel_view_helpers.rs:114-121 的 /// 夹在 `pub(crate) const PANEL_BORDER_W`（:122）之上，而前 6 行说明的是其后的 panel_slide_in_from_right（:124）；rustdoc 里常量背了函数的文档、函数反而没有 | 把这 6 行移到 panel_slide_in_from_right 之上；PANEL_BORDER_W 只留最后两行（1px 左边框那句）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-124", "cls": "C", "file": "src/file_panel_view_helpers.rs", "key": "/// 面板作为 Stack overlay 按 offset 平移（正=右移滑入/滑出，负=从左侧回位）。", "decision": "", "done": ""} -->
| C-125 | `src/file_panel_view_helpers.rs:116`–117 | `(顶层)` | 整段（面板滑动包装的说明）挂在 PANEL_BORDER_W 常量上 | file_panel_view_helpers.rs:114-121 的 /// 夹在 `pub(crate) const PANEL_BORDER_W`（:122）之上，而前 6 行说明的是其后的 panel_slide_in_from_right（:124）；rustdoc 里常量背了函数的文档、函数反而没有 | 把这 6 行移到 panel_slide_in_from_right 之上；PANEL_BORDER_W 只留最后两行（1px 左边框那句）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-125", "cls": "C", "file": "src/file_panel_view_helpers.rs", "key": "/// 注意 overlay 层全局压顶：force_overlay 用于让静止的右邻也进 overlay", "decision": "", "done": ""} -->
| C-126 | `src/file_panel_view_helpers.rs:117`–118 | `(顶层)` | 整段（面板滑动包装的说明）挂在 PANEL_BORDER_W 常量上 | file_panel_view_helpers.rs:114-121 的 /// 夹在 `pub(crate) const PANEL_BORDER_W`（:122）之上，而前 6 行说明的是其后的 panel_slide_in_from_right（:124）；rustdoc 里常量背了函数的文档、函数反而没有 | 把这 6 行移到 panel_slide_in_from_right 之上；PANEL_BORDER_W 只留最后两行（1px 左边框那句）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-126", "cls": "C", "file": "src/file_panel_view_helpers.rs", "key": "/// （压栈序在后 = 画在上层），左邻动画时从它底下钻过而不是骑到它脸上。", "decision": "", "done": ""} -->
| C-127 | `src/file_panel_view_helpers.rs:118`–119 | `(顶层)` | 整段（面板滑动包装的说明）挂在 PANEL_BORDER_W 常量上 | file_panel_view_helpers.rs:114-121 的 /// 夹在 `pub(crate) const PANEL_BORDER_W`（:122）之上，而前 6 行说明的是其后的 panel_slide_in_from_right（:124）；rustdoc 里常量背了函数的文档、函数反而没有 | 把这 6 行移到 panel_slide_in_from_right 之上；PANEL_BORDER_W 只留最后两行（1px 左边框那句）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-127", "cls": "C", "file": "src/file_panel_view_helpers.rs", "key": "/// constrain_absolute_children 让面板拿到槽位的紧高度约束（内部 Stretch 正常撑满）。", "decision": "", "done": ""} -->
| C-128 | `src/file_panel_view_helpers.rs:119`–120 | `(顶层)` | 整段（面板滑动包装的说明）挂在 PANEL_BORDER_W 常量上 | file_panel_view_helpers.rs:114-121 的 /// 夹在 `pub(crate) const PANEL_BORDER_W`（:122）之上，而前 6 行说明的是其后的 panel_slide_in_from_right（:124）；rustdoc 里常量背了函数的文档、函数反而没有 | 把这 6 行移到 panel_slide_in_from_right 之上；PANEL_BORDER_W 只留最后两行（1px 左边框那句）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-128", "cls": "C", "file": "src/file_panel_view_helpers.rs", "key": "/// offset≈0 且未强制时原样返回。", "decision": "", "done": ""} -->
| C-129 | `src/file_panel_view_helpers.rs:293` | `local_file_panel_context_menu_items` | 内置只读查看器（ADR 0002） | ADR 0003（accepted 2026-05-30，docs/adr/0003-builtin-code-editor.md）反转了 0002：查看器已就地可编辑 | 内置代码查看器 / 编辑器（ADR 0002、0003）：二进制 / 超大由 handler 回退「打开」。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-129", "cls": "C", "file": "src/file_panel_view_helpers.rs", "key": "// 内置只读查看器（ADR 0002）：二进制 / 超大由 handler 回退「打开」。", "decision": "", "done": ""} -->
| C-130 | `src/font_enumeration.rs:1`–2 | `(顶层)` | Warp appearance_page.rs:2010-2025 | ../warp app/src/settings_view/appearance_page.rs:2010-2025 现在是 set_line_height_ratio，枚举系统字体的调用在 :1113 附近（all_system_fonts）；行号已漂移 | 去掉行号，写成「参考 warp appearance_page.rs 的 all_system_fonts 调用」。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-130", "cls": "C", "file": "src/font_enumeration.rs", "key": "//! 系统字体枚举与候选优先级（Warp appearance_page.rs:2010-2025 同等思路）。", "decision": "", "done": ""} -->
| C-131 | `src/font_enumeration.rs:52` | `enumerate_all_system_fonts` | Warp appearance_page.rs:2010-2025 | ../warp app/src/settings_view/appearance_page.rs:2010-2025 现在是 set_line_height_ratio，枚举系统字体的调用在 :1113 附近（all_system_fonts）；行号已漂移 | 去掉行号，写成「参考 warp appearance_page.rs 的 all_system_fonts 调用」。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-131", "cls": "C", "file": "src/font_enumeration.rs", "key": "// warp: appearance_page.rs:2010-2025 — 枚举所有系统字体", "decision": "", "done": ""} -->
| C-132 | `src/foreground_kind.rs:10`–11 | `ForegroundKind` | `Shell` 的判定与改动前的 `query_shell_foreground` 完全一致 | 全仓已无 query_shell_foreground（现为 terminal_runtime.rs:3413 query_foreground_kind），「改动前」是变更叙述 | `Shell` 覆盖常见 shell 进程名（见 classify）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-132", "cls": "C", "file": "src/foreground_kind.rs", "key": "/// PTY 前台进程分类。`Shell` 的判定与改动前的 `query_shell_foreground` 完全一致。", "decision": "", "done": ""} -->
| C-133 | `src/foreground_kind.rs:14`–15 | `ForegroundKind` | 保持改动前「查不到就当 shell 在前台」的行为 | 同上，改动前的函数已不存在 | 保持「查不到就当 shell 在前台」的行为。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-133", "cls": "C", "file": "src/foreground_kind.rs", "key": "/// 保持改动前「查不到就当 shell 在前台」的行为。", "decision": "", "done": ""} -->
| C-134 | `src/foreground_kind.rs:52`–53 | `ForegroundKind` | 与改动前 `query_shell_foreground` 的返回值同义 | 同上 | `Shell` 即 true：当 shell 在前台。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-134", "cls": "C", "file": "src/foreground_kind.rs", "key": "/// 与改动前 `query_shell_foreground` 的返回值同义。", "decision": "", "done": ""} -->
| C-135 | `src/foreground_kind.rs:100`–101 | `tests` | 必须等价于改动前的 `return true`（当 shell 在前台） | 同上 | 必须落到 Shell（即当 shell 在前台）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-135", "cls": "C", "file": "src/foreground_kind.rs", "key": "/// 必须等价于改动前的 `return true`（当 shell 在前台）。", "decision": "", "done": ""} -->
| C-136 | `src/git_ops.rs:2`–3 | `(顶层)` | warp/app/src/util/git.rs:14 `run_git_command` | ../warp 中 run_git_command 已移到 crates/warp_util/src/git.rs:8（带 env 版 :16，参数/环境在 :27-29）；app/src/util/git.rs:14 现在是别的函数 | 参考 warp crates/warp_util/src/git.rs 的 run_git_command：…… | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-136", "cls": "C", "file": "src/git_ops.rs", "key": "//! env / arg 配置参考 warp/app/src/util/git.rs:14 `run_git_command`：", "decision": "", "done": ""} -->
| C-137 | `src/git_ops.rs:16`–17 | `run_git` | 成功 = stdout trim 后的字符串 | git_ops.rs:83-86 成功分支返回 `String::from_utf8_lossy(&amp;output.stdout).into_owned()`，没有 trim | 成功 = stdout 原文（lossy UTF-8，不 trim）；失败 = stderr + stdout 合并的错误。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-137", "cls": "C", "file": "src/git_ops.rs", "key": "/// 通用 git 调用：成功 = stdout trim 后的字符串；失败 = stderr + stdout 合并的错误。", "decision": "", "done": ""} -->
| C-138 | `src/git_ops.rs:17`–18 | `(顶层)` | 调用方按需放宽（本模块仅对 diff 命令做） | 放宽由 run_git_diff（git_ops.rs:30-52）内部完成：退出码 1 且 stdout 非空视为成功；run_git 并不让调用方放宽 | `git diff` 的「退出码 1 = 有差异」由 run_git_diff 单独处理，本函数一律按非 0 退出码报错。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-138", "cls": "C", "file": "src/git_ops.rs", "key": "/// `git diff` 单独有一类\"退出码 1 = 有差异\"，调用方按需放宽（本模块仅对 diff 命令做）。", "decision": "", "done": ""} -->
| C-139 | `src/git_ops.rs:408`–409 | `(顶层)` | warp util/git.rs:116 同款回落策略 | ../warp app/src/util/git.rs 中 detect_current_branch 在 :51，:116 已是 detect_main_branch 内部 | warp app/src/util/git.rs detect_current_branch 同款回落策略（或去掉行号）。 | 按建议改：写符号名，不写行号 | 2026-11-22 |<!-- entry {"no": "C-139", "cls": "C", "file": "src/git_ops.rs", "key": "/// warp util/git.rs:116 同款回落策略。", "decision": "", "done": ""} -->
| C-140 | `src/git_ops.rs:417`–418 | `(顶层)` | warp util/git.rs:133 | ../warp app/src/util/git.rs detect_current_branch_display 现在在 :68 | warp app/src/util/git.rs detect_current_branch_display（或去掉行号）。 | 按建议改：写符号名，不写行号 | 2026-11-22 |<!-- entry {"no": "C-140", "cls": "C", "file": "src/git_ops.rs", "key": "/// warp util/git.rs:133。", "decision": "", "done": ""} -->
| C-141 | `src/git_ops.rs:427`–428 | `(顶层)` | warp util/git.rs:157 | ../warp app/src/util/git.rs detect_main_branch 现在在 :92 | warp app/src/util/git.rs detect_main_branch（或去掉行号）。 | 按建议改：写符号名，不写行号 | 2026-11-22 |<!-- entry {"no": "C-141", "cls": "C", "file": "src/git_ops.rs", "key": "/// 主分支检测：origin/HEAD → 回落候选列表。warp util/git.rs:157。", "decision": "", "done": ""} -->
| C-142 | `src/git_panel.rs:6`–7 | `(顶层)` | warp/app/src/code_review/git_status_update.rs | 文件已不存在：warp 提交 4815c8250「Reorganize git repo models (#12541)」删除了 app/src/code_review/git_status_update.rs，逻辑迁到 app/src/code_review/git_repo_model/local.rs（../warp，integration_mirror=5bf0471） | 数据语义参考 warp/app/src/code_review/git_repo_model/local.rs 的简化版（或去掉路径）。 | 按建议改：写新路径 git_repo_model/local.rs | 2026-11-22 |<!-- entry {"no": "C-142", "cls": "C", "file": "src/git_panel.rs", "key": "//! 数据语义参考 warp/app/src/code_review/git_status_update.rs 的简化版：", "decision": "", "done": ""} -->
| C-143 | `src/git_panel.rs:7`–8 | `(顶层)` | 我们没有 fs watcher（P4 才上） | fs watcher 已实现：git_panel.rs:692 build_repo_watcher，WATCHER_DEBOUNCE=2s（:31）；P4 是 docs/git-panel-plan.md 的阶段号 | 刷新触发点是 cwd 变化 / 手动按钮 / 操作完成 / fs watcher（debounce 2s）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-143", "cls": "C", "file": "src/git_panel.rs", "key": "//! 我们没有 fs watcher（P4 才上），刷新触发点是 cwd 变化 / 手动按钮 / 操作完成。", "decision": "", "done": ""} -->
| C-144 | `src/git_panel.rs:29`–30 | `(顶层)` | Warp git_status_update.rs:204 用 5s throttle | 文件已不存在：warp 提交 4815c8250「Reorganize git repo models (#12541)」删除了 app/src/code_review/git_status_update.rs，逻辑迁到 app/src/code_review/git_repo_model/local.rs（../warp，integration_mirror=5bf0471）；5s throttle 现为 git_repo_model/local.rs:95 | Warp git_repo_model/local.rs 用 5s throttle（或去掉行号）。 | 按建议改：写 git_repo_model/local.rs，不写行号 | 2026-11-22 |<!-- entry {"no": "C-144", "cls": "C", "file": "src/git_panel.rs", "key": "/// fs watcher debounce 时长。Warp git_status_update.rs:204 用 5s throttle，", "decision": "", "done": ""} -->
| C-145 | `src/git_panel.rs:536`–537 | `run_worker` | Warp git_status_update.rs:367 同款 | 文件已不存在：warp 提交 4815c8250「Reorganize git repo models (#12541)」删除了 app/src/code_review/git_status_update.rs，逻辑迁到 app/src/code_review/git_repo_model/local.rs（../warp，integration_mirror=5bf0471）；index.lock 检测现为 git_repo_model/local.rs:324-328 | Warp git_repo_model/local.rs 同款（或去掉路径行号）。 | 按建议改：写 git_repo_model/local.rs，不写行号 | 2026-11-22 |<!-- entry {"no": "C-145", "cls": "C", "file": "src/git_panel.rs", "key": "// index.lock 抑制：用户正在跑 commit / merge 等， // 读到的 HEAD / index 是中间态。Warp git_status_update.rs:367 同款。", "decision": "", "done": ""} -->
| C-146 | `src/git_panel_row_helpers.rs:37`–38 | `GitPanelSectionKind` | 与旧实现一致 | git_panel_row_helpers 是虚拟化重写后的实现，旧实现已无代码可对照（无引用） | 删除这句；固定顺序已由前半句说清。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-146", "cls": "C", "file": "src/git_panel_row_helpers.rs", "key": "/// 四个分组固定顺序：staged → unstaged → untracked → unmerged，与旧实现一致。", "decision": "", "done": ""} -->
| C-147 | `src/host_monitor_view_helpers.rs:12`–13 | `OVERVIEW_CONTENT_WIDTH` | 侧栏固定 248px | ADR 0023 后主机概览在右侧面板：sidebar_helpers.rs:7 OVERVIEW_PANEL_WIDTH=248（内容宽，左边框另计 1px），左侧栏是主机列表 | 右侧概览面板内容宽 248px（OVERVIEW_PANEL_WIDTH），减左右各 10px 内边距后的内容宽度。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-147", "cls": "C", "file": "src/host_monitor_view_helpers.rs", "key": "/// 侧栏固定 248px，左右各 10px 内边距后的内容宽度。", "decision": "", "done": ""} -->

## 批次 core-2

### B 为捷径辩护

#### B-059 `src/lib.rs:2727`–2728 `tests`

<!-- entry {"no": "B-059", "cls": "B", "file": "src/lib.rs", "key": "// Host-management storage is intentionally disabled in the default // WarpUI spike build while rusqlite conflicts with Warp's sqlite stack.", "decision": "", "done": ""} -->
> // Host-management storage is intentionally disabled in the default
> // WarpUI spike build while rusqlite conflicts with Warp's sqlite stack.

- 捷径：整个测试（及同文件另外 10 处 #[cfg(any())] 测试）用永假条件编译关掉，不编译不运行，理由是 rusqlite 与 Warp 的 sqlite 栈冲突
- 豁免：#[cfg(any())]（恒假条件编译，等价于删掉该 #[test]）；理由：Host-management storage is intentionally disabled in the default WarpUI spike build while rusqlite conflicts with Warp's sqlite stack.；去掉后还报不报错：未核实：去掉 cfg 后需运行 cargo test --lib host_management_snapshot_matches_legacy_page_structure（及另外 10 个同类）看是否编译、是否通过；按 lib.rs:2740 的占位符断言预计会失败，需要更新断言或删除测试。
- 是否仍然成立：否。证据：Cargo.toml:22 rusqlite 0.35 是无条件依赖，host_management.rs / ssh_key_store.rs / rdp_cert_store.rs 都在用，主程序已在编译；Cargo.lock 只有一份 libsqlite3-sys 0.33.0（Cargo.lock:5632），不存在链接冲突。被关掉的测试已漂移：lib.rs:2740 断言 search_placeholder == "搜索主机..."，host_management.rs:960 / 1115 / 1573 实为 "搜索主机或分组..."，说明它们早就没人跑。注释只写在第一处，lib.rs 里 grep "cfg(any())" 共 11 处（lib.rs:2729,2750,2762,2837,2874,2902 等）。
- 建议：修代码
- 风险：中：一批描述主机管理页结构的测试长期不运行，断言已与实现脱节，回归无人把关，注释还给出已不成立的理由
- 备注：设计上是为「不跑的测试」辩护，故归 B
- 决定：修代码
- 拍板备注：留在 core-2 批次修，并排在 B-064 之前——这批测试是主机库改动的回归网（见拍板记录）。注释给的理由已不成立：rusqlite 是无条件依赖，Cargo.lock 里只有一份 libsqlite3-sys。逐个去掉 `#[cfg(any())]`：测的行为还在的（建库、增删改、筛选、选择）让它跑起来，文案类漂移（如搜索框占位符）可以改断言；测的行为已经不存在的（SSH 走 DirectPty、`connect_command_for` 拼 ssh 命令、legacy 演示数据的固定内容）删掉测试；行为对不上又说不清哪边错的，停下报告，不改断言。只为这些测试活着的生产代码（`legacy_host_management_snapshot`、`connect_command_for` 一线）不在这条里删，列进汇报。
- 补充拍板（Matt 2026-11-22）：剩下的 `host_management_snapshot_can_load_real_tauri_host_database` 删掉。它的种子库是没有 `auth_method` 列的极简旧结构，Tauri 那一代已废弃，NexShell 自建的库一直有这一列；「旧 Tauri 结构迁移后可读写」已由 `persists_draft` 覆盖。否决：为这种实际不存在的旧库加补 `auth_method` 的迁移。
- 完成：2026-11-22（恢复 7 个、删 4 个）

#### B-060 `src/menu.rs:343` `MenuItemLabel.render`

<!-- entry {"no": "B-060", "cls": "B", "file": "src/menu.rs", "key": "// for the future use cases", "decision": "", "done": ""} -->
> // for the future use cases

- 捷径：保留一个函数体内完全没用到的参数 _vertical_padding，用下划线前缀压掉 unused 警告，理由「将来可能用」
- 是否仍然成立：否。证据：menu.rs:343 参数声明；MenuItemLabel::render（menu.rs:336 起至 menu.rs:470 前后）内无任何引用，grep vertical_padding 在该段只命中声明处。调用点 menu.rs:1333 仍在传值。上游 warp app/src/menu.rs 写法与注释相同，是原样搬入。
- 建议：修代码
- 风险：低：只是多一个死参数；动它会与上游 menu.rs 产生分歧，追平上游时多一处冲突，维护者可选择保留
- 决定：保留
- 拍板备注：推翻「修代码」。这个参数和这句注释是 Warp 上游原文（warp `app/src/menu.rs` 同样有），保持一致、不动（见拍板记录）。
- 完成：2026-11-22（保留，不动）

#### B-061 `src/osc7.rs:20` `system_hostname`

<!-- entry {"no": "B-061", "cls": "B", "file": "src/osc7.rs", "key": "// SAFETY: buf 长度传给 libc，返回后按 NUL 截断", "decision": "", "done": ""} -->
> // SAFETY: buf 长度传给 libc，返回后按 NUL 截断

- 捷径：unsafe 调用 libc::gethostname 的 SAFETY 论证
- 是否仍然成立：是。证据：osc7.rs:19 buf 为 [0u8;256] 零初始化；osc7.rs:21 传入 len-1，末字节恒为 0，即使 gethostname 截断不补 NUL 也能终结；返回后按首个 NUL 截断（osc7.rs:25），不越界。理由成立，但没点出「零初始化 + 少传 1 字节」这个关键点。
- 建议：保留
- 风险：低：理由成立，仅表述偏简
- 备注：SAFETY 注释永不删；可选补一句「buf 零初始化且少传 1 字节，保证 NUL 终结」
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-062 `src/remote_edit_io.rs:165` `remote_save`

<!-- entry {"no": "B-062", "cls": "B", "file": "src/remote_edit_io.rs", "key": "// stat 失败（文件被删等）不阻断保存：按新建写回。", "decision": "", "done": ""} -->
> // stat 失败（文件被删等）不阻断保存：按新建写回。

- 捷径：保存前的冲突检测在 stat 失败时直接跳过，按「文件被删」当新建写回
- 是否仍然成立：未核实。证据：remote_edit_io.rs:164-171：stat_meta 失败返回 None 即跳过冲突比对继续写；stat_meta（remote_edit_io.rs:183-188）用 .ok() 吞掉所有错误，无法区分「文件不存在」与权限/网络瞬时错误。「文件被删」场景成立；瞬时失败场景下会绕过冲突检测，但此时后面的 write_file_from_memory 通常也会失败，实际风险小。需要查：sftp_ops::stat_file 在目标不存在时返回的具体错误类型（russh-sftp Status NoSuchFile），确认能否只对该类错误放行。
- 建议：保留
- 风险：低：极少数瞬时 stat 错误 + 写入成功时，可能覆盖他人刚改的内容
- 决定：保留
- 完成：2026-11-22（保留，不动）

#### B-063 `src/sftp_ops.rs:237`–238 `rename_overwrite`

<!-- entry {"no": "B-063", "cls": "B", "file": "src/sftp_ops.rs", "key": "/// 把临时文件原子改名成目标。russh-sftp 没有 posix-rename 扩展，", "decision": "", "done": ""} -->
> /// 把临时文件原子改名成目标。russh-sftp 没有 posix-rename 扩展，

- 捷径：SFTP 覆盖写回退为「先删目标再 rename」，不再是原子替换，并与「原文件保持完整」的承诺冲突
- 是否仍然成立：是。证据：russh-sftp 锁定 2.1.1（Cargo.lock:8799），其 src/extensions.rs 只有 limits/hardlink/fsync/statvfs，没有 posix-rename@openssh.com 封装（RawSftpSession::extended 可手发，未用）。sftp_ops.rs:241-258 rename_overwrite：先 rename，失败则 remove_file(目标) 再 rename。SFTP v3 规范要求目标已存在时 rename 报错（未对具体服务端实测，OpenSSH 预期走此回退，需要在真机上确认），因此多数覆盖都会进入「删后重命名」窗口。若第二次 rename 也失败，调用方 sftp_ops.rs:502（put_file_stream）/ :684（write_file_from_memory）还会再删临时文件，结果原文件和新内容都没了。
- 建议：修代码
- 风险：中：覆盖上传/远程编辑保存时，在 remove 与 rename 之间断线或第二次 rename 失败，会丢失原文件（临时文件也被清掉），与注释宣称的原子语义相反
- 备注：同时影响 core-2#0416 / 0439 / 0448 的「原子 / 不破坏原文件」说法
- 决定：修代码
- 拍板备注：确认是 bug，走单独的 bug 修复、先做，不在 core-2 批次里做（见拍板记录）。要求：覆盖过程任何一步失败，都不能让原文件丢掉。修法由实现者定（可选：`posix-rename@openssh.com` 扩展；服务端不支持时先把原文件改名让位、换入成功后再删）。修完在真机验证：OpenSSH，外加一台不支持该扩展的设备。本条注释和 C-166 / C-167 / C-168 按修复后的实际行为一起改，完成栏随修复提交填。
- 完成：2026-11-21（随 SFTP 覆盖修复提交；已对本机 OpenSSH sftp-server 测试，真机待 Matt 验证）

### C 过期的「为什么」

| 编号 | 位置 | 所在符号 | 过期的句子 | 依据 | 建议改成 | 决定 | 完成 |
|---|---|---|---|---|---|---|---|
| C-148 | `src/layout.rs:1`–4 | `(顶层)` | main.rs `title_bar` 用 `.h(px(36.0))`；这个常量喂给 ShellLayout 决定 terminal_host.y，鼠标坐标解算靠它 | main.rs 里已没有 px(36.0)；标题栏高度是 main.rs:110 TITLE_BAR_HEIGHT=34.0 + TITLE_BAR_BORDER_HEIGHT=1.0（root_view/tab_bar_section/title_bar.rs:78,534 用 with_height(TITLE_BAR_HEIGHT)）。layout.rs 的 TITLE_BAR_HEIGHT=36 只被 ShellLayout（native_shell_host.rs:61、lib.rs:1152,1173 测试）使用，与真实渲染高度不再对应。 | 删除「实际渲染高度（main.rs title_bar 用 px(36.0)）」说法，改成「旧 native shell 布局用的标题栏高度，与 main.rs TITLE_BAR_HEIGHT 无关」，或让两处共用一个常量；如 ShellLayout 已无生产用途，整文件可记待办清理 | 按建议改，取这一种：「旧 native shell 布局（ShellLayout）用的标题栏高度，与 main.rs 的 TITLE_BAR_HEIGHT 无关」；不合并常量，不记待办 | 2026-11-22 |<!-- entry {"no": "C-148", "cls": "C", "file": "src/layout.rs", "key": "// 标题栏的实际渲染高度（main.rs `title_bar` 用 `.h(px(36.0))`）。 // 这个常量喂给 `ShellLayout`，进而决定 `terminal_host.y`， // 鼠标坐标解算靠它把窗口坐标换算成 cell 行号。两边对不上时， // 选择跨行时会出现“稍稍上移就跳到上一行”的偏移。", "decision": "", "done": ""} -->
| C-149 | `src/lib.rs:4589` | `tests.terminal_render_runs_emit_one_run_per_cell_with_wide_chars_taking_two_cols` | Total visible text spans 4 cells. | lib.rs:4591 紧随其后断言 total == 20：对 20 列的行把所有 run（含空白单元格 run）的 cols 求和，是整行宽度，不是 4。 | 「整行 20 列：4 列文字（你好）+ 16 个单列空 run」 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-149", "cls": "C", "file": "src/lib.rs", "key": "// Total visible text spans 4 cells.", "decision": "", "done": ""} -->
| C-150 | `src/lib.rs:4649`–4650 | `tests.terminal_runtime_marked_text_round_trips_through_snapshot_and_clears` | matches Warp's `replace_and_mark_text_in_range` behaviour | 在 ../warp（HEAD 5bf047181）全文检索 replace_and_mark_text_in_range 无命中；marked text 现在对应 app/src/terminal/model/terminal_model.rs:2548 set_marked_text 与 crates/warpui host_view.m setMarkedText:。src/terminal_runtime.rs:176 也引用同一旧名。 | 改指现存符号（TerminalModel::set_marked_text），或删除「matches Warp 的 …」半句，只保留「空串视为清除」 | 按建议改：取后一种，删掉「matches Warp's ……」半句，只留「空串视为清除」 | 2026-11-22 |<!-- entry {"no": "C-150", "cls": "C", "file": "src/lib.rs", "key": "// Empty marked text is treated as \"clear\" — matches Warp's // `replace_and_mark_text_in_range` behaviour for empty `new_text`.", "decision": "", "done": ""} -->
| C-151 | `src/main.rs:109` | `(顶层)` | warp tab.rs:529 TAB_BAR_HEIGHT = 34 | TAB_BAR_HEIGHT=34. 现在在 warp app/src/workspace/view.rs:568，tab.rs:529 已是无关代码。warp 源文件行号已漂移（../warp HEAD 5bf047181 比引用时多出 454 个上游提交）；数值本身仍成立 | warp workspace/view.rs TAB_BAR_HEIGHT = 34（去掉行号，只留符号名） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-151", "cls": "C", "file": "src/main.rs", "key": "// warp tab.rs:529 TAB_BAR_HEIGHT = 34。", "decision": "", "done": ""} -->
| C-152 | `src/main.rs:116` | `(顶层)` | warp buttons.rs:73-74 ICON_DIMENSIONS=24 | ICON_DIMENSIONS=24 现在在 warp crates/warp_core/src/ui/icons.rs:6，buttons.rs:73-74 已无此符号。warp 源文件行号已漂移（../warp HEAD 5bf047181 比引用时多出 454 个上游提交）；数值本身仍成立 | warp_core ui/icons.rs ICON_DIMENSIONS=24, padding=4 → icon 区 16x16（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-152", "cls": "C", "file": "src/main.rs", "key": "// warp buttons.rs:73-74 ICON_DIMENSIONS=24, padding=4 → icon 区 16x16。", "decision": "", "done": ""} -->
| C-153 | `src/main.rs:119` | `(顶层)` | warp view.rs:17146 最后一个 left toolbar button margin_right(8.) | workspace/view.rs:17146 已是无关代码（该文件现 2 万行以上，tab_bar.add_child(... with_margin_right(8.)) 类似位置在 view.rs:20936）。warp 源文件行号已漂移（../warp HEAD 5bf047181 比引用时多出 454 个上游提交）；数值本身仍成立 | warp workspace/view.rs 最后一个 left toolbar button 的 margin_right(8.)（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-153", "cls": "C", "file": "src/main.rs", "key": "// warp view.rs:17146 最后一个 left toolbar button margin_right(8.)。", "decision": "", "done": ""} -->
| C-154 | `src/main.rs:121` | `(顶层)` | warp view.rs:17619-17622: BUTTON_HEIGHT=24, SIDE_MENU_WIDTH=16, BUTTON_WIDTH=24+16, BUTTON_LEFT_MARGIN=4 | 这几个常量现在在 warp workspace/view.rs:21464-21466（SIDE_MENU_WIDTH=16、BUTTON_WIDTH=24+SIDE_MENU_WIDTH、BUTTON_LEFT_MARGIN=4）。warp 源文件行号已漂移（../warp HEAD 5bf047181 比引用时多出 454 个上游提交）；数值本身仍成立 | warp workspace/view.rs 新建标签按钮区的 SIDE_MENU_WIDTH=16 / BUTTON_WIDTH=24+16 / BUTTON_LEFT_MARGIN=4（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-154", "cls": "C", "file": "src/main.rs", "key": "// warp view.rs:17619-17622: BUTTON_HEIGHT=24, SIDE_MENU_WIDTH=16, BUTTON_WIDTH=24+16, BUTTON_LEFT_MARGIN=4。", "decision": "", "done": ""} -->
| C-155 | `src/main.rs:126` | `(顶层)` | Warp tab.rs:1280 horizontal_padding(8.) | warp app/src/tab.rs:1280 现为 doc 注释行（Marks this tab as a member of a horizontal tab group），不是 padding 代码。warp 源文件行号已漂移（../warp HEAD 5bf047181 比引用时多出 454 个上游提交）；数值本身仍成立 | Warp tab.rs 标签内容 horizontal_padding(8.)（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-155", "cls": "C", "file": "src/main.rs", "key": "// Warp tab.rs:1280 horizontal_padding(8.)。", "decision": "", "done": ""} -->
| C-156 | `src/main.rs:128` | `(顶层)` | Warp tab.rs:1419 vertical_padding(2.) | warp app/src/tab.rs:1419 现为无关代码；with_vertical_padding(2.) 现在在 tab.rs:2097。warp 源文件行号已漂移（../warp HEAD 5bf047181 比引用时多出 454 个上游提交）；数值本身仍成立 | Warp tab.rs 标签 vertical_padding(2.)（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-156", "cls": "C", "file": "src/main.rs", "key": "// Warp tab.rs:1419 vertical_padding(2.)。", "decision": "", "done": ""} -->
| C-157 | `src/main.rs:130` | `(顶层)` | Warp tab.rs:67 / 75 | TAB_CLOSE_BUTTON_WIDTH=20.0 / TAB_CLOSE_BUTTON_HORIZONTAL_INSET=2.0 现在在 warp app/src/tab.rs:262 / 273。warp 源文件行号已漂移（../warp HEAD 5bf047181 比引用时多出 454 个上游提交）；数值本身仍成立 | Warp tab.rs TAB_CLOSE_BUTTON_WIDTH / TAB_CLOSE_BUTTON_HORIZONTAL_INSET（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-157", "cls": "C", "file": "src/main.rs", "key": "// Warp tab.rs:67 / 75。", "decision": "", "done": ""} -->
| C-158 | `src/main.rs:133` | `(顶层)` | warp_core/src/ui/appearance.rs:12 DEFAULT_UI_FONT_SIZE = 12.0 | 现在在 warp crates/warp_core/src/ui/appearance.rs:11。warp 源文件行号已漂移（../warp HEAD 5bf047181 比引用时多出 454 个上游提交）；数值本身仍成立 | warp_core ui/appearance.rs DEFAULT_UI_FONT_SIZE = 12.0（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-158", "cls": "C", "file": "src/main.rs", "key": "// warp_core/src/ui/appearance.rs:12 DEFAULT_UI_FONT_SIZE = 12.0。", "decision": "", "done": ""} -->
| C-159 | `src/main.rs:458`–459 | `(顶层)` | 连接状态三态（连接中 / 已连接渲染帧 / 已断开） | main.rs:459-473 RdpConnectionPhase 有 Connecting / Connected / Reconnecting / Disconnected / CertificateChanged 五个变体，其中两个在下面各自有 doc。 | RDP 整页 tab 的连接阶段（连接中 / 已连接 / 自动重连中 / 已断开 / 证书待确认） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-159", "cls": "C", "file": "src/main.rs", "key": "/// RDP 整页 tab 的连接状态三态（连接中 / 已连接渲染帧 / 已断开）。", "decision": "", "done": ""} -->
| C-160 | `src/main.rs:494`–497 | `RdpTabState` | 输入通道占位 | input_tx 已被使用：root_view/rdp_section.rs:409,422,427,436 try_send 发送键鼠事件，rdp_view/page_element.rs:36 持有。（rdp_session/mod.rs:318 的「本步不消费」同样过期，不在本批。） | 协议层句柄（framebuffer / 帧事件 / 输入通道）。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-160", "cls": "C", "file": "src/main.rs", "key": "/// 协议层句柄（framebuffer / 帧事件 / 输入通道占位）。 /// 重连用连接参数（分辨率连接时定一次，重连沿用同一分辨率）。", "decision": "", "done": ""} -->
| C-161 | `src/main.rs:500`–509 | `RdpTabState` | 第 ④ 步鼠标→远端坐标反算用 | viewport 已用于鼠标坐标反算：root_view/rdp_section.rs:167,312,476 读写 rdp.viewport。「第 ④ 步」指早期实施计划步骤，已完成。 | 当前 letterbox 几何（Element 每帧回写），鼠标→远端坐标反算用。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-161", "cls": "C", "file": "src/main.rs", "key": "/// 画面像素源（包住会话 framebuffer）。Arc 身份要稳定，渲染器才能保留常驻纹理；换会话时重建。 /// 本会话收到过帧；没有就不画画面（重连中只显示文字）。 /// 远端桌面尺寸，按 Resized 事件跟踪，UI 线程不锁 framebuffer。 /// 当前 letterbox 几何（Element 每帧回写），第 ④ 步鼠标→远端坐标反算用。 /// 上次发往远端的鼠标", "decision": "", "done": ""} -->
| C-162 | `src/main.rs:909`–910 | `register_warp_text_input_stack` | warp 在 lib.rs:1707 独立调用 find::view::init | warp app/src/lib.rs:2079 现为 code::editor::find::view::init(ctx)（行号与路径前缀都变了）；本仓调用在 main.rs:911 nexshell::code_editor::find::view::init。其余原因（解耦时漏调导致 cmd-g 失效）仍成立。 | warp 在 app/src/lib.rs 的初始化里独立调用 code::editor::find::view::init（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-162", "cls": "C", "file": "src/main.rs", "key": "// 查找栏键盘导航（cmd-g / cmd-shift-G 跳下一处/上一处）：warp 在 lib.rs:1707 独立调用 // find::view::init（与 init_code_editor_view 是两个 init），解耦时漏调，致快捷键失效。", "decision": "", "done": ""} -->
| C-163 | `src/main.rs:1143` | `main` | 点 X → 只关窗口，有进程则弹确认 | main.rs:1145-1150：macOS 主窗口的 X 只隐藏 App、保留会话并直接返回 Cancel（ADR 0022），不会走后面的弹窗；该描述现在只适用于非 macOS 以及 macOS 的辅助窗口。 | 非 macOS（及 macOS 非主窗口）点 X → 只关窗口，有进程则弹确认；macOS 主窗口见下 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-163", "cls": "C", "file": "src/main.rs", "key": "// 点 X → 只关窗口，有进程则弹确认（与 Warp/iTerm 一致）", "decision": "", "done": ""} -->
| C-164 | `src/main.rs:1280`–1281 | `main` | 对齐 warp（lib.rs:1037） | warp app/src/lib.rs:1275 现为 app_builder.set_app_user_model_id(ChannelState::app_id())，lib.rs:1037 已无关。warp 源文件行号已漂移（../warp HEAD 5bf047181 比引用时多出 454 个上游提交）；数值本身仍成立 | 对齐 warp app/src/lib.rs 的 set_app_user_model_id（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-164", "cls": "C", "file": "src/main.rs", "key": "// 对齐 warp（lib.rs:1037）：设 AppUserModelID（用 nexshell 自己的 bundle id）， // Windows 任务栏据此正确分组窗口 / 取应用图标 / 归集跳转列表与通知。", "decision": "", "done": ""} -->
| C-165 | `src/sftp_ops.rs:80`–81 | `(顶层) const CHUNK_SIZE` | SFTP 协议单包最大 32768 字节，再大会被服务端拒 | SFTP 草案（draft-ietf-secsh-filexfer-02 §3，凭记忆引用，未重新取证）只要求服务端至少支持约 34000 字节的包，32 KiB 是互通的安全下限，不是协议上限；OpenSSH 等实现接受更大的读写块，russh-sftp 2.1.1 另有 limits@openssh.com 查询（extensions.rs:3）。32 KiB 作为保守块大小本身没问题。 | 分块大小：32 KiB。这是各 SFTP 服务端都应支持的保守包长，更大块在个别服务端上可能被拒。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-165", "cls": "C", "file": "src/sftp_ops.rs", "key": "/// 分块大小：32 KiB。SFTP 协议单包最大 32768 字节，再大会被服务端拒。", "decision": "", "done": ""} -->
| C-166 | `src/sftp_ops.rs:198`–199 | `(顶层)` | 是真正的原子替换 | 远端路径下 rename_overwrite（sftp_ops.rs:241-258）在 rename 因目标存在失败时回退为「删除目标再 rename」，不是原子替换；只有本地路径（fs::rename）才是。见 core-2#0421。 | 必须同目录，rename 才在同一文件系统内（远端受 SFTP v3 rename 不覆盖所限，会回退为删后重命名，见 rename_overwrite）。 | 不改：等 B-063 单独修复后，按修复后的实际行为改（建议里「会回退为删后重命名」到时已不成立） | 2026-11-21（随 B-063 修复按实际行为改写） |<!-- entry {"no": "C-166", "cls": "C", "file": "src/sftp_ops.rs", "key": "/// 必须同目录，rename 才在同一文件系统内，是真正的原子替换。", "decision": "", "done": ""} -->
| C-167 | `src/sftp_ops.rs:452` | `put_file_stream` | 完整写完再 rename 覆盖：中断不会破坏原文件 | 写入阶段中断确实不破坏原文件；但 rename_overwrite 回退路径（sftp_ops.rs:250-256）会先删原文件，此后失败则原文件丢失（并且 sftp_ops.rs:502 会再删临时文件）。见 core-2#0421。 | 先写同目录临时文件，写完再 rename 覆盖：写入阶段中断不会破坏原文件（rename 回退为删后重命名时除外）。 | 不改：等 B-063 单独修复后，按修复后的实际行为改 | 2026-11-21（随 B-063 修复按实际行为改写） |<!-- entry {"no": "C-167", "cls": "C", "file": "src/sftp_ops.rs", "key": "// 先写同目录临时文件，完整写完再 rename 覆盖：中断不会破坏原文件", "decision": "", "done": ""} -->
| C-168 | `src/sftp_ops.rs:646`–647 | `(顶层)` | 中途失败只清理临时文件，原文件保持完整 | 同 core-2#0439：write_file_from_memory 也走 rename_overwrite，回退路径下原文件可能已被删。 | 同目录临时文件写完再 rename 覆盖；写入阶段失败只清理临时文件，原文件保持完整（rename 回退为删后重命名时除外）。 | 不改：等 B-063 单独修复后，按修复后的实际行为改 | 2026-11-21（随 B-063 修复按实际行为改写） |<!-- entry {"no": "C-168", "cls": "C", "file": "src/sftp_ops.rs", "key": "/// 同目录临时文件写完再 rename 覆盖；中途失败只清理临时文件，原文件保持完整。", "decision": "", "done": ""} -->
| C-169 | `src/ssh_key_store.rs:238` | `derive_public_key` | 后续 copy-to-server 用 | copy-to-server 已实现：host_management_view/key_manager_view.rs:457,545 render_copy_to_server，其公钥来自 root_view/host_library_section/actions.rs:426 对 derive_public_key 的缓存调用。 | 推导 openssh 公钥（详情页展示 / 复制到服务器用）；加密无口令则 None。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-169", "cls": "C", "file": "src/ssh_key_store.rs", "key": "// 推导 openssh 公钥（详情页展示 / 后续 copy-to-server 用）；加密无口令则 None。", "decision": "", "done": ""} -->

### D 待办

| 编号 | 位置 | 所在符号 | 原文 | 待办内容 | 现状与依据 | 建议 | 决定 | 完成 |
|---|---|---|---|---|---|---|---|---|
| D-007 | `src/host_overview_fleet.rs:45` | `HostOverviewFleet` | // TODO: 主机数很大时这里会一次性起 N 个连接线程，后续可加并发闸门（信号量分批）。 | 主机数很大时 HostOverviewFleet::start 一次性为每台主机起连接线程，后续加并发闸门（信号量分批） | 未做：host_overview_fleet.rs / host_overview.rs 里无 semaphore/permit/并发限制；调用方 root_view/host_library_section/actions.rs:341 把筛选后的全部 SSH 主机一次传入 start。现状靠 main.rs:1070 raise_open_file_limit 把 fd 软上限提到 10240 缓解 EMFILE，并未限流。 | 补引用 | 补引用 | 2026-11-22（T-002） |<!-- entry {"no": "D-007", "cls": "D", "file": "src/host_overview_fleet.rs", "key": "// TODO: 主机数很大时这里会一次性起 N 个连接线程，后续可加并发闸门（信号量分批）。", "decision": "", "done": ""} -->
| D-008 | `src/menu.rs:158`–159 | `Menu` | /// TODO: In the future, if we keep bumping into more rendering changes here, it may make sense to<br> | 菜单渲染变更多时，把 Menu 抽成 trait，下拉与右键菜单分别实现 | 上游 warp（/Users/matt/SynologyDrive/code/vpstools/warp，HEAD 5bf047181）app/src/menu.rs:80 有同一句，本仓 menu.rs 是 refactor b105e7e 本地化时原样搬入，没做。 属上游设计待办，本仓不会做；若要保持与 warp 逐行一致以便追平上游则保留，由维护者定。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-22（保留，不动） |<!-- entry {"no": "D-008", "cls": "D", "file": "src/menu.rs", "key": "/// TODO: In the future, if we keep bumping into more rendering changes here, it may make sense to", "decision": "", "done": ""} -->
| D-009 | `src/menu.rs:159`–160 | `Menu` | /// abstract the Menu functionality into a trait and have separte implementations for dropdown and<br> | 0163 TODO 的续行（abstract the Menu functionality into a trait…） | 同 core-2#0163，属同一条 TODO 的第二行。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-22（保留，不动） |<!-- entry {"no": "D-009", "cls": "D", "file": "src/menu.rs", "key": "/// abstract the Menu functionality into a trait and have separte implementations for dropdown and", "decision": "", "done": ""} -->
| D-010 | `src/menu.rs:160`–161 | `(顶层)` | /// context menu.<br> | 0163 TODO 的末行（context menu.） | 同 core-2#0163，属同一条 TODO 的末行。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-22（保留，不动） |<!-- entry {"no": "D-010", "cls": "D", "file": "src/menu.rs", "key": "/// context menu.", "decision": "", "done": ""} -->
| D-011 | `src/menu.rs:376` | `MenuItemLabel.render` | // TODO figure out the sizes | menu.rs MenuItemLabel::render 里某处尺寸待确定（figure out the sizes） | 上游 warp（/Users/matt/SynologyDrive/code/vpstools/warp，HEAD 5bf047181）app/src/menu.rs:297 有同一句，本仓 menu.rs 是 refactor b105e7e 本地化时原样搬入，没做。 上下文是 for the future use cases 的预留分支。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-22（保留，不动） |<!-- entry {"no": "D-011", "cls": "D", "file": "src/menu.rs", "key": "// TODO figure out the sizes", "decision": "", "done": ""} -->
| D-012 | `src/menu.rs:1601`–1603 | `MenuItem.selectable` | // For items row, right now, we assume that it's either entire row that's selectable or<br>// not (so for simplicity, either all items are selectable or none of them).<br>// TODO be smarter about it | ItemsRow 的 selectable 目前按「整行要么全可选要么全不可选」处理，待做得更细 | 上游 warp（/Users/matt/SynologyDrive/code/vpstools/warp，HEAD 5bf047181）app/src/menu.rs:1533 有同一句，本仓 menu.rs 是 refactor b105e7e 本地化时原样搬入，没做。 本仓行为同上游：MenuItem::selectable 对 ItemsRow 用 any(...)（menu.rs:1603 附近）。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-22（保留，不动） |<!-- entry {"no": "D-012", "cls": "D", "file": "src/menu.rs", "key": "// For items row, right now, we assume that it's either entire row that's selectable or // not (so for simplicity, either all items are selectable or none of them). // TODO be smarter about it", "decision": "", "done": ""} -->
| D-013 | `src/menu.rs:1853`–1854 | `Event` | // via_select_item is true when you close a menu by selecting an item (by click or enter)<br>// TODO: improve this logic since this is not the ideal solution | Event 里 via_select_item 的关闭菜单判定逻辑不理想，待改进 | 上游 warp（/Users/matt/SynologyDrive/code/vpstools/warp，HEAD 5bf047181）app/src/menu.rs:1784 有同一句，本仓 menu.rs 是 refactor b105e7e 本地化时原样搬入，没做。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-22（保留，不动） |<!-- entry {"no": "D-013", "cls": "D", "file": "src/menu.rs", "key": "// via_select_item is true when you close a menu by selecting an item (by click or enter) // TODO: improve this logic since this is not the ideal solution", "decision": "", "done": ""} -->
| D-014 | `src/menu.rs:1949`–1951 | `SubMenu.select_internal` | // Currently we're assuming that for a selectable item row all items in a row are<br>// also selectable, and so - we just need to reduce the index of fields.<br>// TODO be smarter about it | select_internal 对 ItemsRow 假定整行可选，只递减 fields 下标，待做得更细 | 上游 warp（/Users/matt/SynologyDrive/code/vpstools/warp，HEAD 5bf047181）app/src/menu.rs:1881 有同一句，本仓 menu.rs 是 refactor b105e7e 本地化时原样搬入，没做。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-22（保留，不动） |<!-- entry {"no": "D-014", "cls": "D", "file": "src/menu.rs", "key": "// Currently we're assuming that for a selectable item row all items in a row are // also selectable, and so - we just need to reduce the index of fields. // TODO be smarter about it", "decision": "", "done": ""} -->
| D-015 | `src/menu.rs:2186`–2187 | `SubMenu` | /// TODO(asweet): Allow multiple submenus to be open at a time.<br> | （TODO(asweet)）支持同时打开多个子菜单 | 上游 warp（/Users/matt/SynologyDrive/code/vpstools/warp，HEAD 5bf047181）app/src/menu.rs:2115 有同一句，本仓 menu.rs 是 refactor b105e7e 本地化时原样搬入，没做。 作者标识 asweet 是上游开发者，本仓无人跟进。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-22（保留，不动） |<!-- entry {"no": "D-015", "cls": "D", "file": "src/menu.rs", "key": "/// TODO(asweet): Allow multiple submenus to be open at a time.", "decision": "", "done": ""} -->
| D-016 | `src/menu.rs:2268` | `SubMenu.render_submenus` | // TODO(asweet): handle scrollable, or make scrollable nested items invalid | （TODO(asweet)）处理可滚动子菜单，或把「可滚动的嵌套项」判为非法 | 上游 warp（/Users/matt/SynologyDrive/code/vpstools/warp，HEAD 5bf047181）app/src/menu.rs:2199 有同一句，本仓 menu.rs 是 refactor b105e7e 本地化时原样搬入，没做。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-22（保留，不动） |<!-- entry {"no": "D-016", "cls": "D", "file": "src/menu.rs", "key": "// TODO(asweet): handle scrollable, or make scrollable nested items invalid", "decision": "", "done": ""} -->
| D-017 | `src/menu.rs:2854`–2855 | `SubMenu.render` | // TODO(asweet): Currently, this is based on the _current number of submenus_<br>// instead of the maximum. We should likely change this. | （TODO(asweet)）submenu 宽度判断基于当前子菜单数而非最大数，应改 | 上游 warp（/Users/matt/SynologyDrive/code/vpstools/warp，HEAD 5bf047181）app/src/menu.rs:2765 有同一句，本仓 menu.rs 是 refactor b105e7e 本地化时原样搬入，没做。 | 删除 | 保留：Warp 上游原文，不动（见拍板记录） | 2026-11-22（保留，不动） |<!-- entry {"no": "D-017", "cls": "D", "file": "src/menu.rs", "key": "// TODO(asweet): Currently, this is based on the _current number of submenus_ // instead of the maximum. We should likely change this.", "decision": "", "done": ""} -->

## 批次 core-3

### B 为捷径辩护

#### B-064 `src/ssh_session.rs:340`–344 `SshSession.connect`

<!-- entry {"no": "B-064", "cls": "B", "file": "src/ssh_session.rs", "key": "// Some SSH servers and middleboxes do not reply to OpenSSH global // keepalive requests even though the channel is still usable. Keep // sending protocol-level probes, but let the channel event loop'", "decision": "", "done": ""} -->
> // Some SSH servers and middleboxes do not reply to OpenSSH global
> // keepalive requests even though the channel is still usable. Keep
> // sending protocol-level probes, but let the channel event loop's
> // periodic no-op window-change detect real write failures instead
> // of closing a healthy idle session from russh's timeout counter.

- 捷径：忽略用户配置的 keep_alive_max_failures：`let _ = options.keep_alive_max_failures; cfg.keepalive_max = 0;`，russh 自身的超时计数被关掉
- 是否仍然成立：是。证据：ssh_session.rs:340-348。理由本身站得住：russh client/mod.rs:956 在 keepalive_max != 0 且超时次数超限才断开，部分服务器 / 中间设备不回全局 keepalive 会把健康连接误杀；断线改由 run_remote_ssh_event_loop 里的周期 no-op window-change 发现（terminal_runtime.rs:4326-4329）。但主机编辑窗口仍给用户一个「最大失败次数」输入框（host_edit_window.rs:59,418-442,625,678，库里 keep_alive_max_failures 默认 3，host_management.rs:52），这个值现在对 SSH 完全无效
- 建议：记待办
- 风险：中：用户改了「最大失败次数」不会有任何效果（死配置项）；是移除字段还是接上新语义属于产品决定
- 决定：修代码
- 拍板备注：推翻「记待办」。移除配置项，不接上（理由见拍板记录）。要删：主机编辑窗口的「最大失败次数」输入框和 draft 字段、各层连接配置结构里的 `keep_alive_max_failures`、`locales` 的 `form_max_failures`、`ssh_session.rs` 的 `let _ = options.keep_alive_max_failures;`；这段英文注释留下，它解释的是 `keepalive_max` 为什么是 0。连带文件 `host_edit_window.rs`、`host_management.rs`、`host_overview.rs`、`terminal_runtime.rs`、`root_view/host_library_section/edit_window.rs`、`locales/`、`lib.rs` 里的测试，已同意连带修改；少一个输入框这个可见变化已同意。旧库要能照常打开，旧版导出的主机文件要能照常导入，各留一条回归用例；库里那一列留不留由实现者定。排在 B-059 之后做。
- 完成：2026-11-22（库里那一列：新库不再建，旧库留着不读写）

#### B-065 `src/terminal_runtime.rs:3072`–3074 `LocalTerminalRuntime.spawn_remote_ssh`

<!-- entry {"no": "B-065", "cls": "B", "file": "src/terminal_runtime.rs", "key": "// 远程 shell 我们暂不注入 shell integration（要在远端写 rc 麻烦）， // 没有 marker 时 bootstrapped 永远 false → 一直占位，不能这样。 // 临时方案：远端 spawn 即视为 bootstrap 完成（直接连上就开始接受用户输入）。", "decision": "", "done": ""} -->
> // 远程 shell 我们暂不注入 shell integration（要在远端写 rc 麻烦），
> // 没有 marker 时 bootstrapped 永远 false → 一直占位，不能这样。
> // 临时方案：远端 spawn 即视为 bootstrap 完成（直接连上就开始接受用户输入）。

- 捷径：远程 SSH 不注入 shell integration，spawn 时直接把 bootstrapped 置 true，不等 marker（注释自称「临时方案」）
- 是否仍然成立：是。证据：terminal_runtime.rs:3072-3075 置 true；串口（terminal_runtime.rs:3140）和 Telnet（terminal_runtime/telnet.rs:82）同样直接置 true；只有本地 PTY 走 marker（scan_bootstrap_marker，terminal_runtime.rs:2572-2584；本地 launch.rs:13 对没有 integration 的 shell 到点直接发）。docs 里没有「远端注入 integration」的计划或 ADR（rg 无命中）
- 建议：保留
- 风险：低：行为是对的（没有 marker 就不该永远占位），只是「临时方案」措辞暗示以后会改而实际无此计划；建议保留逻辑、把注释改成「远端 / 串口 / Telnet 没有 marker，spawn 即视为 bootstrap 完成」
- 决定：保留
- 拍板备注：行为是对的：远端没有 marker，不能一直占位。把「暂不…（…麻烦）」「临时方案」这类措辞去掉，改成如实描述：远端 / 串口 / Telnet 没有 shell integration marker，spawn 即视为 bootstrap 完成。
- 完成：2026-11-22（保留行为，改写措辞）

#### B-066 `src/ui_colors.rs:2`–3 `(顶层)`

<!-- entry {"no": "B-066", "cls": "B", "file": "src/ui_colors.rs", "key": "//! 此处只 re-export，保持旧类型名 + `from_theme` 签名，消费方 call-site 零改动。", "decision": "", "done": ""} -->
> //! 此处只 re-export，保持旧类型名 + `from_theme` 签名，消费方 call-site 零改动。

- 捷径：ui_colors.rs 只 re-export design_tokens 的类型，为「消费方 call-site 零改动」保留旧名 UiColors / HostOverviewColors（兼容门面）
- 是否仍然成立：是。证据：ui_colors.rs:4-5 两行 `pub(crate) use ... as UiColors/HostOverviewColors`；src 下仍有 233 处引用旧名（rg `UiColors\|HostOverviewColors\|ui_colors::`，分布在 git_panel_*、host_management_view/*、root_view/* 等约 19 个文件），main.rs:31 `mod ui_colors;`。迁移是机械改名
- 建议：记待办
- 风险：低：多一层间接和两套名字，不影响行为；拖得越久引用越多
- 备注：整组是模块文档注释，另含对现状的说明；归 B 是因为「保持旧名零改动」是在为保留兼容层辩护
- 决定：记待办
- 拍板备注：待办内容：把 233 处 `UiColors` / `HostOverviewColors` 改成 design_tokens 的 `ChromeColors` / `OverviewColors`，删掉 `ui_colors.rs`。纯改名，挑没有并行分支的时候做。
- 完成：2026-11-22（T-003）

### C 过期的「为什么」

| 编号 | 位置 | 所在符号 | 过期的句子 | 依据 | 建议改成 | 决定 | 完成 |
|---|---|---|---|---|---|---|---|
| C-170 | `src/terminal_grid_element.rs:89`–92 | `(顶层)` | 「PTY 异步输出的 redraw 通路改由 RootView 端的后台轮询负责（见 main.rs `schedule_poll`），用 `ViewContext::spawn` + `Timer::after` 自重排」 | 全仓库没有 schedule_poll（rg 只命中这条注释本身）；现行通路是 root_view/mod.rs:909 attach_terminal_streams 里 `spawn_stream_local(throttle(WAKEUP_THROTTLE_PERIOD, wakeup_rx), Self::handle_terminal_wakeup, ..)`，即 pty 线程发 wakeup、UI 侧 60Hz 节流后重绘，没有轮询 | PTY 输出的重绘由 root_view::attach_terminal_streams 订阅节流后的 wakeup 流触发（见 handle_terminal_wakeup）；dispatch_event 只在自己处理事件后立刻 `ctx.notify()`，确保用户输入立即上屏。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-170", "cls": "C", "file": "src/terminal_grid_element.rs", "key": "// PTY 异步输出的 redraw 通路改由 RootView 端的后台轮询负责（见 main.rs // `schedule_poll`），用 `ViewContext::spawn` + `Timer::after` 自重排， // revision 变了才 `ctx.notify()`。dispatch_event 只在自己处理事件后立刻 // `ctx.notify()`，确保用户输入", "decision": "", "done": ""} -->
| C-171 | `src/terminal_grid_element.rs:558`–559 | `CellMetrics` | `grid_size_util.rs:15-57 ... :61-81` | ../warp (5bf047181) app/src/terminal/grid_size_util.rs 现为 grid_cell_dimensions 第 14-58 行、calculate_grid_baseline_position 第 62 行起，行号已漂移 | 去掉行号，只留函数名 grid_cell_dimensions / calculate_grid_baseline_position | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-171", "cls": "C", "file": "src/terminal_grid_element.rs", "key": "/// warp: grid_size_util.rs:15-57 grid_cell_dimensions + :61-81 calculate_grid_baseline_position", "decision": "", "done": ""} -->
| C-172 | `src/terminal_grid_element.rs:2102` | `(顶层)` | `warp view.rs:750-756 PADDING_LEFT = 16` | ../warp app/src/terminal/view.rs:746 现为 `pub static ref PADDING_LEFT: f32 = 16.;`，行号已漂移（值 16 仍对） | warp view.rs `PADDING_LEFT = 16`（LessHorizontalTerminalPadding），去掉行号 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-172", "cls": "C", "file": "src/terminal_grid_element.rs", "key": "// warp view.rs:750-756 PADDING_LEFT = 16 (LessHorizontalTerminalPadding)。", "decision": "", "done": ""} -->
| C-173 | `src/terminal_grid_element.rs:2143`–2144 | `(顶层)` | `Warp scrollable.rs:44` 的 `NUM_PIXELS_PER_LINE = 40` | ../warp 里该常量现在 crates/warpui_core/src/elements/gui/scrollable.rs:43（值 40 仍对），行号差 1、目录也没写 | Warp `warpui_core/.../gui/scrollable.rs` 的 `NUM_PIXELS_PER_LINE = 40`，去掉行号 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-173", "cls": "C", "file": "src/terminal_grid_element.rs", "key": "/// Warp scrollable.rs:44 的 `NUM_PIXELS_PER_LINE = 40` 放大。", "decision": "", "done": ""} -->
| C-174 | `src/terminal_grid_element.rs:2838` | `TerminalGridElement.paint` | `warp: workspace/util.rs:364-386` | ../warp app/src/workspace/util.rs 里对应逻辑 get_terminal_background_opacity 现在在 392 行起，364-386 已不是它 | warp: workspace/util.rs get_terminal_background_opacity — 有背景图时默认 bg 单元格也要半透明（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-174", "cls": "C", "file": "src/terminal_grid_element.rs", "key": "// warp: workspace/util.rs:364-386 — 有背景图时默认 bg 单元格也要半透明", "decision": "", "done": ""} -->
| C-175 | `src/terminal_runtime.rs:101`–102 | `(顶层)` | `crates/warp_terminal/src/model/escape_sequences.rs:255` | ../warp 该文件 255 行现在是 meta/legacy 键编码的回退分支（to_pty_bytes 一带），文件里已没有 SGR 鼠标上报编码函数（rg `fn .*sgr\|mouse` 只命中 alt_screen_scroll_to_pty_bytes 和 MouseState 的 use）；鼠标报告测试在 escape_sequences_tests.rs:115 起 | 去掉行号；约定出处写 warp_terminal 的 escape_sequences（对照 escape_sequences_tests.rs 的 test_mouse_actions_to_escape_sequence） | 按建议改，改法更正：去掉行号，符号写 escape_sequences.rs 的 `impl ToEscapeSequence for MouseState`（建议说文件里已没有鼠标编码，不对） | 2026-11-22 |<!-- entry {"no": "C-175", "cls": "C", "file": "src/terminal_runtime.rs", "key": "/// Warp encodes in `crates/warp_terminal/src/model/escape_sequences.rs:255`.", "decision": "", "done": ""} -->
| C-176 | `src/terminal_runtime.rs:2012`–2013 | `TerminalGridCore` | `app/src/terminal/grid_renderer.rs:639-695` | ../warp grid_renderer.rs 里 marked text 覆盖现在从 554 行 `grid.marked_text()` 取值、657 行起覆盖 cell，639-695 不再对应 | 去掉行号，写 `grid_renderer.rs` 里处理 marked_text 的部分 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-176", "cls": "C", "file": "src/terminal_runtime.rs", "key": "/// (`app/src/terminal/grid_renderer.rs:639-695`) 把 marked 字符从光标", "decision": "", "done": ""} -->
| C-177 | `src/terminal_runtime.rs:2358`–2375 | `TerminalRuntimeState` | `/// OSC 7 扫描跨 chunk 拼接缓冲；上限 OSC7_BUF_CAP，超出丢弃旧字节。`（terminal_runtime.rs:2368） | 全仓库没有 OSC7_BUF_CAP；常量是 OSC7_MAX_LEN = 4096（terminal_runtime.rs:5519），语义是「单条 OSC 7 超长丢弃」，consume_osc7_sequences 里 5540 行用它；osc7_scan_buf 本身没有独立上限 | OSC 7 扫描跨 chunk 拼接缓冲；只保留未完成序列的尾巴，单条超过 OSC7_MAX_LEN 的序列整段丢弃。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-177", "cls": "C", "file": "src/terminal_runtime.rs", "key": "/// shell integration marker 触发后翻 true，UI 由占位切到 grid。 /// 扫 bootstrap marker 的跨调用 buffer（marker 可能横跨多次 process_output）。 /// 占位文本 \"Starting {name}...\" 使用的 shell 名。 /// 最近一次 OSC 7 上报的本地 cwd（远程 tab 不解析，恒", "decision": "", "done": ""} -->
| C-178 | `src/terminal_runtime.rs:2670` | `apply_term_event` | `in this spike yet` | 这是 match 的兜底分支 `_ => {}`；「spike」阶段早已结束（docs 称 native-shell-spike 已是唯一架构），措辞误导成「以后会补」，但全仓库没有对应待办 | 其余事件（MouseCursorDirty / Wakeup 等）不处理。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-178", "cls": "C", "file": "src/terminal_runtime.rs", "key": "// MouseCursorDirty / Wakeup / etc. don't surface in this spike yet.", "decision": "", "done": ""} -->
| C-179 | `src/terminal_runtime.rs:3300`–3301 | `LocalTerminalRuntime` | `view.rs:644` | ../warp app/src/terminal/view.rs 里 WAKEUP_THROTTLE_PERIOD 现在在 616 行（用处在 3763 行），644 已漂移 | 去掉行号（或改成 view.rs 的 WAKEUP_THROTTLE_PERIOD） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-179", "cls": "C", "file": "src/terminal_runtime.rs", "key": "/// 60 Hz throttle (Warp's `WAKEUP_THROTTLE_PERIOD`,", "decision": "", "done": ""} -->
| C-180 | `src/terminal_runtime.rs:3860`–3862 | `LocalTerminalRuntime（游离注释，位于 impl 块之后、resize_request_is_duplicate 之前，不属于该函数）` | `EventLoopHandle::Drop sends Message::Shutdown, kills the child, and joins the PTY thread` | pty_event_loop/mod.rs:158-165 的 Drop 只发 Shutdown 再 join，注释写明「不自行 kill」，子进程靠 master fd 关闭后内核发 SIGHUP 回收；「kills the child」与代码不符 | EventLoopHandle::Drop 发 Message::Shutdown 并 join PTY 线程（子进程随 master fd 关闭由内核发 SIGHUP 回收），所以 LocalTerminalRuntime 不需要额外的 Drop impl——drop `event_loop: Option&lt;EventLoopHandle>` 就够了。 | 按建议改，改法更正：Drop 发 Shutdown 并 join；子进程还活着时由事件循环线程退出前 kill（unix，EventLoop::run 末尾）。建议里「随 master fd 关闭由内核发 SIGHUP 回收」不对 | 2026-11-22 |<!-- entry {"no": "C-180", "cls": "C", "file": "src/terminal_runtime.rs", "key": "// EventLoopHandle::Drop sends Message::Shutdown, kills the child, and joins // the PTY thread, so LocalTerminalRuntime needs no extra Drop impl — // dropping `event_loop: Option\u003cEventLoopHandle\u003e` is ", "decision": "", "done": ""} -->
| C-181 | `src/terminal_runtime.rs:4749`–4750 | `(顶层)` | `app/src/terminal/local_tty/unix.rs:261` | ../warp 里 build_host_shell_command 现在在 crates/warp_terminal/src/local_tty/unix.rs:275（目录已从 app/ 迁到 crates/warp_terminal/） | Warp: `crates/warp_terminal/src/local_tty/unix.rs` build_host_shell_command（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-181", "cls": "C", "file": "src/terminal_runtime.rs", "key": "/// Warp: `app/src/terminal/local_tty/unix.rs:261` build_host_shell_command", "decision": "", "done": ""} -->
| C-182 | `src/terminal_runtime.rs:4843`–4844 | `build_login_shell_command` | `app/src/terminal/local_tty/shell.rs:555` | ../warp 里 arguments_for_session_spawning_command 现在在 crates/warp_terminal/src/local_tty/shell.rs:617（目录已迁） | Warp: `crates/warp_terminal/src/local_tty/shell.rs` arguments_for_session_spawning_command（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-182", "cls": "C", "file": "src/terminal_runtime.rs", "key": "/// Warp: `app/src/terminal/local_tty/shell.rs:555` arguments_for_session_spawning_command", "decision": "", "done": ""} -->
| C-183 | `src/terminal_runtime.rs:4915`–4916 | `get_pw_home` | `app/src/terminal/local_tty/unix.rs:126` get_pw_entry | ../warp 全仓 rg get_pw_entry 无命中；该文件现在用 getpwuid_r 查询（crates/warp_terminal/src/local_tty/unix.rs:113-125，current_user_via_getpwuid），函数已改名/重写 | 对照 Warp `crates/warp_terminal/src/local_tty/unix.rs` 的 getpwuid_r 用户查询；去掉函数名和行号 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-183", "cls": "C", "file": "src/terminal_runtime.rs", "key": "/// Warp: `app/src/terminal/local_tty/unix.rs:126` get_pw_entry", "decision": "", "done": ""} -->
| C-184 | `src/terminal_runtime.rs:5027` | `TerminalPalette.from_theme` | `warp: workspace/view.rs:22856` | ../warp workspace/view.rs 里 theme.background_image() 的处理现在在 27646 行附近，22856 已漂移 | warp: workspace/view.rs 里按 theme.background_image() 处理终端背景透明（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-184", "cls": "C", "file": "src/terminal_runtime.rs", "key": "// warp: workspace/view.rs:22856 — 有背景图时终端背景半透明", "decision": "", "done": ""} -->
| C-185 | `src/terminal_runtime.rs:5876`–5877 | `kitty_modifier_tests` | openspec `terminal-kitty-modifier-keys` | 仓库 openspec/changes 下只有 upgrade-upstream-warp-ironrdp，没有这个 change，git 历史里也没有该路径的文件；上游提交 af2f7c61d 在 ../warp 存在（Encode Cmd/Option modifiers ... Kitty keyboard protocol #14012） | Kitty 键盘协议下 Option 修饰的编辑键编码（上游 warp af2f7c61d）。协议未激活时保持 legacy 序列不变。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-185", "cls": "C", "file": "src/terminal_runtime.rs", "key": "/// Kitty 键盘协议下 Option 修饰的编辑键编码（openspec `terminal-kitty-modifier-keys`，", "decision": "", "done": ""} -->
| C-186 | `src/terminal_view_helpers.rs:150` | `close_button_element` | `对照 warp tab.rs:984-1062 / icon_button` | ../warp app/src/tab.rs 里关闭按钮现在是 render_close_button_or_pin_icon（1527 行起），984-1062 已不是它 | 对照 warp tab.rs 的 render_close_button_or_pin_icon / icon_button（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-186", "cls": "C", "file": "src/terminal_view_helpers.rs", "key": "// Close button(× icon),hover 时背景提亮 ── 对照 warp tab.rs:984-1062 / icon_button。", "decision": "", "done": ""} -->
| C-187 | `src/throttle.rs:1`–2 | `(顶层)` | `vendored verbatim from` | 与上游并非逐字：diff ../warp/app/src/throttle.rs src/throttle.rs 显示 import 写法/顺序不同、少了一行 `// Stream has been closed.` 注释，逻辑相同 | Stream throttle adapter，取自 `warp/app/src/throttle.rs`（逻辑一致，仅 import 排序不同）。 | 按建议改：写「取自 warp/app/src/throttle.rs，逻辑一致」，不列具体差异（列了还会再过期） | 2026-11-22 |<!-- entry {"no": "C-187", "cls": "C", "file": "src/throttle.rs", "key": "//! Stream throttle adapter, vendored verbatim from", "decision": "", "done": ""} -->
| C-188 | `src/throttle.rs:3`–4 | `(顶层)` | `view.rs:644` `WAKEUP_THROTTLE_PERIOD` | ../warp app/src/terminal/view.rs WAKEUP_THROTTLE_PERIOD 现在在 616 行 | 去掉行号，写 Warp 的 `WAKEUP_THROTTLE_PERIOD = 1s / 60` | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-188", "cls": "C", "file": "src/throttle.rs", "key": "//! 60 Hz on the UI thread (Warp's `view.rs:644` `WAKEUP_THROTTLE_PERIOD", "decision": "", "done": ""} -->
| C-189 | `src/throttle.rs:5`–6 | `(顶层)` | `we keep it bit-for-bit identical` | diff ../warp/app/src/throttle.rs src/throttle.rs：逻辑相同，但 import 写法/顺序不同且少了一行 `// Stream has been closed.` 注释，不是逐字节相同 | 逻辑与上游一致（仅 import 排序和一行注释不同），保持一致以便行为对齐。 | 按建议改：写「逻辑与上游保持一致」，不列具体差异 | 2026-11-22 |<!-- entry {"no": "C-189", "cls": "C", "file": "src/throttle.rs", "key": "//! over there; we keep it bit-for-bit identical so behaviour matches.", "decision": "", "done": ""} -->
| C-190 | `src/warp_dropdown.rs:199` | `render_dropdown_menu` | `menu.rs:2002-2015` | ../warp app/src/menu.rs 里 ClippedScrollable 现在在 2233 行附近，2002-2015 是 select_first_selectable 之类 | warp: menu.rs 里带 ClippedScrollable 的下拉菜单（去掉行号） | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-190", "cls": "C", "file": "src/warp_dropdown.rs", "key": "// warp: menu.rs:2002-2015 — 带 ClippedScrollable 的下拉菜单", "decision": "", "done": ""} -->
| C-191 | `src/warp_tab_context_menu.rs:5`–6 | `(顶层)` | `The native spike currently has local terminal tabs only` | warp_tab_context_menu.rs:64-79 的 HorizontalTabContextMenuActions 已有 reconnect_tab / disconnect_tab / connection_info（RDP）/ toggle_recording，标签不再只有本地终端；「spike」命名同样过时 | NexShell 的标签有本地 / SSH / RDP / 串口 / Telnet 等多种，Warp 里依赖产品状态而我们没有的分区（共享会话、保存的标签配置）保持为空分区，不改造成自定义菜单。 | 按建议改 | 2026-11-22 |<!-- entry {"no": "C-191", "cls": "C", "file": "src/warp_tab_context_menu.rs", "key": "//! separators only between non-empty sections. The native spike currently has", "decision": "", "done": ""} -->

## 盘点方法

- 范围：`git ls-files`，排除 `docs/`、`openspec/`、`patches/`、`src/text_editor/`、`src/code_editor/`、生成代码、`node_modules/`、`vendor/`、`third_party/`、`dist/`、`testdata/`、`fixtures/`、`__snapshots__/`、Markdown 与二进制。
- `docs/` 是文档；`openspec/` 是 openspec 变更存档（只有 md / yaml）；`patches/` 是 IronRDP 调试用补丁文件，不是源码。
- `src/text_editor/`、`src/code_editor/` 是从 Warp 整份拷来再本地化的编辑器代码，保持和上游一致、便于对照摘取修复，不盘点（Matt 2026-11-21 拍板）。
- 抽取：`ast-grep scan` 按注释节点 kind 扫 bash、rust、yaml（kind 名先在小样本上确认）；`.vue` 按 HTML 解析，`<script>`、`<style>` 里的注释经注入扫出。Python docstring 用规则「模块、类、函数体里第一条（不计注释）表达式语句的字符串」抽取。没有解析器的文件按行首 `#` 抽取；YAML `run: |` 块和 shell heredoc 里嵌的脚本，解析器把注释当字符串，也按行首 `#` 补抽。
- 清理脚本（`delete_a.py`、`prove_comment_only.py`）按这里记下的配置认文件语言和注释节点 kind：全用默认。<!-- inventory-config {} -->
- 相邻的整行注释（同文件、同列、行号相连）合并成一组算一条；行尾注释单独算。「所在符号」取包含注释的最内层函数 / 方法 / 类型，顶层注释取紧随其后的声明。
- 跳过（工具读取或非注释内容）：shebang 9。明细：
  - `scripts/build-dmg.sh:1` shebang：`#!/usr/bin/env bash`
  - `scripts/build-icns.sh:1` shebang：`#!/usr/bin/env bash`
  - `scripts/cargo-env.sh:1` shebang：`#!/usr/bin/env bash`
  - `scripts/cargo-run.sh:1` shebang：`#!/usr/bin/env bash`
  - `scripts/tests/macos-bundle-metadata.sh:1` shebang：`#!/usr/bin/env bash`
  - `scripts/tests/warp-compatibility.sh:1` shebang：`#!/usr/bin/env bash`
  - `scripts/validate.sh:1` shebang：`#!/usr/bin/env bash`
  - `scripts/verify-terminal-decorations-signpost.sh:1` shebang：`#!/usr/bin/env bash`
  - `scripts/verify-warp-compatibility.sh:1` shebang：`#!/usr/bin/env bash`
- 交叉核对：每种语言用 rg 粗数行首注释行，与 ast-grep 覆盖的行数对比，差异逐行看过：

| 语言 | rg 粗数 | ast-grep 覆盖 | 差异原因 |
|---|---:|---:|---|
| bash | 24 | 24 | 一致 |
| hash-fallback | 71 | 71 | 一致 |
| rust | 4867 | 4722 | 145 行全是行首 `*` 解引用或乘号续行（如 `*stats.x.entry(k).or_default() += 1`），被 rg 误计；ast-grep 覆盖数加 145 正好等于 rg 数 |
| yaml | 59 | 59 | 一致 |

- 分类：按批次交给独立上下文的 sonnet 子代理，它们读代码、git 历史和依赖锁定版本自行判断；主会话只抽取和汇总。汇总时唯一的改动是把误归 A 的文档注释改回 C（文档注释不参与 A 类判断）。
- 增量更新：重跑时按（类别、文件、注释原文、同文原文的第几处）匹配每条的隐藏标记 `<!-- entry … -->`，沿用编号、决定、完成和手写的备注行；新条目接着编号；扫不到的旧条目移到「已消失的条目」。行号会随代码变化，不参与匹配。

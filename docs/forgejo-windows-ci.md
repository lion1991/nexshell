# Forgejo Windows CI 运维手册

对应 workflow：`.forgejo/workflows/windows-release.yml`。

## 1. 总览

```
GitHub（origin，不动）
nexshell 本地仓 ──push tag──▶ Forgejo（第二 remote）
warp-nexshell 本地仓 ──push──▶ Forgejo（第二 remote）
                                      │
                                      ▼
                         Forgejo Windows runner（label: windows）
                         checkout nexshell + 按 warp-compatibility.toml
                         钉住的 sha checkout warp（并排目录）
                                      │
                                      ▼
                     cargo build --release --features warpui-app
                                      │
                                      ▼
                   nexshell.exe → zip → 挂 Forgejo Release
```

要点：
- GitHub 上的 `origin` 保持现状不变，Forgejo 只是新增的第二个 remote，两边互不影响。
- 触发方式是给 nexshell 推 `v*` tag 到 Forgejo；`workflow_dispatch` 用于手动跑构建做验证。
- warp 依赖版本由仓库根 `warp-compatibility.toml` 的 `integration_mirror`（40 位 sha）钉死，
  workflow 运行时解析该文件取值，不写死在 workflow 里，改动只需改这一处 toml。
- 产物是 `nexshell-windows-x86_64-<版本>.zip`，tag 触发时自动挂到 Forgejo Release；
  手动触发时产物在 Actions run 的 artifact 里。

## 2. Forgejo 侧准备

1. 建两个私有仓，同一 owner 下：`<owner>/nexshell`、`<owner>/warp-nexshell`。
2. 确认 Forgejo 实例与这两个仓库都启用了 Actions（实例级 `[actions] ENABLED = true`；
   仓库 Settings → Actions 里勾选启用）。
3. `nexshell` 仓库 → Settings → Actions → Secrets，新增：
   - `REPO_READ_TOKEN`：Forgejo 个人 access token，只需 `repository` 的 read 权限，
     用于 workflow 拉取 `warp-nexshell` 私仓。
   - `RELEASE_TOKEN`（可选）：见第 6 节故障排查，默认 `GITHUB_TOKEN` 建 Release 若权限不够再配。
4. runner 注册 token：仓库级在该仓 Settings → Actions → Runners；实例级在管理后台
   Site Administration → Actions → Runners（一次注册可给多仓用，视需要选择粒度）。

## 3. 本地双 remote 推送

两个仓库各加一个 `forgejo` remote（SSH 地址替换成实际值）：

```bash
# nexshell 仓库
git remote add forgejo git@forgejo.example.com:<owner>/nexshell.git

# warp-nexshell 仓库
git remote add forgejo git@forgejo.example.com:<owner>/warp-nexshell.git
```

**推送顺序铁律：先推 warp，再推 nexshell 和 tag。**

```bash
# 1. warp-nexshell 仓库：先把钉住的 sha 及其历史推到 Forgejo
cd ../warp
git push forgejo <branch>

# 2. nexshell 仓库：再推代码和 tag
cd ../nexshell
git push forgejo main
git tag v1.2.3
git push forgejo v1.2.3
```

顺序反了的后果：workflow 里按 `warp-compatibility.toml` 钉住的 sha 去 Forgejo 上 fetch，
若这个 sha 还没推上去，checkout warp 那一步会直接失败。

## 4. Windows runner 搭建（从零到常驻）

### 工具链

- **VS Build Tools / Community 2022**：必须完整包含三件——MSVC 编译器、**VC 库**
  （组件 `VC.Tools.x86.x64`，缺了报 LNK1104 msvcrt.lib）、**Windows SDK**（缺了报
  LNK1104 kernel32.lib）。勾"使用 C++ 的桌面开发"工作负载即全含；半残安装用
  `setup.exe modify --add <组件> --quiet --norestart` 补。
- **rustup**：安装后 `rustup default stable-x86_64-pc-windows-msvc`。
- **protoc**：GitHub protobuf release 的 win64 zip 解压到 `C:\tools\protoc`，bin 加 PATH
  （winget 在 SSH 会话里会因 msstore 源协议问题失败，别依赖它）。
- **CMake**：Kitware release zip 解压到 `C:\tools\cmake`，bin 加 PATH（`libopus_sys` 编
  Opus 需要，走 VS 生成器 + MSBuild）。
- **NASM**：官方 win64 zip 解压到 `C:\tools\nasm` 加 PATH（`aws-lc-sys` 编汇编需要）。
- **Git for Windows**：workflow 里的 checkout warp 步骤直接调用 `git`，必须在 PATH 里。
- **Node.js LTS**：`actions/checkout`、`actions/upload-artifact` 等官方 action 是 JS action，
  跑在 runner 内置的 Node 运行时上，需要预先装好。

以上 PATH 变更都写**用户级** PATH（runner 以哪个用户跑就写谁的），改完必须重启 runner
进程才生效——daemon 只在启动时读一次环境。

**当前部署（2026-08-31 起）任务以 SYSTEM 账户跑**，看不到 matt 的用户级 PATH/rustup，
所以环境统一钉在 `daemon.cmd` 开头（不依赖任何账户的 PATH）：

```bat
set "CARGO_HOME=C:\Users\matt\.cargo"
set "RUSTUP_HOME=C:\Users\matt\.rustup"
set "PATH=C:\Users\matt\.cargo\bin;C:\tools\protoc\bin;C:\tools\cmake\bin;C:\tools\nasm;%PATH%"
```

新增工具只改这一行，然后 `schtasks /end /tn \forgejo-runner` + `schtasks /run /tn \forgejo-runner`
（SSH 以 matt 登录即为管理员令牌，不用输密码）。nexshell 的 `rust-toolchain.toml` 钉了
rustc 版本，rustup 会在首个 cargo 调用时自动装到上面的 `RUSTUP_HOME`；想省 CI 时间可提前
`rustup toolchain install <版本> --profile minimal -c rustfmt -c clippy`。

### 安装 forgejo-runner

1. 从 Forgejo 官方 release 下载 `forgejo-runner-*-windows-amd64.exe`。
2. 注册（`<注册token>` 取自第 2 节步骤 4）：

   ```powershell
   forgejo-runner.exe register `
     --instance https://<forgejo地址> `
     --token <注册token> `
     --name win-builder `
     --labels windows:host `
     --no-interactive
   ```

   注意：`--labels windows:host` 是注册语法（`<标签名>:<执行模式>`，`host` 表示直接跑在
   宿主机进程里，不走 docker）。workflow 里 `runs-on:` 引用的是标签名本身，即 `windows`，
   不是带冒号的完整字符串。

3. 常驻运行（当前部署即此方案）：任务计划程序建任务 `\forgejo-runner`，Action 指向
   wrapper 批处理 `C:\forgejo-runner\daemon.cmd`（内容：调 `forgejo-runner.exe daemon`
   并把 stdout/stderr 追加到 `C:\forgejo-runner\daemon.log`——直接指 exe 的话日志全丢）。
   注意：任务存了用户凭据，`schtasks /change` 改动作时会要求重输密码。

### host 模式的实际行为（实测勘误）

- **workspace 并不持久化**：每个 run 用独立的 `work\<runid>\hostexecutor` 目录，跑完即删。
  所以每次构建都是全量冷编译（warp 也每次重新 fetch，好在按 sha 浅拉取很快）；
  workflow 里 checkout warp 的幂等分支实际走不到"复用"路径，留着无害。
  实测冷构建全程约 11 分钟。持久化 `CARGO_TARGET_DIR` 提速是 v2 候选，但 path 依赖的
  指纹含绝对路径、而 workspace 路径每 run 都变，收益只覆盖 crates.io 依赖，未必划算。
- daemon 的 stdout 只记 task 接取，不含步骤结果与 job 日志——排障看 Forgejo 网页的
  Actions run 日志，别指望 daemon.log。
- 单台 runner 默认并发数是 1（同一时间只跑一个 job），不需要额外配置 `concurrency`。
- checkout warp 用的 PAT 会随 `git remote set-url` 留在持久化的 `warp/.git/config` 里，
  单用户构建机可接受；机器共用时注意。
- runner 拉取 `actions/checkout` 等官方 action 默认走 code.forgejo.org，加上 cargo 拉
  crates.io，构建机需要有出站互联网。

## 5. 发版操作手册

正常发版，完整命令序列：

```bash
cd ../warp
git push forgejo <branch>

cd ../nexshell
git push forgejo main
git tag v1.2.3
git push forgejo v1.2.3
```

推完 tag 后去 Forgejo 仓库的 Actions 页面看 run 状态；成功后到仓库 Releases 页面，
`v1.2.3` 下应该能看到 `nexshell-windows-x86_64-v1.2.3.zip`。

手动构建（不发 Release，只验证能不能编译过）：Forgejo 仓库 Actions 页面选中
`windows-release` workflow → Run workflow。产物在这次 run 的 Artifacts 列表里下载，
文件名形如 `nexshell-windows-x86_64-<short-sha>.zip`。

## 6. 故障排查

- **checkout warp 报 LFS smudge 404**：warp 仓用 LFS 追踪 `*.pdb` 和
  `crates/input_classifier/models/**`，但这些文件不参与 nexshell 构建（input_classifier
  不在依赖图里），且本地 macOS 无 LFS、一直以指针文件构建成功。workflow 已在 checkout warp
  步骤设 `GIT_LFS_SKIP_SMUDGE=1` 跳过下载，不要往 Forgejo 补推 LFS 对象。
- **`shell: cmd` 步骤报 `fork/exec .\cmd.exe` 找不到文件**：act_runner host 模式对 cmd
  shell 的解析不可靠（powershell 步骤不受影响）。规矩：workflow 里不用 `shell: cmd`，
  需要跑 bat 时用默认 powershell 包 `& cmd.exe /c "call xxx.bat"` 并检查 `$LASTEXITCODE`。
- **run 日志里中文乱码**：act 写出的 .ps1 无 BOM，Windows PowerShell 5.1 按 ANSI 读取。
  规矩：run 脚本内的输出/报错一律英文；step 名和 YAML 注释不受影响可用中文。
- **checkout warp 时 `git fetch --depth 1 origin <sha>` 失败**：workflow 已自动回退到
  全量 `git fetch origin` 重试，不需要人工介入（实测 Forgejo 支持按 sha 浅拉取，
  一般走不到回退分支）。
- **发布 Release 步骤报 403**：默认用的 `GITHUB_TOKEN`（Forgejo 内置的 job token）权限不足以
  创建 Release 时会出现。解决办法：另建一个有仓库写权限的 Forgejo PAT，配成仓库 secret
  `RELEASE_TOKEN`，workflow 会优先用它（`secrets.RELEASE_TOKEN || secrets.GITHUB_TOKEN`）。
- **产物没有出现在 Artifacts 里**：`actions/upload-artifact` 必须用 `@v3`——Forgejo 对 `v4`
  的支持还不稳定，workflow 里已固定用 v3，不要手滑升级版本号。
- **cargo / protoc / cmake 报"找不到命令"**：环境自检那一步会先失败并报错。两个常见原因：
  ① 工具刚装、PATH 刚改，但 daemon 没重启——daemon 只在启动时读一次环境，
  `schtasks /end` + `/run` 重启计划任务即可；② 工具装到了别的用户名下（任务 Run As 谁，
  就得装到谁的用户 PATH 或机器 PATH）。"本地命令行能跑、CI 里找不到"优先查这两条。

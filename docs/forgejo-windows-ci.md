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

- **VS Build Tools 2022**：安装时勾选"使用 C++ 的桌面开发"工作负载（MSVC 工具链）。
- **rustup**：安装后 `rustup default stable-x86_64-pc-windows-msvc`。
- **protoc**：`winget install Google.Protobuf`，或手动下载二进制放进 PATH。
- **Git for Windows**：workflow 里的 checkout warp 步骤直接调用 `git`，必须在 PATH 里。
- **Node.js LTS**：`actions/checkout`、`actions/upload-artifact` 等官方 action 是 JS action，
  跑在 runner 内置的 Node 运行时上，需要预先装好。

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

3. 常驻运行，二选一：

   **方案 A：NSSM 包装成 Windows 服务**（推荐，可设开机自启 + 崩溃自动重启）

   ```powershell
   nssm install forgejo-runner "C:\path\to\forgejo-runner.exe" daemon
   nssm set forgejo-runner AppDirectory "C:\path\to\runner目录"
   nssm start forgejo-runner
   ```

   **方案 B：任务计划程序**，建一个"登录时启动"或"开机启动"的任务，
   Action 指向 `forgejo-runner.exe daemon`，工作目录设为 runner 所在目录。

### host 模式的两个特性

- workspace 在 host 模式下是持久化的（不像 docker 模式每次用完就清），
  所以 `warp/` 目录会在多次构建之间保留，cargo 增量编译天然生效，第二次构建起会明显变快
  ——这也是 workflow 里 checkout warp 那步要做幂等处理（先判断 `warp/.git` 是否已存在）的原因。
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

- **checkout warp 时 `git fetch --depth 1 origin <sha>` 失败**：workflow 已自动回退到
  全量 `git fetch origin` 重试，不需要人工介入；首次全量 clone warp 仓库耗时较长属正常现象，
  之后 workspace 持久化，后续构建只需增量 fetch。
- **发布 Release 步骤报 403**：默认用的 `GITHUB_TOKEN`（Forgejo 内置的 job token）权限不足以
  创建 Release 时会出现。解决办法：另建一个有仓库写权限的 Forgejo PAT，配成仓库 secret
  `RELEASE_TOKEN`，workflow 会优先用它（`secrets.RELEASE_TOKEN || secrets.GITHUB_TOKEN`）。
- **产物没有出现在 Artifacts 里**：`actions/upload-artifact` 必须用 `@v3`——Forgejo 对 `v4`
  的支持还不稳定，workflow 里已固定用 v3，不要手滑升级版本号。
- **cargo / protoc 报"找不到命令"**：环境自检那一步会先失败并报错，多数是 NSSM 服务的
  运行账户 PATH 和交互登录会话的 PATH 不一致——用 NSSM 把 runner 包装成服务后，
  服务进程默认继承的是 Local System 或指定账户的系统级环境变量，不会自动带上你在
  交互式终端里手动加的用户级 PATH 项。必要时在 NSSM 里显式配置：

  ```powershell
  nssm set forgejo-runner AppEnvironmentExtra PATH=C:\rust\bin;C:\protoc\bin;%PATH%
  ```

  这是最常见的坑，遇到"本地命令行能跑、CI 里找不到"时优先查这里。

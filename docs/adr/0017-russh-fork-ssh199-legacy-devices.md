# russh fork 补丁：兼容 SSH-1.99 / ssh-rsa 老设备

Status: accepted (2026-10-02)；升级 russh 后撤销

## 背景

交换机 192.168.254.252（`SSH-1.99-OpenSSH_6.8`）连接报 `SSH connection failed: Disconnected`。实测其提议：

- KEX：`curve25519-sha256@libssh.org`、`diffie-hellman-group-exchange-sha256`、`diffie-hellman-group14-sha1`
- 主机密钥：只有 `ssh-rsa`、`ssh-dss`
- MAC：无 sha2 系，只有 `hmac-sha1` / `md5` / `umac` 等

两处不兼容：

1. russh 0.46 读版本行只认 `SSH-2.0-` 前缀，`SSH-1.99-`（RFC 4253 §5.1：兼容 v2）被当成前导行丢弃，一直读到对端断开 → `Error::Disconnect`。上游 #514（2025-04）已修，但该修复所在版本与 0.46 之间 russh-keys 已并入 russh、密钥类型换成 `ssh-key`，升级牵动面大。
2. russh 默认主机密钥算法不含 `ssh-rsa`（SHA-1 签名）。

## 决策

- fork `lion1991/russh`，分支 `nexshell-0.46` 从上游 tag `v0.46.0`（`e7a9ee9`）出发，`[patch.crates-io]` 只把 `russh` 钉到该分支 rev，两个补丁：
  1. `russh/Cargo.toml` 去掉 russh-keys / russh-cryptovec / russh-util 的 `path`，三者仍走 crates.io。tag 上的 cryptovec 与发布的 0.7.3 源码不同（含 #351 wasm 重构），这样依赖与 crates.io 版 russh 0.46.0 完全一致。
  2. `russh/src/ssh_read.rs`：版本行接受 `SSH-1.99-`，原串返回（交换哈希要用对端原串，不能改写成 2.0）。
- 最初用 `vendor/russh` 拷贝源码（约 630K），随即改为 fork 钉 rev，与 IronRDP 做法一致、仓库不背源码。
- `ssh_session.rs` 在默认主机密钥算法末位追加 `ssh-rsa`，新服务器仍优先 ed25519 / ecdsa / rsa-sha2。russh 读 known_hosts 时 RSA 记录一律按 rsa-sha2-256 解析，比对前把密钥算法统一到 SHA2_256，否则 ssh-rsa 记录永远对不上。
- MAC 不用动：该设备上 chacha20-poly1305 可用，AEAD 不走单独 MAC。

已用独立探针经该交换机验证：版本行接受、协商出 `curve25519-sha256@libssh.org` / `ssh-rsa` / `chacha20-poly1305`、主机签名校验通过，服务端提供 `publickey,password` 认证。

## 后续

- 升级 russh 到含 #514 的版本（顺带拿上游 2026-05 的安全修复）时删掉 `russh` 的 patch 项、弃用 fork 分支，并按新 API 改写 ssh-rsa 追加与 known_hosts 归一化。

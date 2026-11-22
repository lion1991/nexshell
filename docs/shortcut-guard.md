# 捷径检查

提交前（`.githooks/pre-commit`）、推送前（`.githooks/pre-push`）和 CI（`.forgejo/workflows/shortcut-guard.yml`）用同一个脚本 `.githooks/check_shortcuts.py` 查新增行。新增按 `git diff -M` 算：整文件改名、移动不算；文件内挪动、格式化重排出来的行算。存量不查。

## 拦什么

1. **辩护注释**：注释里出现 workaround、hack、temporary、for now、暂时、先这样、临时（只认原词和英文复数），而同一条注释没有待办引用。「temporary file / directory」「临时文件 / 临时目录」不算。只看注释，不看字符串和标识符。
2. **待办**：注释里的 TODO / FIXME / XXX 必须带待办引用。
3. **无理由的检查豁免**：`eslint-disable`、`@ts-expect-error`、`@vue-expect-error`、`//nolint`、`#nosec`、`# type: ignore`、`# mypy:`、`# noqa`、`# pylint: disable`、`# shellcheck disable`、`# hadolint ignore`、`# yamllint disable`、`istanbul / v8 / c8 ignore` 等要点名规则并写理由；`@ts-ignore`、`@vue-ignore` 不用。只认写在注释开头的指令，正文里提到指令名不算。`t.Skip()`、`pytest.skip()`、`@unittest.skip`、`xfail`、`it.skip`、`it.todo` 要写原因。
4. **语言捷径**（测试代码不查）：Rust 的 `unwrap()`，失败就 panic。改用 `?` 往上传，或 `.expect("为什么这里不可能失败")`。脚本里还带着 Go、Python、TS/JS 的规则，本仓库用不到。

**待办引用**的格式是 `T-001` 这样的编号，必须已登记在 `docs/todo.md` 且没被划掉（划掉表示已完成）。Forgejo / GitHub 的 issue 没在用，所以用这份清单。

## 为什么拦

这些写法都在说「没做完，先提交了」。辩护注释让读的人以为已经权衡过；没有登记的待办没人跟进；没理由的豁免悄悄关掉了检查；丢掉的 error、吞掉的异常要到现场才暴露。AI 写代码时尤其爱走这几条路。

## 被拦下怎么办

按报错提示把捷径修掉；修不了就在 `docs/todo.md` 登记一行，在注释里写上编号。只换个说法绕开关键词不算解决。第 4 类确实绕不开的，在同一行或上一行写 `shortcut-ok: 理由`（理由去掉标点后至少 3 个汉字或 6 个字母数字，或写已登记的编号）。

## 范围

NexShell 用到的语言里查 Rust、shell、YAML、TOML；`.m`（`warp_ime_shim.m`）、`.bat`、`.patch`、Markdown 不查，也不提示。语言捷径对 Rust 只有 `unwrap()` 一条：`tests/` 目录和 `#[cfg(test)]` 之后的代码算测试代码不查，`examples/` 照查。检查豁免只认注释形式的，Rust `#[allow(...)]` 这类属性不拦。不查：检查脚本所在目录 `.githooks/`（脚本与测试）、`docs/`、生成代码（头 5 行的注释里同时写有 generated 和 DO NOT EDIT，或写 `@generated`）、`testdata/`、`fixtures/`、`vendor/`、`*.snap`。

## 启用与绕过

本地启用：`git config core.hooksPath .githooks`，新克隆要各自执行一次。pre-push 查要推上去的新提交（推 main 查远端 main 之后的，其他分支查和 `origin/main` 分叉之后的，和 CI 一致），挡住 `git commit --no-verify` 之后直接推送。`git push --no-verify` 仍能绕过本地两道，CI（Forgejo 的 `go` runner）会再查一遍，但 CI 是推送之后才跑；要在服务端拦住，得在 Forgejo 给 main 开分支保护并把 shortcut-guard 设为必过。只推 GitHub（`origin`）不跑 CI。

回归用例：`python3 .githooks/test_check_shortcuts.py`。

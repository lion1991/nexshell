"""check_shortcuts.py 的回归用例：每条都在临时仓库里真实暂存（或提交）后跑检查。

运行：python3 <检查脚本所在目录>/test_check_shortcuts.py（默认是 .githooks/）
"""
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

HOOK_DIR = os.path.dirname(os.path.abspath(__file__))
CHECKER = os.path.join(HOOK_DIR, "check_shortcuts.py")
# 没有 Go 的仓库按 guard.md 删掉了 goblank/，就不复制它，按类型判断的用例也跳过
HAS_GOBLANK = os.path.exists(os.path.join(HOOK_DIR, "goblank", "main.go"))
GUARD_FILES = ("pre-commit", "pre-push", "check_shortcuts.py", "test_check_shortcuts.py") + (("goblank/main.go",) if HAS_GOBLANK else ())
TODO_MD = "# 待办清单\n\n| 编号 | 要做什么 | 登记日期 |\n|---|---|---|\n| T-001 | 补超时重试 | 2026-11-14 |\n"


def sh(cwd, *args, check=True):
    return subprocess.run(args, cwd=cwd, check=check, capture_output=True, text=True)


class Repo:
    def __init__(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = os.path.realpath(self.tmp.name)
        sh(self.dir, "git", "init", "-q", "-b", "main")
        for k, v in (("user.email", "t@example.com"), ("user.name", "t"), ("commit.gpgsign", "false")):
            sh(self.dir, "git", "config", k, v)

    def write(self, files):
        for path, text in files.items():
            full = os.path.join(self.dir, path)
            os.makedirs(os.path.dirname(full), exist_ok=True)
            with open(full, "w", encoding="utf-8") as f:
                f.write(text)
        sh(self.dir, "git", "add", "-A")

    def commit(self, files=None, msg="c"):
        if files:
            self.write(files)
        sh(self.dir, "git", "commit", "-q", "--no-verify", "--allow-empty", "-m", msg)

    def git(self, *args, check=True):
        return sh(self.dir, "git", *args, check=check)

    def install(self, where=".githooks"):
        """把检查脚本、hook 和测试复制进仓库的 where 目录，返回复制后的检查脚本路径。"""
        for name in GUARD_FILES:
            dst = os.path.join(self.dir, where, name)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            shutil.copy2(os.path.join(HOOK_DIR, name), dst)
        return os.path.join(self.dir, where, "check_shortcuts.py")

    def check(self, *args, checker=CHECKER):
        r = sh(self.dir, sys.executable, checker, *args, check=False)
        self.stderr, self.rc = r.stderr, r.returncode
        return [(m.group(1), int(m.group(2)), m.group(3))
                for m in re.finditer(r"^(\S.*?):(\d+): \[([\w-]+)\]", r.stderr, re.M)]

    def close(self):
        self.tmp.cleanup()


def run(files, base=None, todo=True):
    """暂存 files 后跑检查，返回 [(path, line, rule)]。todo=True 时清单里登记了 T-001。"""
    repo = Repo()
    try:
        if base:
            repo.commit(base)
        if todo:
            files = dict(files, **{"docs/todo.md": TODO_MD})
        repo.write(files)
        return repo.check()
    finally:
        repo.close()


def rules(files, **kw):
    return [h[2] for h in run(files, **kw)]


class Case(unittest.TestCase):
    def hit(self, path, text, rule, **kw):
        got = rules({path: text}, **kw)
        self.assertIn(rule, got, f"{path} 应被 {rule} 拦下：\n{text}")

    def ok(self, path, text, **kw):
        got = rules({path: text}, **kw)
        self.assertEqual([], got, f"{path} 不该报：\n{text}")


# ---------------------------------------------------------------- 规则 1：辩护注释

class Excuse(Case):
    def test_each_language_blocked(self):
        self.hit("a.go", "package a\n\n// workaround: 先跳过校验\nfunc F() {}\n", "excuse")
        self.hit("a.py", "x = 1  # 暂时不加锁\n", "excuse")
        self.hit("a.ts", "export const a = 1 // hack around the cache\n", "excuse")
        self.hit("a.js", "// for now we skip it\nexport const a = 1\n", "excuse")
        self.hit("a.vue", "<template>\n  <!-- 先这样，后面再改 -->\n  <p/>\n</template>\n", "excuse")
        self.hit("a.sh", "echo hi  # temporary fix\n", "excuse")
        self.hit("a.yml", "on: push  # 临时关掉 PR 触发\n", "excuse")
        self.hit("a.css", "/* hack: 覆盖 element-plus */\n.a { color: red; }\n", "excuse")
        self.hit("a.scss", ".a { color: red; } // hack\n", "excuse")
        self.hit("a.html", "<body>\n  <!-- 暂时隐藏 -->\n</body>\n", "excuse")
        self.hit("a.rs", "fn main() {} // workaround\n", "excuse")
        self.hit("a.toml", "[tool]\nx = 1  # 暂时写死\n", "excuse")
        self.hit("Dockerfile", "FROM alpine\n# hack: 固定镜像版本\n", "excuse")
        self.hit("Makefile", "build:\n\tgo build  # for now 不带版本号\n", "excuse")

    def test_each_language_with_todo_ref_passes(self):
        self.ok("a.go", "package a\n\n// workaround: 上游 netlink 不支持，见 T-001\nfunc F() {}\n")
        self.ok("a.py", "x = 1  # 暂时不加锁（T-001）\n")
        self.ok("a.ts", "export const a = 1 // hack around the cache, T-001\n")
        self.ok("a.js", "// for now we skip it, tracked in T-001\nexport const a = 1\n")
        self.ok("a.vue", "<template>\n  <!-- 先这样，见 T-001 -->\n  <p/>\n</template>\n")
        self.ok("a.sh", "echo hi  # temporary fix (T-001)\n")
        self.ok("a.yml", "on: push  # 临时关掉 PR 触发，T-001\n")
        self.ok("a.css", "/* hack: 覆盖 element-plus，T-001 */\n.a { color: red; }\n")
        self.ok("a.scss", ".a { color: red; } // hack, T-001\n")
        self.ok("a.html", "<body>\n  <!-- 暂时隐藏，T-001 -->\n</body>\n")
        self.ok("a.rs", "fn main() {} // workaround T-001\n")
        self.ok("a.toml", "[tool]\nx = 1  # 暂时写死，T-001\n")
        self.ok("Dockerfile", "FROM alpine\n# hack: 固定镜像版本，T-001\n")
        self.ok("Makefile", "build:\n\tgo build  # for now 不带版本号，T-001\n")

    def test_ref_elsewhere_in_same_comment_counts(self):
        self.ok("a.go", "package a\n\n/*\n * workaround：上游不支持\n * 见 T-001\n */\nfunc F() {}\n")
        self.ok("a.py", "# 暂时绕开上游 bug\n# 见 T-001\nx = 1\n")

    def test_ref_in_a_different_comment_does_not_count(self):
        self.hit("a.py", "# 暂时绕开上游 bug\n\n# 见 T-001\nx = 1\n", "excuse")
        self.hit("a.go", "package a\n\n// T-001\nvar X = 1 // workaround\n", "excuse")

    def test_temp_file_and_directory_pass(self):
        self.ok("a.go", "package a\n\n// 先写临时文件再改名，保证原子替换\nfunc F() {}\n")
        self.ok("a.py", "# 测试把配置指到临时目录\nx = 1\n")
        self.ok("a.ts", "// write to a temporary file first, then a temporary directory\nexport {}\n")
        self.hit("a.go", "package a\n\n// 临时 IP 连不上属正常\nfunc F() {}\n", "excuse")
        self.hit("a.ts", "// temporary key\nexport {}\n", "excuse")

    def test_block_comment_middle_lines(self):
        self.hit("a.go", "package a\n\n/*\n说明\n这里暂时不校验\n*/\nfunc F() {}\n", "excuse")
        self.hit("a.ts", "/**\n * 说明\n * for now we skip it\n */\nexport {}\n", "excuse")
        self.hit("a.vue", "<template>\n  <!--\n    说明\n    先这样\n  -->\n  <p/>\n</template>\n", "excuse")
        self.hit("a.vue", "<style>\n/*\n  hack: 覆盖 element-plus\n*/\n</style>\n", "excuse")

    def test_keywords_in_strings_and_identifiers_pass(self):
        self.ok("a.go", 'package a\n\nvar msg = "暂时不可用 // workaround"\nvar workaroundCount, isTemporary = 1, true\n')
        self.ok("a.go", "package a\n\nvar q = `多行\n// hack\n`\n")
        self.ok("a.py", 's = "# 先这样"\nt = """\n# temporary\n"""\nhack_count = 1\n')
        self.ok("a.ts", "const s = '// for now'\nconst t = `/* 临时 */`\nconst tempHack = 1\n")
        self.ok("a.sh", "echo \"# hack\"\necho '# 暂时'\n")
        self.ok("a.yml", "title: \"# 先这样\"\nnote: don't # 测试配置\n")

    def test_double_slash_in_urls_passes(self):
        self.ok("a.vue", "<template>\n  <a href=\"https://x.example/a\">见 https://x.example/hack</a>\n</template>\n")
        self.ok("a.yml", "url: https://x.example/hack\n")
        self.ok("a.sh", "curl https://x.example/#temporary\n")
        self.ok("a.go", 'package a\n\nconst U = "http://x.example//暂时"\n')
        self.ok("a.ts", "const re = /^https?:\\/\\/x\\/hack$/\nconst u = 'https://x.example/for now'\n")
        self.ok("a.vue", "<style>\n.a { background: url(https://x.example/hack.png); }\n</style>\n")

    def test_regex_literal_does_not_open_block_comment(self):
        self.hit("a.js", "const re = /\\/*$/\nconst x = 1 // hack\n", "excuse")


# ---------------------------------------------------------------- 规则 2：待办

class Todo(Case):
    def test_each_language(self):
        for path, bad, good in (
                ("a.go", "package a\n\n// TODO: 补错误处理\n", "package a\n\n// TODO(T-001): 补错误处理\n"),
                ("a.py", "# FIXME 回头再看\n", "# FIXME(T-001): 换成流式解析\n"),
                ("a.ts", "export {} // XXX 边界没测\n", "export {} // XXX(T-001) 边界没测\n"),
                ("a.js", "/* TODO */\n", "/* TODO T-001 */\n"),
                ("a.vue", "<template>\n  <!-- TODO 改文案 -->\n</template>\n", "<template>\n  <!-- TODO(T-001) 改文案 -->\n</template>\n"),
                ("a.sh", "# TODO 加重试\n", "# TODO(T-001) 加重试\n"),
                ("a.yml", "a: 1 # TODO\n", "a: 1 # TODO(T-001)\n"),
                ("a.html", "<!-- TODO -->\n", "<!-- TODO(T-001) -->\n"),
                ("a.rs", "// FIXME\n", "// FIXME(T-001)\n"),
                ("a.toml", "x = 1 # TODO\n", "x = 1 # TODO(T-001)\n"),
                ("Dockerfile", "# FIXME 换基础镜像\nFROM alpine\n", "# FIXME(T-001) 换基础镜像\nFROM alpine\n"),
                ("Makefile", "# XXX 并行构建\nall:\n", "# XXX(T-001) 并行构建\nall:\n")):
            with self.subTest(path=path):
                self.hit(path, bad, "todo")
                self.ok(path, good)

    def test_unregistered_ref(self):
        self.assertEqual(["todo-ref"], rules({"a.go": "package a\n\n// TODO(T-099): 补\n"}))
        got = run({"a.go": "package a\n\n// TODO(T-001): 补\n"}, todo=False)
        self.assertEqual([("a.go", 3, "todo-ref")], got)

    def test_message_tells_next_id_and_format(self):
        repo = Repo()
        self.addCleanup(repo.close)
        repo.write({"docs/todo.md": TODO_MD, "a.go": "package a\n\n// TODO 补\n"})
        repo.check()
        self.assertIn("docs/todo.md", repo.stderr)
        self.assertIn("TODO(T-002): …", repo.stderr)

    def test_code_todos_are_not_comments(self):
        self.ok("a.kt", 'fun f(): Int = TODO("补实现")\n')
        self.ok("a.rs", "fn f() -> i32 { todo!() }\n")
        self.ok("a.go", 'package a\n\nvar s = "TODO"\n')


# ---------------------------------------------------------------- 规则 3：检查豁免

class Suppress(Case):
    def test_pairs(self):
        for path, bad, good in (
                ("a.js", "// eslint-disable-next-line no-console\nconsole.log(1)\n",
                 "// eslint-disable-next-line no-console -- 调试面板需要输出原始日志\nconsole.log(1)\n"),
                ("a.js", "/* eslint-disable */\n", "/* eslint-disable no-console -- 这是命令行脚本，输出就是结果 */\n"),
                ("a.vue", "<script setup>\n// eslint-disable-next-line vue/no-mutating-props\nprops.a = 1\n</script>\n",
                 "<script setup>\n// eslint-disable-next-line vue/no-mutating-props -- 父组件约定由子组件回写\nprops.a = 1\n</script>\n"),
                ("a.ts", "// @ts-expect-error\nfoo()\n", "// @ts-expect-error -- element-plus 的类型漏了 size 字段\nfoo()\n"),
                ("a.go", "package a\n\nfunc F() { g() //nolint\n}\n", "package a\n\nfunc F() { g() //nolint:errcheck // 进程马上退出，日志关不掉也无妨\n}\n"),
                ("a.go", "package a\n\nvar c = exec.Command(x) // #nosec G204\n", "package a\n\nvar c = exec.Command(x) // #nosec G204 -- x 来自内置白名单\n"),
                ("a.py", "import foo  # type: ignore\n", "import foo  # type: ignore[import-untyped]  # foo 没发布类型存根\n"),
                ("a.py", "import foo  # type: ignore[import-untyped]\n", "import foo  # type: ignore[import-untyped]  # T-001\n"),
                ("a.py", "x = 1  # noqa\n", "x = 1  # noqa: E501  # 长 URL 没法折行\n"),
                ("a.py", "x = eval(s)  # nosec\n", "x = eval(s)  # nosec B307  表达式来自内置模板\n"),
                ("a.sh", "# shellcheck disable=SC2086\necho $x\n", "# shellcheck disable=SC2086 # 这里要按空格拆成多个参数\necho $x\n"),
                ("a.vue", "<template>\n  <!-- @vue-expect-error -->\n  <C :a=\"1\"/>\n</template>\n",
                 "<template>\n  <!-- @vue-expect-error 组件库的 props 类型漏了 a -->\n  <C :a=\"1\"/>\n</template>\n"),
                ("a.py", "# mypy: ignore-errors\n", "# mypy: ignore-errors  # 旧接口转译层，T-001 拆掉\n"),
                ("a.py", "# pylint: disable-next=broad-except\n", "# pylint: disable-next=broad-except  # 插件异常不能拖垮主循环\n"),
                ("Dockerfile", "# hadolint ignore=DL3008\nRUN apt-get install -y curl\n",
                 "# hadolint ignore=DL3008 # 基础镜像随发行版走，不钉版本\nRUN apt-get install -y curl\n"),
                ("a.yml", "a: 1  # yamllint disable-line rule:line-length\n", "a: 1  # yamllint disable-line rule:line-length # 证书指纹不能折行\n"),
                ("a.ts", "/* istanbul ignore next */\nexport {}\n", "/* istanbul ignore next -- 只在 Windows 上走到 */\nexport {}\n"),
                ("a.ts", "/* v8 ignore next 3 */\nexport {}\n", "/* v8 ignore next 3 -- 只在 Windows 上走到 */\nexport {}\n")):
            with self.subTest(path=path, bad=bad):
                self.hit(path, bad, "suppress")
                self.ok(path, good)

    def test_ts_ignore_always_blocked(self):
        self.hit("a.ts", "// @ts-ignore 第三方类型缺字段，没办法修\nfoo()\n", "suppress")
        self.hit("a.vue", "<template>\n  <!-- @vue-ignore 组件库类型漏了字段 -->\n  <C/>\n</template>\n", "suppress")

    def test_mentioning_a_directive_is_not_a_suppression(self):
        self.ok("a.go", "package a\n\n// 这里不加 nolint，错误照样往上抛\nfunc F() {}\n")
        self.ok("a.py", "# 别用 noqa 或 type: ignore 糊过去\nx = 1\n")
        self.ok("a.js", "// 不要用 eslint-disable 关规则\nexport {}\n")
        self.ok("a.sh", "# 别加 shellcheck disable=SC2086\necho \"$x\"\n")

    def test_placeholder_reason_is_not_a_reason(self):
        self.hit("a.js", "// eslint-disable-next-line no-console -- 理由\nconsole.log(1)\n", "suppress")

    def test_test_skips(self):
        self.hit("a_test.go", "package a\n\nfunc TestX(t *testing.T) {\n\tt.Skip()\n}\n", "test-skip")
        self.ok("a_test.go", "package a\n\nfunc TestX(t *testing.T) {\n\tt.Skip(\"需要 root 权限\")\n}\n")
        self.hit("tests/test_a.py", "@unittest.skip\ndef test_x(self):\n    pass\n", "test-skip")
        self.ok("tests/test_a.py", "@unittest.skip('需要外网')\ndef test_x(self):\n    pass\n")
        self.hit("a.test.ts", "it.skip('loads', () => {})\n", "test-skip")
        self.hit("a.test.ts", "it.todo('loads')\n", "test-skip")
        self.hit("a_test.go", "package a\n\nfunc TestX(t *testing.T) {\n\tt.Skip(\"\")\n}\n", "test-skip")
        self.hit("tests/test_a.py", "def test_x():\n    pytest.skip()\n", "test-skip")
        self.hit("tests/test_a.py", "@pytest.mark.xfail\ndef test_x():\n    pass\n", "test-skip")
        self.hit("tests/test_a.py", "@pytest.mark.skip(reason='')\ndef test_x():\n    pass\n", "test-skip")
        self.ok("tests/test_a.py", "def test_x():\n    pytest.skip('需要真实网卡')\n")
        self.ok("tests/test_a.py", "@pytest.mark.xfail(reason='上游 bug，T-001')\ndef test_x():\n    pass\n")
        self.ok("a.test.ts", "it.skip('loads', () => {}) // shortcut-ok: 依赖的测试服务下线了，见 T-001\n")


# ---------------------------------------------------------------- 规则 4：语言捷径

class LanguageShortcuts(Case):
    def test_go_blank_text_fallback(self):
        # 临时仓库里没有 go.mod，走文本规则
        self.hit("a.go", "package a\n\nfunc F() {\n\tb, _ := json.Marshal(v)\n}\n", "go-blank")
        self.hit("a.go", "package a\n\nfunc F() {\n\tdefer func() { _ = c.Close() }()\n}\n", "go-blank")
        for line in ("v, _ := m[k]", "s, _ := x.(string)", "v, _ := <-ch", "before, _, _ := strings.Cut(s, \"=\")",
                     "for _, v := range xs {\n\t}", "_, err := f()"):
            with self.subTest(line=line):
                self.ok("a.go", f"package a\n\nfunc F() {{\n\t{line}\n}}\n")
        self.ok("a.go", "package a\n\nvar _ io.Reader = (*T)(nil)\n")
        self.hit("a.go", "package a\n\nvar y, _ = strconv.Atoi(s)\n", "go-blank")

    def test_waiver(self):
        self.ok("a.go", "package a\n\nfunc F() {\n\t_ = f.Close() // shortcut-ok: 只读文件，关闭失败不影响结果\n}\n")
        self.ok("a.go", "package a\n\nfunc F() {\n\t// shortcut-ok: 只读文件，关闭失败不影响结果\n\t_ = f.Close()\n}\n")
        self.hit("a.go", "package a\n\nfunc F() {\n\t_ = f.Close() // shortcut-ok: 理由\n}\n", "waiver")
        self.hit("a.go", "package a\n\nfunc F() {\n\t_ = f.Close() // shortcut-ok: 暂时没空处理\n}\n", "excuse")

    def test_python(self):
        self.hit("a.py", "try:\n    f()\nexcept:\n    raise\n", "py-bare-except")
        self.hit("a.py", "try:\n    f()\nexcept Exception:\n    # best effort\n\n    pass\n", "py-swallow")
        self.hit("a.py", "with suppress(Exception):\n    f()\n", "py-swallow")
        self.ok("a.py", "try:\n    f()\nexcept FileNotFoundError:\n    pass\n")
        self.ok("a.py", "try:\n    f()\nexcept Exception as e:\n    log.warning('x: %s', e)\n")

    def test_ts_any(self):
        for line in ("const x = y as any", "function f(a: any) {}", "const r: Record<string, any> = {}",
                     "type H = any", "const m = new Map<any, string>()", "let v: string | any",
                     "function g<T = any>() {}", "const x = y as unknown as Foo"):
            with self.subTest(line=line):
                self.hit("a.ts", line + "\n", "ts-any")
        self.hit("a.vue", "<script setup lang=\"ts\">\nconst x = y as any\n</script>\n", "ts-any")
        self.ok("a.ts", "const s = \"as any\"\nconst company = obj.anyway\n")
        self.ok("a.vue", "<template>\n  <p>Warning: any unsaved change will be lost</p>\n</template>\n")

    def test_js_swallow(self):
        self.hit("a.js", "try { a() } catch (e) {}\n", "js-swallow")
        self.hit("a.mjs", "try {\n  a()\n} catch {\n\n}\n", "js-swallow")
        self.hit("a.ts", "p.catch(() => {})\n", "js-swallow")
        self.ok("a.js", "try { a() } catch (e) { console.error(e) }\n")
        self.ok("a.js", "try { a() } catch { /* 只是提示，读不到就不显示 */ }\n")
        self.ok("a.js", "try {\n  a()\n} catch {\n  // 下次轮询再试，不打扰用户\n}\n")

    def test_rust(self):
        self.hit("src/a.rs", "fn f() { let v = s.parse::<i32>().unwrap(); }\n", "rs-unwrap")
        self.ok("src/a.rs", "fn f() -> Result<i32, E> { let v = s.parse::<i32>()?; Ok(v) }\n")

    def test_test_code_is_exempt(self):
        self.ok("tests/it.rs", "fn t() { s.parse::<i32>().unwrap(); }\n")
        self.ok("src/a.rs", "fn f() {}\n\n#[cfg(test)]\nmod tests {\n    fn t() { s.parse::<i32>().unwrap(); }\n}\n")
        # 只修饰单项的 cfg(test) 不让后面的生产代码漏查；cfg(all(test, …)) 的模块、tests.rs 算测试代码
        self.hit("src/a.rs", "#[cfg(test)] use std::fmt;\n#[cfg(test)]\nfn helper() {}\n\nfn f() { x.unwrap(); }\n", "rs-unwrap")
        self.hit("src/a.rs", "#[cfg(not(test))]\nmod real {\n    fn f() { x.unwrap(); }\n}\n", "rs-unwrap")
        self.ok("src/a.rs", "fn f() {}\n\n#[cfg(all(test, feature = \"x\"))]\n#[allow(dead_code)]\npub(crate) mod tests {\n    fn t() { x.unwrap(); }\n}\n")
        self.ok("src/a.rs", "#[cfg(test)] mod tests {\n    fn t() { x.unwrap(); }\n}\n")
        self.ok("src/rdp/tests.rs", "fn t() { x.unwrap(); }\n")
        self.ok("a_test.go", "package a\n\nfunc TestX(t *testing.T) {\n\tb, _ := json.Marshal(v)\n}\n")
        self.ok("tests/test_a.py", "try:\n    f()\nexcept:\n    pass\n")
        self.ok("a.spec.ts", "const x = y as any\n")


GO_MOD = {"go.mod": "module example.com/m\n\ngo 1.21\n"}
GO_TYPED = """package m

import (
\t"errors"
\t"io/fs"
\t"net"
\t"os"
\t"strconv"
)

type myErr struct{}

func (*myErr) Error() string { return "" }

var _ error = (*myErr)(nil)

func pair() (string, bool) { return "", true }

func F(s string, n *net.IPNet, err error) int {
\tv, _ := pair()
\tones, _ := n.Mask.Size()
\t_, isPath := err.(*fs.PathError)
\tvar target *fs.PathError
\t_ = errors.As(err, &target)
\tc, _ := strconv.Atoi(s)
\t_ = os.Remove(s)
\t_ = isPath
\treturn len(v) + ones + c
}
"""


@unittest.skipUnless(shutil.which("go") and HAS_GOBLANK, "没有 go 工具链或 goblank/，按类型判断的用例跑不了")
class GoTyped(unittest.TestCase):
    def test_only_discarded_errors_are_reported(self):
        got = run(dict(GO_MOD, **{"m.go": GO_TYPED}))
        self.assertEqual([("m.go", 25, "go-blank"), ("m.go", 26, "go-blank")], got)

    def test_partially_staged_file_falls_back_to_text_rule(self):
        repo = Repo()
        self.addCleanup(repo.close)
        repo.write(dict(GO_MOD, **{"m.go": GO_TYPED}))
        with open(os.path.join(repo.dir, "m.go"), "a", encoding="utf-8") as f:
            f.write("\nvar Y = 1\n")
        got = [h[1] for h in repo.check()]
        self.assertIn(20, got)  # pair() 的 bool 被文本规则误判，说明确实回退了
        self.assertIn("没做类型检查", repo.stderr)


class AddedLines(unittest.TestCase):
    def repo(self, base):
        r = Repo()
        self.addCleanup(r.close)
        r.commit(base)
        return r

    def test_existing_code_is_ignored(self):
        r = self.repo({"a.go": "package a\n\n// TODO: 旧的\nfunc F() {\n\t_ = g()\n}\n"})
        r.write({"a.go": "package a\n\n// TODO: 旧的\nfunc F() {\n\t_ = g()\n}\n\nfunc H() {}\n"})
        self.assertEqual([], r.check())

    def test_whole_file_rename_is_not_new(self):
        r = self.repo({"a/old.py": "# TODO 旧的\ntry:\n    f()\nexcept:\n    pass\n"})
        os.makedirs(os.path.join(r.dir, "b"))
        r.git("mv", "a/old.py", "b/new.py")
        self.assertEqual([], r.check())

    def test_rename_with_edit_only_reports_edited_line(self):
        body = "".join(f"x{i} = {i}\n" for i in range(20))
        r = self.repo({"old.py": "# TODO 旧的\n" + body})
        r.git("mv", "old.py", "new.py")
        r.write({"new.py": "# TODO 旧的\n" + body + "# 暂时这样\n"})
        self.assertEqual([("new.py", 22, "excuse")], r.check())

    def test_in_file_move_and_reformat_count_as_new(self):
        r = self.repo({"a.go": "package a\n\nfunc F() {\n\t_ = g() // TODO 旧的\n}\n\nfunc H() {}\n"})
        r.write({"a.go": "package a\n\nfunc H() {}\n\nfunc F() {\n\tif true {\n\t\t_ = g() // TODO 旧的\n\t}\n}\n"})
        self.assertEqual({("a.go", 7, "todo"), ("a.go", 7, "go-blank")}, set(r.check()))

    def test_excluded_paths(self):
        repo = Repo()
        self.addCleanup(repo.close)
        checker = repo.install(".githooks")  # 检查脚本自己的目录不查，要从仓库里运行才算得出这个目录
        repo.write({
            "gen.go": "// Code generated by wails; DO NOT EDIT.\npackage a\n// TODO x\n",
            "docs/x.py": "# TODO 暂时\n",
            "sub/docs/y.go": "// TODO\n",
            ".githooks/x.py": "# TODO 暂时\n",
            "web/node_modules/x/a.js": "// TODO\n",
            "vendor/x/a.go": "// TODO\n",
            "pkg/testdata/a.go": "// TODO\n",
            "tests/fixtures/a.py": "# TODO\n",
            "src/__snapshots__/a.ts": "// TODO\n",
            "src/a.ts.snap": "// TODO\n",
            "notes.md": "TODO 暂时\n",
        })
        self.assertEqual([], repo.check(checker=checker))

    def test_merge_commit_only_checks_conflict_resolution(self):
        r = self.repo({"a.py": "x = 1\n"})
        r.git("switch", "-q", "-c", "other")
        r.commit({"b.py": "# TODO 别的分支带来的\n"})
        r.git("switch", "-q", "main")
        r.commit({"c.py": "y = 2\n"})
        r.git("merge", "-q", "--no-commit", "--no-ff", "other")
        self.assertEqual([], r.check())
        r.write({"c.py": "y = 2  # 暂时\n"})
        self.assertEqual([("c.py", 1, "excuse")], r.check())

    def test_user_diff_config_does_not_hide_files(self):
        r = self.repo({"a.py": "x = 1\n"})
        r.git("config", "diff.noprefix", "true")
        r.git("config", "diff.mnemonicPrefix", "true")
        r.write({"src/my file.py": "y = 1  # hack here\n"})
        self.assertEqual([("src/my file.py", 1, "excuse")], r.check())

    def test_initial_commit(self):
        self.assertEqual(["todo"], rules({"a.py": "# TODO x\n"}))

    def test_range_mode_from_merge_base(self):
        r = self.repo({"docs/todo.md": TODO_MD, "a.py": "# TODO 旧的\n"})
        r.git("switch", "-q", "-c", "feat")
        r.commit({"b.py": "# TODO(T-001) 有登记\nx = 1  # 先这样\n"})
        base = r.git("merge-base", "main", "HEAD").stdout.strip()
        self.assertEqual([("b.py", 2, "excuse")], r.check("--range", f"{base}..HEAD"))
        self.assertEqual([("b.py", 2, "excuse")], r.check("--range", "main...HEAD"))
        r.check("--range", "HEAD~1")
        self.assertEqual(2, r.rc)


class ReviewRegressions(Case):
    """代码审查查出的问题，各留一条回归。"""

    def test_stock_line_in_second_file_of_multi_file_commit(self):
        repo = Repo()
        self.addCleanup(repo.close)
        repo.commit({"a.py": "x = 1\ny = 2\n", "b.py": "a = 1\nb = 2\n# hack old\nc = 3\nd = 4\n"})
        repo.write({"a.py": "x = 1\ny = 3\n", "b.py": "a = 1\nb = 2\n# hack old\nc = 3\nd = 5\n"})
        self.assertEqual([], repo.check())

    def test_noqa_case_and_space_separated_codes(self):
        self.hit("a.py", "import os  # NOQA\n", "suppress")
        self.hit("a.py", "import os  # noqa: F401 E402 W503\n", "suppress")
        self.ok("a.py", "import os  # noqa: F401 E402  # 插件靠 import 副作用注册\n")

    def test_double_cast_through_any(self):
        self.hit("a.ts", "const x = y as any as Foo\n", "ts-any")

    def test_keyword_split_across_comment_lines(self):
        self.ok("a.py", "# uses a temporary\n# file to hold the dump\nx = 1\n")
        self.hit("a.py", "# do this for\n# now only\nx = 1\n", "excuse")

    def test_message_placeholders_are_not_reasons(self):
        self.hit("a.go", "package a\n\nfunc F() {\n\t_ = f.Close() // shortcut-ok: <为什么可以忽略>\n}\n", "waiver")
        self.hit("a.js", "try { a() } catch { /* <为什么可以忽略> */ }\n", "js-swallow")

    def test_struck_out_id_is_reserved_but_not_valid(self):
        todo = TODO_MD + "| ~~T-002~~ | 已做完 | 2026-11-14 |\n"
        got = run({"docs/todo.md": todo, "a.go": "package a\n\n// TODO(T-002): 补\n// TODO 再补\n"}, todo=False)
        self.assertEqual([("a.go", 3, "todo-ref"), ("a.go", 4, "todo")], got)
        repo = Repo()
        self.addCleanup(repo.close)
        repo.write({"docs/todo.md": todo, "a.go": "package a\n\n// TODO 补\n"})
        repo.check()
        self.assertIn("T-003", repo.stderr)

    def test_tokenizer_edges(self):
        self.ok("a.sh", "echo \"第一行\n# workaround 是文字的一部分\"\n")
        self.hit("a.js", "const n = i++ / 2; const m = n / 3 // hack\n", "excuse")
        self.hit("a.mts", "export const a = 1 // TODO\n", "todo")
        self.hit("a.yml", "a: `x\nb: 1 # TODO\n", "todo")


class FableReview(Case):
    """独立验收查出的问题，各留一条回归。"""

    def test_css_url_double_slash_is_not_a_comment(self):
        self.ok("a.css", ".a { background: url(https://cdn.example/hack.png); }\n")
        self.ok("a.scss", ".a { background: url(https://cdn.example/hack.png); }\n")
        self.ok("a.css", ".a { background: url('https://cdn.example/hack.png'); }\n")
        self.hit("a.css", ".a { color: red; } /* hack */\n", "excuse")

    def test_vue_scss_style_line_comment(self):
        self.hit("a.vue", "<style lang=\"scss\">\n.a { color: red; } // hack\n</style>\n", "excuse")
        self.ok("a.vue", "<style lang=\"scss\">\n.a { background: url(https://x.example/hack.png); }\n</style>\n")

    def test_shell_heredoc_body_is_data(self):
        self.ok("a.sh", "cat <<EOF >/etc/x.conf\n# 临时配置\nEOF\n")
        self.ok("a.sh", "cat <<-'EOF'\n\t# hack\n\tEOF\n")
        self.hit("a.sh", "cat <<EOF >/etc/x.conf\na=1\nEOF\n# 临时配置\n", "excuse")
        self.hit("a.sh", "echo $((1 << 2))  # hack\n", "excuse")

    def test_nested_template_strings(self):
        self.ok("a.js", "const s = `a ${b ? `x // hack` : 'y'}`\n")
        self.ok("a.ts", "const s = `a ${f({ k: `// hack` })} b`\nconst t = 1\n")
        self.hit("a.js", "const s = `a ${b ? `x` : 'y'}` // hack\n", "excuse")

    def test_html_sections(self):
        self.ok("a.html", "<script type=\"module\" src=\"/src/main.ts\"></script>\n<p>见 https://x.example/hack</p>\n")
        self.hit("a.html", "<script>\n  const a = 1 // hack\n</script>\n", "excuse")
        self.ok("a.html", "<script>\n  const re = /\\/\\/hack/\n</script>\n")

    def test_generated_header_must_be_standard(self):
        self.hit("a.go", "// DO NOT EDIT\npackage a\n\n// TODO 补\n", "todo")
        self.hit("a.py", "x = 1\ns = 'DO NOT EDIT'\n# TODO 补\n", "todo")
        self.ok("a.go", "// Code generated by protoc-gen-go. DO NOT EDIT.\npackage a\n\n// TODO 补\n")
        self.ok("a.py", "# Generated by the protocol buffer compiler.  DO NOT EDIT!\n# TODO 补\n")
        self.ok("a.ts", "/* @generated */\n// TODO 补\n")

    def test_word_list_is_literal(self):
        for text in ("# hacky but fine\nx = 1\n", "# temporarily disabled\nx = 1\n", "# a work-around\nx = 1\n"):
            with self.subTest(text=text):
                self.ok("a.py", text)
        self.hit("a.py", "# two hacks here\nx = 1\n", "excuse")
        self.hit("a.py", "# known workarounds\nx = 1\n", "excuse")

    def test_waiver_hint_in_messages(self):
        repo = Repo()
        self.addCleanup(repo.close)
        repo.write({"a.py": "try:\n    f()\nexcept:\n    raise\n", "a.ts": "const x = y as any\n"})
        repo.check()
        self.assertIn("# shortcut-ok:", repo.stderr)
        self.assertIn("// shortcut-ok:", repo.stderr)


class SecondReview(Case):
    """修 Fable 验收问题后的审查查出的问题，各留一条回归。"""

    def test_generated_header_variants(self):
        for head in ("// THIS FILE IS GENERATED. DO NOT EDIT.\n", "// GENERATED FILE - DO NOT EDIT\n",
                     "// DO NOT EDIT. This file is generated by foo\n", "/**\n * Generated by foo.\n * DO NOT EDIT\n */\n",
                     "# autogenerated, DO NOT EDIT\n"):
            with self.subTest(head=head):
                self.ok("a.ts" if not head.startswith("#") else "a.py", head + ("# TODO 补\n" if head.startswith("#") else "// TODO 补\n"))
        self.ok("a.go", "// Code generated by x. DO NOT EDIT.\r\npackage a\r\n\r\n// TODO 补\r\n")
        self.hit("a.ts", "export const s = 'generated, DO NOT EDIT'\n// TODO 补\n", "todo")

    def test_component_names_starting_with_script_or_style(self):
        self.hit("a.vue", "<template>\n  <style-picker v-model=\"x\" />\n</template>\n<script setup lang=\"ts\">\nconst a: any = 1\n</script>\n", "ts-any")
        self.ok("a.vue", "<template>\n  <script-loader/>\n  <p>见 https://x.example/hack</p>\n</template>\n")

    def test_closing_script_tag_not_at_line_start(self):
        self.ok("a.html", "<script>\n  foo();</script>\n<p>after // hack 不是注释</p>\n")

    def test_heredoc_delimiter_only_from_code(self):
        self.hit("a.sh", "echo \"<<A\" ; cat <<EOF\nbody\nEOF\nx=1 # hack\n", "excuse")
        self.hit("a.sh", "cat <<EOF\r\n# 正文\r\nEOF\r\nx=1 # hack\r\n", "excuse")
        self.hit("a.sh", "cat <<A; cat <<B\n# a\nA\n# b\nB\nx=1 # hack\n", "excuse")
        self.ok("a.sh", "cat <<A; cat <<B\n# hack\nA\n# 临时\nB\n")

    def test_unquoted_scss_lang(self):
        self.hit("a.vue", "<style lang=scss>\n.a { color: red; } // hack\n</style>\n", "excuse")


class Hook(unittest.TestCase):
    def test_hook_blocks_commit_with_actionable_message(self):
        repo = Repo()
        self.addCleanup(repo.close)
        repo.install(".githooks")
        repo.git("config", "core.hooksPath", ".githooks")
        repo.write({"a.py": "x = 1  # 暂时不校验\n"})
        r = repo.git("commit", "-m", "x", check=False)
        self.assertNotEqual(0, r.returncode)
        self.assertIn("a.py:1: [excuse]", r.stderr)
        self.assertIn("docs/todo.md", r.stderr)
        repo.write({"a.py": "x = 1\n"})
        r = repo.git("commit", "-m", "x", check=False)
        self.assertEqual(0, r.returncode, r.stderr)

    def test_pre_push_blocks_commits_made_with_no_verify(self):
        remote = tempfile.TemporaryDirectory()
        self.addCleanup(remote.cleanup)
        sh(remote.name, "git", "init", "-q", "--bare", "-b", "main")
        repo = Repo()
        self.addCleanup(repo.close)
        repo.install(".githooks")
        repo.git("config", "core.hooksPath", ".githooks")
        repo.git("remote", "add", "origin", remote.name)
        repo.commit({"a.py": "# TODO 存量\nx = 1\n"})
        self.assertEqual(0, repo.git("push", "-q", "--no-verify", "origin", "main", check=False).returncode)
        repo.git("fetch", "-q", "origin")
        # 已有分支：只查远端之后的提交，存量的 TODO 不算
        repo.commit({"b.py": "y = 1  # 暂时不校验\n"})
        r = repo.git("push", "origin", "main", check=False)
        self.assertNotEqual(0, r.returncode)
        self.assertIn("b.py:1: [excuse]", r.stderr)
        self.assertNotIn("a.py", r.stderr)
        # 新分支：查和远端 main 分叉之后的提交
        repo.git("switch", "-q", "-c", "feat", "HEAD~1")
        repo.commit({"c.py": "z = 1  # hack\n"})
        r = repo.git("push", "origin", "feat", check=False)
        self.assertNotEqual(0, r.returncode)
        self.assertIn("c.py:1: [excuse]", r.stderr)
        repo.commit({"c.py": "z = 1\n"})
        r = repo.git("push", "origin", "feat", check=False)
        self.assertEqual(0, r.returncode, r.stderr)
        # 合并 main、rebase 后强推：别人已在远端 main 上的行不算到推送者头上
        repo.git("switch", "-q", "main")
        repo.git("reset", "-q", "--hard", "HEAD~1")
        repo.commit({"m.py": "w = 1  # 暂时不校验\n"})
        self.assertEqual(0, repo.git("push", "-q", "--no-verify", "origin", "main", check=False).returncode)
        repo.git("fetch", "-q", "origin")
        repo.git("switch", "-q", "feat")
        repo.git("merge", "-q", "--no-edit", "main")
        r = repo.git("push", "origin", "feat", check=False)
        self.assertEqual(0, r.returncode, r.stderr)
        repo.git("reset", "-q", "--hard", "HEAD~1")
        repo.git("rebase", "-q", "main")
        r = repo.git("push", "--force-with-lease", "origin", "feat", check=False)
        self.assertEqual(0, r.returncode, r.stderr)
        # 删除远端分支不查
        self.assertEqual(0, repo.git("push", "origin", "--delete", "feat", check=False).returncode)


class SkillFixes(Case):
    """comment-cleanup skill 修复轮确认的问题，各留一条回归。"""

    def test_guard_installed_outside_githooks_skips_itself(self):
        repo = Repo()
        self.addCleanup(repo.close)
        checker = repo.install("tools/shortcut-guard")
        repo.write({"a.py": "x = 1  # 暂时不校验\n", ".githooks/x.py": "# TODO 暂时\n"})
        got = set(repo.check(checker=checker))
        self.assertEqual({("a.py", 1, "excuse"), (".githooks/x.py", 1, "excuse"), (".githooks/x.py", 1, "todo")}, got)

    def test_hooks_installed_outside_githooks(self):
        remote = tempfile.TemporaryDirectory()
        self.addCleanup(remote.cleanup)
        sh(remote.name, "git", "init", "-q", "--bare", "-b", "main")
        repo = Repo()
        self.addCleanup(repo.close)
        repo.install("tools/shortcut-guard")
        repo.git("config", "core.hooksPath", "tools/shortcut-guard")
        repo.git("remote", "add", "origin", remote.name)
        repo.write({"a.py": "x = 1  # 暂时不校验\n"})
        r = repo.git("commit", "-m", "x", check=False)
        self.assertNotEqual(0, r.returncode)
        self.assertIn("a.py:1: [excuse]", r.stderr)
        self.assertNotIn("tools/shortcut-guard", r.stderr)
        repo.write({"a.py": "x = 1\n"})
        r = repo.git("commit", "-m", "x", check=False)
        self.assertEqual(0, r.returncode, r.stderr)
        self.assertEqual(0, repo.git("push", "-q", "origin", "main", check=False).returncode)
        repo.git("fetch", "-q", "origin")
        repo.commit({"b.py": "y = 1  # hack\n"})
        r = repo.git("push", "origin", "main", check=False)
        self.assertNotEqual(0, r.returncode)
        self.assertIn("b.py:1: [excuse]", r.stderr)

    def test_long_minified_line_is_linear(self):
        import time
        sys.path.insert(0, HOOK_DIR)
        import check_shortcuts
        line = "var a=b/c,d=e/f;" * 12500  # 20 万字符的单行
        t = time.time()
        lines, groups = check_shortcuts.tokenize(line + "// hack\n", "public/app.min.js")
        self.assertLess(time.time() - t, 2)  # 平方级时 8 万字符就要 8 秒
        self.assertEqual([" hack"], [c[1] for c in lines[0][1]])
        self.hit("public/app.min.js", line + "// hack\n", "excuse")

    def test_code_tail_matches_full_join(self):
        import random
        sys.path.insert(0, HOOK_DIR)
        import check_shortcuts as cs
        rnd = random.Random(7)
        atoms = ["a", "x", " ", "\t", "(", ")", "=", "+", "++", "-", "--", "<", "/re/", "''", "${", "return", "typeof",
                 "xreturn", "in", "of", "await", "url()", "}"]
        for _ in range(3000):
            code = [p for _ in range(rnd.randint(0, 14)) for p in rnd.choice(atoms)] if rnd.random() < .5 \
                else [rnd.choice(atoms) for _ in range(rnd.randint(0, 14))]
            full, tail = "".join(code), cs.code_tail(code)
            with self.subTest(code=code):
                self.assertEqual(bool(cs.JS_REGEX_BEFORE.search(full)), bool(cs.JS_REGEX_BEFORE.search(tail)))
                self.assertEqual(bool(re.search(r"(\+\+|--)\s*$|<$", full)), bool(re.search(r"(\+\+|--)\s*$|<$", tail)))

    def test_rust_multiline_string(self):
        self.ok("src/a.rs", 'fn f() {\n    let s = "line1\n   see http://x.y/temporary/path\n";\n}\n')
        self.ok("src/a.rs", 'fn f() {\n    let s = "a \\"q\\" // hack\n b";\n}\n')
        self.hit("src/a.rs", 'fn f() {\n    let s = "line1\n line2"; // hack\n}\n', "excuse")
        self.hit("src/a.rs", "fn f<'a>(x: &'a str) -> char {\n    let c = '\"'; // hack\n    c\n}\n", "excuse")

    def test_rust_raw_strings(self):
        """raw 串里的 \\ 不是转义、" 后跟不够个数的 # 不结束；认错了后面整个文件都会漏查。"""
        def lines(text):
            return sorted((ln, rule) for _, ln, rule in run({"src/a.rs": text}))

        self.assertEqual([(2, "todo"), (3, "excuse")],
                         lines('let p = r"C:\\tmp\\";\n// TODO fix this\nlet q = 1; // hack here\n'))
        self.assertEqual([(2, "todo"), (3, "rs-unwrap")],
                         lines('let j = r#"{"a": 1, "b"#;\n// TODO fix this\nfn f() { x.unwrap(); }\n'))
        self.assertEqual([(5, "excuse")],
                         lines('let a = br##"x "# // hack\n y"##;\nlet b = cr"\n// hack"; let v = var"q";\n'
                               'let c = r"\\"; // hack\n'))
        self.ok("src/a.rs", 'fn f() {\n    let s = r#"a\n // TODO hack\n"#;\n}\n')


class NexShell(Case):
    """NexShell 主要是 Rust：文档注释、块注释中间行、字符串和 URL 里的关键词、测试模块里的 unwrap。"""

    def test_rust_comments(self):
        self.hit("src/a.rs", "/// 暂时不处理 IPv6\npub fn f() {}\n", "excuse")
        self.hit("src/a.rs", "//! workaround：上游 crate 还没修\n", "excuse")
        self.hit("src/a.rs", "/*\n * 说明\n * 这里先这样\n */\nfn f() {}\n", "excuse")
        self.ok("src/a.rs", "/// 写到临时文件再原子改名\n/// 用临时目录放解包内容\nfn f() {}\n")
        self.ok("src/a.rs", 'fn f() -> &\'static str { "暂时 // hack" }\n')
        self.ok("src/a.rs", 'const URL: &str = "https://x.example//temporary";\n')

    def test_rust_unwrap(self):
        self.hit("examples/probe.rs", "fn main() { std::env::args().nth(1).unwrap(); }\n", "rs-unwrap")
        self.ok("src/a.rs", "fn f(x: Option<u8>) -> u8 { x.unwrap_or(0) + x.unwrap_or_default() }\n")
        self.ok("src/a.rs", 'fn f(x: Option<u8>) -> u8 { x.expect("调用方已校验非空") }\n')
        self.ok("src/a.rs", "fn f() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { Some(1).unwrap(); }\n}\n")

    def test_rust_whole_file_rename(self):
        r = Repo()
        try:
            r.commit({"src/old.rs": "fn f() { x.unwrap(); } // hack\n"})
            r.git("mv", "src/old.rs", "src/new.rs")
            self.assertEqual([], r.check())
        finally:
            r.close()


if __name__ == "__main__":
    unittest.main()

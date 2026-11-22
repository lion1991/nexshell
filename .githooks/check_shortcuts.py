#!/usr/bin/env python3
"""只查新增行：为捷径辩护的注释、没带待办引用的 TODO、不写理由的检查豁免、各语言的常见捷径。

「新增」= git diff -M 的 + 行：整文件改名、移动不算；文件内挪动、格式化重排出来的行算。
约定见 docs/shortcut-guard.md，待办清单在 docs/todo.md。
"""
import argparse
import functools
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TODO_FILE = "docs/todo.md"

# 仓库自己的归档、禁入目录加在 REPO_EXCLUDED_DIRS（按目录名匹配任意层级）
REPO_EXCLUDED_DIRS = set()
EXCLUDED_DIRS = {"docs", "node_modules", "vendor", "Vendor", "third_party", "dist",
                 "testdata", "fixtures", "__fixtures__", "__snapshots__"} | REPO_EXCLUDED_DIRS
EXCLUDED_SUFFIXES = (".snap", ".golden")

C_LIKE = {".go", ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".rs", ".kt", ".java", ".c", ".h", ".cs"}
HASH = {".py", ".sh", ".bash", ".zsh", ".yml", ".yaml", ".toml", ".ps1"}
HASH_NAMES = {"Makefile", "Dockerfile"}
JS_FAMILY = {".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".vue"}
TYPED_TS = {".ts", ".tsx", ".mts", ".cts", ".vue"}
CHAR_LITERAL = {".go", ".rs", ".kt", ".java", ".c", ".h", ".cs"}
TRIPLE_QUOTE = {".py", ".kt", ".toml"}
SHELL = {".sh", ".bash", ".zsh"}
MULTILINE_DQ = {".rs"}  # 双引号串可以跨行（Rust 里 ' 是字符或生命周期，不会成为跨行的 quote）
SCSS = {".scss", ".less"}  # 和 CSS 不同，有 // 行注释
HTML = {".html", ".htm"}


def lang_of(path):
    base = os.path.basename(path)
    ext = os.path.splitext(base)[1]
    if ext == ".vue" or ext in HTML:
        return "vue"  # 都按 template / script / style 分段
    if ext == ".css":
        return "css"
    if ext in SCSS:
        return "scss"
    if ext in C_LIKE:
        return "c"
    if ext in HASH or base in HASH_NAMES or base.startswith("Dockerfile"):
        return "hash"
    return None


# 只认工具真会生成的文件头：Go 用官方格式，其余要在头 5 行的注释里同时写 generated（英文，不分大小写）和「DO NOT EDIT」，或写 @generated
GO_GENERATED = re.compile(r"^// Code generated .* DO NOT EDIT\.\r?$", re.M)
COMMENT_LINE = re.compile(r"^\s*(//|#|/?\*+|<!--)")


def guard_dir(top):
    """检查脚本所在目录（相对仓库根）。脚本和它的测试都放在这里，整个目录不查；不在仓库里或就在根目录时为 None。"""
    rel = os.path.relpath(os.path.realpath(HERE), top).replace(os.sep, "/")
    return None if rel == "." or rel.startswith("../") or rel == ".." else rel


def excluded(path, text, self_dir=None):
    parts = path.split("/")
    if (self_dir and path.startswith(self_dir + "/")) or any(p in EXCLUDED_DIRS for p in parts[:-1]) \
            or path.endswith(EXCLUDED_SUFFIXES):
        return True
    head = text.split("\n")[:5]
    if path.endswith(".go"):
        return bool(GO_GENERATED.search("\n".join(head)))
    notes = "\n".join(l for l in head if COMMENT_LINE.match(l))
    return "@generated" in notes or bool(re.search(r"generated", notes, re.I) and re.search(r"DO NOT EDIT", notes, re.I))


def is_test_path(path):
    base = os.path.basename(path)
    return bool(base.endswith(("_test.go", "_test.py")) or re.match(r"test_.*\.py$", base) or base == "conftest.py"
                or re.search(r"\.(test|spec)\.[cm]?[jt]sx?$", base)
                or any(p in ("tests", "test", "__tests__") for p in path.split("/")[:-1]))


JS_REGEX_BEFORE = re.compile(r"(^|[(,=:\[!&|?{};+\-*%<>~^]|\b(return|typeof|case|in|of|delete|void|throw|new|yield|await))\s*$")
HEREDOC = re.compile(r"<<(-?)\s*(['\"]?)([A-Za-z_][\w-]*)\2")
CHAR_LIT = re.compile(r"'(\\.|[^'\\])'")
RS_RAW = re.compile(r'[bc]?r(#*)"')  # Rust raw 串：没有转义，以 " 加同样个数的 # 结束


def code_tail(code, k=8):
    """code 末尾直到第 k 个非空白字符为止的文字，不够 k 个就是整段。

    JS_REGEX_BEFORE 只看结尾的关键字（最长 6 个字符）和符号；每个字符都拼整行再匹配，超长单行会变成平方级。
    """
    out, seen = [], 0
    for piece in reversed(code):
        out.append(piece)
        seen += sum(1 for ch in piece if not ch.isspace())
        if seen >= k:
            break
    return "".join(reversed(out))


def tokenize(text, path):
    """返回 (lines, groups)。lines 逐行是 (code, comments, in_code)：code 里字符串内容已抹掉，comments 是 [(组号, 文字, 段号)]；
    groups 是 {组号: [逐段文字]}。

    同一条注释共用一个组号：一个块注释算一条，连续几行只有行注释的也算一条。
    in_code 为 False 的是 .vue / .html 的 template、style 段和样式表，不跑代码规则。
    """
    ext = os.path.splitext(path)[1]
    kind = lang_of(path)
    lines, groups = [], {}
    block = quote = block_gid = prev_line_gid = None
    heredocs = []  # 待读的 heredoc：[(界定符, 是否 <<- 去前导 tab)]
    escapes = True
    tmpl = []  # JS 模板串里每层 ${ 的花括号深度
    section = "template" if kind == "vue" else None

    def new_gid():
        gid = len(groups) + 1
        groups[gid] = []
        return gid

    for raw in text.split("\n"):
        if heredocs:  # shell heredoc 正文是数据，不是注释
            end, tabs = heredocs[0]
            body = raw.rstrip("\r")
            if (body.lstrip("\t") if tabs else body) == end:
                heredocs.pop(0)
            lines.append(("", [], True))
            prev_line_gid = None
            continue
        tag_line = False
        if kind == "vue" and not block and not quote and not tmpl:
            st = raw.strip()
            if section != "template" and re.search(r"</(script|style)>", st):
                section, tag_line = "template", True
            elif section == "template":
                m = re.match(r"<(script|style)(?=[\s>]|$)([^>]*)", st)
                if m and not re.search(r"</(script|style)>", st):  # 单行的 <script src=…></script> 不进脚本段
                    tag_line = True
                    if m.group(1) == "script":
                        section = "script"
                    else:
                        section = "scss" if re.search(r"\blang\s*=\s*['\"]?(scss|less|sass)\b", m.group(2)) else "style"
        if kind == "vue" and (section == "template" or tag_line):
            mode = "html"
        elif kind in ("css", "scss"):
            mode = kind
        elif section in ("style", "scss"):
            mode = "css" if section == "style" else "scss"
        else:
            mode = "hash" if kind == "hash" else "c"
        js_like = mode == "c" and (ext in JS_FAMILY or kind == "vue")
        code, comments, i, n = [], [], 0, len(raw)
        pending = []
        line_comment = None
        arith, arith_at, arith_last = False, 0, ""  # 本行代码里已出现 ((，之后的 << 是位移不是 heredoc
        while i < n:
            if block:
                j = raw.find(block, i)
                piece = raw[i:] if j < 0 else raw[i:j]
                comments.append((block_gid, piece, len(groups[block_gid])))
                groups[block_gid].append(piece)
                i, block = (n, block) if j < 0 else (j + len(block), None)
                continue
            if quote:
                if escapes and raw[i] == "\\":
                    i += 2
                elif quote == "`" and js_like and raw.startswith("${", i):
                    tmpl.append(0)
                    code.append("${")
                    i, quote = i + 2, None
                elif raw.startswith(quote, i):
                    code.append(quote)
                    i, quote = i + len(quote), None
                else:
                    i += 1
                continue
            ch = raw[i]
            if ext in SHELL and raw.startswith("<<", i) and not raw.startswith("<<<", i):
                seen = arith_last + "".join(code[arith_at:])  # 只拼上次看过之后的部分，保持线性
                arith, arith_at, arith_last = arith or "((" in seen, len(code), seen[-1:]
                m = None if arith else HEREDOC.match(raw, i)
                if m:
                    pending.append((m.group(3), m.group(1) == "-"))
                    code.append("<<" + m.group(3))
                    i += len(m.group(0))
                    continue
            if mode == "html":
                if raw.startswith("<!--", i):
                    block, block_gid, i = "-->", new_gid(), i + 4
                else:
                    code.append(ch)
                    i += 1
                continue
            if mode in ("css", "scss") and raw[i:i + 4].lower() == "url(" and (i == 0 or not (raw[i - 1].isalnum() or raw[i - 1] in "-_")):
                j = i + 4
                while j < n and raw[j] in " \t":
                    j += 1
                k = raw.find(")", j)
                if j < n and raw[j] not in "\"'" and k >= 0:  # 不带引号的 url(…) 里的 // 不是注释
                    code.append("url()")
                    i = k + 1
                    continue
            if mode in ("c", "css", "scss") and raw.startswith("/*", i):
                block, block_gid, i = "*/", new_gid(), i + 2
                continue
            if mode in ("c", "scss") and raw.startswith("//", i):
                line_comment = raw[i + 2:]
                break
            if mode == "hash" and ch == "#" and (ext == ".py" or i == 0 or raw[i - 1] in " \t;(") \
                    and not (i == 0 and raw.startswith("#!")):
                line_comment = raw[i + 1:]
                break
            before = code_tail(code) if js_like and ch == "/" else ""
            if js_like and ch == "/" and JS_REGEX_BEFORE.search(before) \
                    and not re.search(r"(\+\+|--)\s*$|<$", before):  # i++ / 2 是除法，</div> 是 JSX 闭合标签
                j, in_class = i + 1, False  # 正则字面量，里面的 // 和 /* 不是注释
                while j < n:
                    if raw[j] == "\\":
                        j += 2
                        continue
                    if raw[j] == "[":
                        in_class = True
                    elif raw[j] == "]":
                        in_class = False
                    elif raw[j] == "/" and not in_class:
                        break
                    j += 1
                if j < n:
                    code.append("/re/")
                    i = j + 1
                    continue
            if ext in TRIPLE_QUOTE and raw[i:i + 3] in ('"""', "'''"):
                quote, escapes = raw[i:i + 3], True
                code.append(quote)
                i += 3
                continue
            if ch == "'" and ext in CHAR_LITERAL:
                m = CHAR_LIT.match(raw, i)  # 字符字面量；匹配不上的是 Rust 生命周期
                code.append("''" if m else "'")
                i += len(m.group(0)) if m else 1
                continue
            if ext == ".rs" and ch in "bcr" and (i == 0 or not (raw[i - 1].isalnum() or raw[i - 1] == "_")):
                m = RS_RAW.match(raw, i)
                if m:
                    quote, escapes = '"' + m.group(1), False
                    code.append(m.group(0))
                    i += len(m.group(0))
                    continue
            # #-注释类语言里，紧跟在字母后的引号是普通文字（YAML 的 don't），Python 的 r"" f"" 前缀除外
            if ch in "\"'`" and not (ch == "`" and mode == "hash" and ext not in SHELL) \
                    and (mode != "hash" or i == 0 or not raw[i - 1].isalnum()
                                 or (ext == ".py" and re.search(r"(^|[^\w])[rRbBfFuU]{1,2}$", raw[max(0, i - 3):i]))):
                quote = ch
                escapes = not ((ch == "`" and ext == ".go") or (ch == "'" and ext in SHELL))
                code.append(ch)
                i += 1
                continue
            if tmpl and ch == "}" and tmpl[-1] == 0:  # ${ … } 结束，回到外层模板串
                tmpl.pop()
                quote, escapes = "`", True
            elif tmpl and ch in "{}":
                tmpl[-1] += 1 if ch == "{" else -1
            code.append(ch)
            i += 1
        if line_comment is not None:
            only_comment = not "".join(code).strip() and not comments
            gid = prev_line_gid if (only_comment and prev_line_gid) else new_gid()
            comments.append((gid, line_comment, len(groups[gid])))
            groups[gid].append(line_comment)
            prev_line_gid = gid if only_comment else None
        else:
            prev_line_gid = None
        # 普通引号串不跨行；模板串、Go 原始串、三引号串、shell 引号串、Rust 的 "" 串可以
        if quote in ('"', "'") and ext not in SHELL and ext not in MULTILINE_DQ:
            quote = None
        joined = "".join(code)
        heredocs += pending
        lines.append((joined, comments, mode not in ("html", "css", "scss") and not tag_line))
    return lines, groups


REF = re.compile(r"\bT-(\d{3,})\b")


class TodoIds(set):
    """有效编号；reserved 另含已划掉（~~T-003~~，已完成）的编号，用来算下一个编号，保证不复用。"""
    reserved = set()


def todo_ids(text):
    ids = TodoIds()
    for m in re.finditer(r"^\s*\|\s*(~~)?\s*T-(\d{3,})\s*(~~)?\s*\|", text or "", re.M):
        ids.reserved = ids.reserved | {m.group(2)}
        if not m.group(1):
            ids.add(m.group(2))
    return ids


def next_id(ids):
    return "T-%03d" % (max([int(i) for i in getattr(ids, "reserved", ids)] or [0]) + 1)


def has_reason(text, ids):
    """去掉分隔符后要有像样的说明（汉字算 2、字母数字算 1，至少 6），或带已登记的待办编号。照抄模板里的「理由」两字不算。"""
    text = re.sub(r"<[^<>]*>", "", text)  # 照抄报错里的 <占位符> 不算
    text = re.sub(r"^[\s#/*:\-=,]+", "", text).strip()
    if any(m.group(1) in ids for m in REF.finditer(text)):
        return True
    return sum(2 if "一" <= ch <= "鿿" else 1 for ch in text if ch.isalnum()) >= 6


EXCUSES = [
    # 词表固定：只认原词和复数，不扩到 hacky、temporarily、work-around 这类变体
    (re.compile(r"\bworkarounds?\b", re.I), "workaround"),
    (re.compile(r"\bhacks?\b", re.I), "hack"),
    (re.compile(r"\btemporary\b(?!\s+(files?|director(y|ies))\b)", re.I), "temporary"),
    (re.compile(r"\bfor\s+now\b", re.I), "for now"),
    (re.compile(r"暂时"), "暂时"),
    (re.compile(r"先这样"), "先这样"),
    (re.compile(r"临时(?!\s*(文件|目录))"), "临时"),
]
TODO_MARK = re.compile(r"\b(TODO|FIXME|XXX)\b")


def check_comment(pieces, k, ids):
    """pieces 是同一条注释的逐段文字，k 是本行那段。关键词可能被折行拆开，所以连同前后两段一起匹配，只报落在本段的。"""
    hits = []
    piece = pieces[k].strip()
    valid = any(m.group(1) in ids for m in REF.finditer(" ".join(pieces)))
    prev = pieces[k - 1].strip() + " " if k else ""
    window = prev + piece + (" " + pieces[k + 1].strip() if k + 1 < len(pieces) else "")
    lo, hi = len(prev), len(prev) + len(piece)

    def found(pat):
        return any(m.start() < hi and m.end() > lo for m in pat.finditer(window))
    nid = next_id(ids)
    for m in REF.finditer(piece):
        if m.group(1) not in ids:
            valid = True  # 已按未登记的编号报了，不再重复报关键词和 TODO
            hits.append(("todo-ref", f"T-{m.group(1)} 不在 {TODO_FILE} 里。先在清单里登记它（表格加一行"
                                     f"「| T-{m.group(1)} | 要做什么 | 登记日期 |」），或改用已登记的编号；下一个可用编号是 {nid}"))
    if not valid:
        for pat, label in EXCUSES:
            if found(pat):
                hits.append(("excuse", f"注释里的「{label}」在为捷径辩护。二选一：把捷径修掉并删掉这句；或在 {TODO_FILE} "
                                       f"登记待办（下一个编号 {nid}），在同一条注释里写上编号。换个说法绕开关键词不算解决"))
                break
        m = TODO_MARK.search(piece)
        if m:
            hits.append(("todo", f"{m.group(1)} 没带待办引用。现在做完并删掉它；做不完就在 {TODO_FILE} 登记一行"
                                 f"「| {nid} | 要做什么 | 登记日期 |」，注释写成「{m.group(1)}({nid}): …」"))
    return hits + check_suppression(piece, ids)


def check_suppression(c, ids):
    """各工具只认写在注释开头（或注释里另起一个 # / //）的指令，正文里提到指令名不算豁免。"""
    hits = []

    def need(rest, how):
        if not has_reason(rest, ids):
            hits.append(("suppress", how))

    m = re.search(r"^\s*\*?\s*eslint-disable(-next-line|-line)?\b(.*)", c)
    if m:
        rules, _, reason = m.group(2).partition("--")
        if not rules.strip():
            hits.append(("suppress", "eslint-disable 要点名规则并写理由：「eslint-disable-next-line no-console -- 理由」；能改代码就别豁免"))
        else:
            need(reason, "eslint-disable 后面用「-- 理由」写清为什么不改代码，如「eslint-disable-next-line no-console -- 理由」")
    if re.search(r"@ts-ignore\b", c):
        hits.append(("suppress", "不用 @ts-ignore：先修类型；确实修不了改用「@ts-expect-error -- 理由」，类型修好后它会报错提醒删掉"))
    m = re.search(r"@ts-(expect-error|nocheck)\b(.*)", c)
    if m:
        need(m.group(2), f"@ts-{m.group(1)} 后面写清理由：「@ts-{m.group(1)} -- 理由」")
    if re.search(r"@vue-ignore\b", c):
        hits.append(("suppress", "不用 @vue-ignore：先修模板类型；确实修不了改用「<!-- @vue-expect-error 理由 -->」，修好后它会报错提醒删掉"))
    m = re.search(r"@vue-(expect-error|skip)\b(.*)", c)
    if m:
        need(m.group(2), f"@vue-{m.group(1)} 后面写清理由：「<!-- @vue-{m.group(1)} 理由 -->」")
    m = re.search(r"(?:^\s*|//\s*)nolint\b(:[\w,-]+)?(.*)", c)
    if m:
        if not m.group(1):
            hits.append(("suppress", "nolint 要点名 linter 并写理由：「//nolint:errcheck // 理由」"))
        else:
            need(m.group(2), "nolint 后面写理由：「//nolint:errcheck // 理由」")
    m = re.search(r"(?:^\s*|//\s*)lint:(?:file-)?ignore\s+\S+(.*)", c)
    if m:
        need(m.group(1), "lint:ignore 后面写理由：「//lint:ignore SA1019 理由」")
    m = re.search(r"(?:^\s*|#)nosec\b(.*)", c)
    if m:
        rest = m.group(1)
        rest = rest.partition("--")[2] if "--" in rest else re.sub(r"^[\s,]*([A-Z]\d+[\s,]*)*", "", rest)
        need(rest, "nosec 要写理由：Go 写「#nosec G204 -- 理由」，Python 写「# nosec B602  理由」")
    m = re.search(r"(?:^\s*|#\s*)type:\s*ignore\b(\[[\w\-, ]+\])?(.*)", c)
    if m:
        if not m.group(1):
            hits.append(("suppress", "type: ignore 要带错误码并写理由：「# type: ignore[attr-defined]  # 理由」；能补类型就别豁免"))
        else:
            need(m.group(2), "type: ignore 后面再用一段注释写理由：「# type: ignore[attr-defined]  # 理由」")
    m = re.search(r"(?:^\s*|#\s*)mypy:\s*(ignore-errors|disable-error-code\s*=\s*\S+)(.*)", c)
    if m:
        need(m.group(2), "mypy 的文件级豁免要写理由：「# mypy: disable-error-code=\"attr-defined\"  # 理由」；能补类型就别豁免")
    m = re.search(r"(?:^\s*|#\s*)pyright:\s*ignore\b(\[[\w\-, ]+\])?(.*)", c)
    if m:
        need(m.group(2) if m.group(1) else "", "pyright: ignore 写成「# pyright: ignore[reportXxx]  # 理由」")
    m = re.search(r"(?:^\s*|#\s*)noqa\b(:\s*[A-Z]+\d+(?:[\s,]+[A-Z]+\d+)*)?(.*)", c, re.I)
    if m:
        if not m.group(1):
            hits.append(("suppress", "noqa 要带规则码并写理由：「# noqa: E501  # 理由」"))
        else:
            need(m.group(2), "noqa 后面再用一段注释写理由：「# noqa: E501  # 理由」")
    m = re.search(r"\bpylint:\s*disable(-next)?=\S+(.*)", c)
    if m:
        need(m.group(2), f"pylint: disable{m.group(1) or ''} 后面写理由：「# pylint: disable{m.group(1) or ''}=broad-except  # 理由」")
    m = re.search(r"\bpragma:\s*no\s*cover\b(.*)", c)
    if m:
        need(m.group(1), "pragma: no cover 后面写清为什么测不到：「# pragma: no cover  # 理由」")
    m = re.search(r"^\s*(istanbul|c8|v8)\s+ignore\s+(?!stop\b|end\b)(\w+)(.*)", c)
    if m:
        need(re.sub(r"^\s*\d+", "", m.group(3)), f"{m.group(1)} ignore 后面写清为什么测不到：「/* {m.group(1)} ignore {m.group(2)} -- 理由 */」")
    m = re.search(r"^\s*shellcheck\s+disable=\S+(.*)", c)
    if m:
        need(m.group(1), "shellcheck disable 后面写理由：「# shellcheck disable=SC2086 # 理由」")
    m = re.search(r"^\s*hadolint\s+(?:global\s+)?ignore=\S+(.*)", c)
    if m:
        need(m.group(1), "hadolint ignore 后面写理由：「# hadolint ignore=DL3008 # 理由」")
    m = re.search(r"^\s*yamllint\s+disable(?:-line|-file)?\b(.*)", c)
    if m:
        need(re.sub(r"\s*rule:\S+", "", m.group(1)), "yamllint disable 要写理由：「# yamllint disable-line rule:line-length # 理由」")
    return hits


PY_WAIVE = "；确实绕不开的，行尾写「# shortcut-ok: <理由>」"
C_WAIVE = "；确实绕不开的，行尾写「// shortcut-ok: <理由>」"
GO_FIX = ("用 _ 丢掉了 error。处理它：「if err != nil { return fmt.Errorf(\"…: %w\", err) }」；"
          "确实只能忽略的（如只读文件的 Close），行尾写「// shortcut-ok: <为什么可以忽略>」")
GO_BOOL_OK = re.compile(r"\b(strings|bytes)\.Cut(Prefix|Suffix)?\(|\.Load(OrStore|AndDelete)?\(|\bos\.LookupEnv\(|\bruntime\.Caller\(")
GO_IDENT = re.compile(r"^[\w.\[\]*]+$")


def go_blank_text(s):
    """没有类型信息时的文本判断：一行里每条语句分别看。"""
    for stmt in re.split(r"[{};]", s):
        stmt = re.sub(r"^(if|switch|for|defer|go|else)\s+", "", stmt.strip())
        if re.match(r"^_\s*=[^=]", stmt):
            return True
        stmt = re.sub(r"^var\s+(?=[\w\s,]+=)", "", stmt)  # var a, _ = f()；带类型的 var _ T = … 是接口断言，不动
        m = re.match(r"^(?!var\b|case\b)(.+?)\s*(:=|=)(?!=)\s*(.*)$", stmt)
        if not m:
            continue
        lhs = [p.strip() for p in m.group(1).split(",")]
        if len(lhs) < 2 or lhs[-1] != "_" or not all(GO_IDENT.match(p) for p in lhs):
            continue
        rhs = m.group(3).strip()
        if not (rhs.startswith(("<-", "range ")) or rhs.endswith("]") or re.search(r"\.\(\s*[\w.*\[\]]+\s*\)$", rhs)
                or GO_BOOL_OK.search(rhs)):
            return True
    return False


def following(lines, idx):
    """idx 之后第一行非空代码，以及中间跳过的注释文字。"""
    notes = []
    for code, comments, _ in lines[idx + 1:]:
        notes += [t for _, t, _ in comments]
        if code.strip():
            return code.strip(), " ".join(notes)
    return "", " ".join(notes)


def check_code(path, lines, idx, ids, go_typed, raw=""):
    """raw 是该行原文：code 里字符串内容已抹掉，判断跳过原因是不是空串要看原文。"""
    code, comments, in_code = lines[idx]
    s = code.strip()
    ext = os.path.splitext(path)[1]
    hits = []
    if not s or not in_code:
        return hits
    same_line_note = " ".join(t for _, t, _ in comments)
    # 跳过测试属于检查豁免，测试代码里照查
    empty_arg = r"\(\s*(?:(?:reason|msg)\s*=\s*)?(?:\"\"|''|``)?\s*\)"
    if ext == ".go" and re.search(r"\b[tb]\.Skip(Now|f)?\(", s) and re.search(r"\b[tb]\.Skip(Now|f)?" + empty_arg, raw):
        hits.append(("test-skip", "t.Skip() 要写原因：「t.Skip(\"需要 root 权限\")」；能修好的测试别跳过"))
    if ext == ".py" and (re.search(r"^@(unittest\.skip|pytest\.mark\.(skip|xfail))\s*$", s)
                         or (re.search(r"\b(unittest\.skip|pytest\.mark\.(skip|xfail)|pytest\.skip|skipTest)\(", s)
                             and re.search(r"\b(unittest\.skip|pytest\.mark\.(skip|xfail)|pytest\.skip|skipTest)" + empty_arg, raw))):
        hits.append(("test-skip", "跳过测试要写原因：「@unittest.skip(\"原因\")」「pytest.mark.skip(reason=\"原因\")」；能修好的别跳过"))
    if ext in JS_FAMILY and re.search(r"\b(it|test|describe)\.(skip|todo)\(|\bx(it|describe|test)\(", s):
        hits.append(("test-skip", "跳过或占位的测试要写原因：行尾写「// shortcut-ok: <原因>」，没写完的用待办编号；能修好的别跳过"))
    if is_test_path(path) or (ext == ".rs" and any("#[cfg(test)]" in l[0] for l in lines[:idx])):
        return hits  # 以下是语言捷径，测试代码不查
    if ext == ".go" and (go_blank_text(s) if go_typed is None else go_typed):
        hits.append(("go-blank", GO_FIX))
    if ext == ".py":
        m = re.match(r"^except\b([^:]*):\s*(.*)$", s)
        if m:
            caught = m.group(1).strip()
            body = m.group(2).strip() or following(lines, idx)[0]
            if not caught:
                hits.append(("py-bare-except", "裸 except 会把 KeyboardInterrupt、拼写错误一起吞掉。写明要接的异常，如"
                                               "「except (OSError, ValueError) as e:」，并处理或「raise … from e」" + PY_WAIVE))
            elif body in ("pass", "...") and re.match(r"^\(?\s*(Base)?Exception\b", caught):
                hits.append(("py-swallow", "except Exception 后直接 pass，所有错误都被吞掉。只接预期的异常类型并记录或处理；"
                                           "确实要忽略的用「contextlib.suppress(具体异常)」" + PY_WAIVE))
        if re.search(r"\bsuppress\(\s*(Base)?Exception\s*\)", s):
            hits.append(("py-swallow", "suppress(Exception) 等于吞掉所有异常。换成具体异常类型，如 suppress(FileNotFoundError)" + PY_WAIVE))
    if ext in TYPED_TS and (re.search(r"(?:[:<,|&=]|\bextends|\bkeyof)\s*any\b(?!\s*[.(\w])", s) or re.search(r"\bas\s+any\b", s)
                            or re.search(r"\bany\s*(\[\]|[|&>,)])", s) or re.search(r"\bas\s+unknown\s+as\b", s)):
        hits.append(("ts-any", "用 any / as unknown as 绕开了类型检查。写出真实类型或 interface；来源不确定就用 unknown，"
                               "再用 typeof / in / 类型守卫收窄" + C_WAIVE))
    if ext in JS_FAMILY:
        empty, note = False, same_line_note
        if re.search(r"\bcatch\s*(\([^)]*\))?\s*\{\s*\}", s) or \
                re.search(r"\.catch\(\s*(\(\s*\w*\s*\)|\w+)\s*=>\s*(\{\s*\}|undefined|null)\s*\)", s):
            empty = True
        elif re.search(r"\bcatch\s*(\([^)]*\))?\s*\{$", s):
            nxt, between = following(lines, idx)
            empty, note = nxt.startswith("}"), same_line_note + " " + between
        if empty and not has_reason(note, ids):  # 同 ESLint no-empty：catch 块里写了理由注释就不算空
            hits.append(("js-swallow", "空 catch 把错误吞掉了。至少记录或提示用户（console.error / ElMessage.error）；"
                                       "确实可忽略的，在 catch 块里写注释说明为什么，如「catch { /* <为什么可以忽略> */ }」"))
    if ext == ".rs" and re.search(r"\.unwrap\(\)", s):
        hits.append(("rs-unwrap", "unwrap() 失败就 panic。用 ? 向上传错误，或 .expect(\"为什么这里不可能失败\")" + C_WAIVE))
    return hits


WAIVER = re.compile(r"shortcut-ok:(.*)")


GO_CANDIDATE = re.compile(r"(^|[\s,({;])_\s*(,|:?=)")


def go_typed_blanks(paths, read, top):
    """用 goblank 按类型找丢掉 error 的行。返回 {path: set(行号)}，只含做过类型检查的文件。

    类型信息来自工作区，所以只用于工作区内容与要检查的内容一致的文件；其余回退到文本规则。
    """
    by_mod, result, fallback = {}, {}, []
    for p in paths:
        full = os.path.join(top, p)
        try:
            with open(full, encoding="utf-8", errors="replace") as f:
                same = f.read() == read(p)
        except OSError:
            same = False
        d = os.path.dirname(full)
        while not os.path.exists(os.path.join(d, "go.mod")) and d.startswith(top + os.sep):
            d = os.path.dirname(d)
        if same and os.path.exists(os.path.join(d, "go.mod")):
            by_mod.setdefault(d, []).append(full)
        else:
            fallback.append(p)
    for mod, files in sorted(by_mod.items()):
        rels = [os.path.relpath(f, top).replace(os.sep, "/") for f in files]
        try:
            r = subprocess.run(["go", "run", os.path.join(HERE, "goblank", "main.go"), mod] + files,
                               cwd=mod, capture_output=True, text=True, timeout=600)
            failed, why = r.returncode != 0, r.stderr.strip()[-300:]
        except (OSError, subprocess.TimeoutExpired) as e:
            failed, why, r = True, str(e), None
        if failed:
            print(f"（{os.path.relpath(mod, top)} 的 Go 类型检查没跑成：{why}）", file=sys.stderr)
            fallback += rels
            continue
        for line in r.stdout.splitlines():
            kind, _, rest = line.partition(" ")
            f, n = rest.rsplit(":", 1) if kind == "blank" else (rest, "")
            rel = os.path.relpath(os.path.realpath(f), top).replace(os.sep, "/")
            result.setdefault(rel, set())
            if n:
                result[rel].add(int(n))
        fallback += [p for p in rels if p not in result]
    if fallback:
        print("（这些 Go 文件没做类型检查（部分暂存、构建标签排除或编译不过），「用 _ 丢掉 error」按文本判断，可能误报或漏报："
              + "、".join(sorted(set(fallback))) + "）", file=sys.stderr)
    return result


def git(*args):
    return subprocess.run(("git", "-c", "core.quotePath=false") + args, check=True, capture_output=True).stdout


def unquote_path(name):
    """git 对含引号、反斜杠、控制字符的路径加 C 风格引号。"""
    if not (name.startswith('"') and name.endswith('"')):
        return name
    raw = name[1:-1].encode("utf-8")
    return raw.decode("unicode_escape").encode("latin-1").decode("utf-8", "replace")


def added_lines(*diff_args):
    """{path: {行号: 原文}}。前缀写死，不受用户 diff.noprefix / diff.mnemonicPrefix 配置影响。"""
    diff = git("diff", "--no-color", "--no-ext-diff", "--src-prefix=a/", "--dst-prefix=b/", "-U0", "-M",
               *diff_args).decode("utf-8", "replace")
    added, path, ln, header = {}, None, 0, False
    for line in diff.split("\n"):
        if line.startswith("diff --git "):
            header, path = True, None
            continue
        if header:
            if line.startswith("+++ "):
                name = unquote_path(line[4:].rstrip("\t"))  # 带空格的文件名后面会多一个 \t
                path = name[2:] if name.startswith("b/") else None
                continue
            if line.startswith("@@"):
                header = False
            else:
                continue
        m = re.match(r"@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@", line)
        if m:
            ln = int(m.group(1))
        elif line.startswith("+") and path:
            added.setdefault(path, {})[ln] = line[1:]
            ln += 1
    return added


def scan(added, read, top):
    ids = todo_ids(read(TODO_FILE))
    problems, targets = [], {}
    self_dir = guard_dir(top)
    for path in sorted(added):
        if not lang_of(path):
            continue
        text = read(path)
        if text is not None and not excluded(path, text, self_dir):
            targets[path] = text
    go_paths = [p for p in targets if p.endswith(".go") and not is_test_path(p)
                and any(GO_CANDIDATE.search(l) for l in added[p].values())]
    typed = go_typed_blanks(go_paths, read, top) if go_paths else {}
    for path, text in targets.items():
        lines, groups = tokenize(text, path)
        for ln, raw in sorted(added[path].items()):
            if ln > len(lines):
                continue
            idx = ln - 1
            hits = []
            for gid, _, k in lines[idx][1]:
                hits += check_comment(groups[gid], k, ids)
            go_typed = (ln in typed[path]) if path in typed else None
            code_hits = check_code(path, lines, idx, ids, go_typed, raw)
            if code_hits:
                # 豁免写在同一行，或紧挨着的上一行（只有注释的行）
                here = " ".join(t for _, t, _ in lines[idx][1])
                above = " ".join(t for _, t, _ in lines[idx - 1][1]) if idx and not lines[idx - 1][0].strip() else ""
                waiver = WAIVER.search(here) or WAIVER.search(above)
                if not waiver:
                    hits += code_hits
                elif not has_reason(waiver.group(1), ids):
                    hits.append(("waiver", "shortcut-ok: 后面要写清为什么这里只能这样，或写已登记的待办编号"))
            problems += [(path, ln, rule, msg, raw.strip()) for rule, msg in hits]
    return problems


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--range", help="检查一段提交的新增行，写成 <基点>..<终点>（如 $(git merge-base origin/main HEAD)..HEAD）"
                                    "或 <分支>...<终点>；缺省检查暂存区")
    args = ap.parse_args()
    if args.range:
        if ".." not in args.range:
            ap.error("--range 要写成 <基点>..<终点> 或 <分支>...<终点>")
        three = "..." in args.range
        base, rev = args.range.split("..." if three else "..", 1)
        base, rev = base or "HEAD", rev or "HEAD"
        if three:
            base = git("merge-base", base, rev).decode().strip()
        added = added_lines(base, rev)

        def read(p):
            try:
                return git("show", f"{rev}:{p}").decode("utf-8", "replace")
            except subprocess.CalledProcessError:
                return None
    else:
        try:
            git("rev-parse", "--verify", "-q", "HEAD")
            base = "HEAD"
        except subprocess.CalledProcessError:
            base = git("hash-object", "-t", "tree", os.devnull).decode().strip()
        added = added_lines("--cached", base)
        # 合并提交只查两边都没有的行（解决冲突时新写的），对方分支带来的行不算新增
        try:
            merge_head = git("rev-parse", "-q", "--verify", "MERGE_HEAD").decode().strip()
        except subprocess.CalledProcessError:
            merge_head = None
        if merge_head:
            other = added_lines("--cached", merge_head)
            added = {p: {ln: t for ln, t in ls.items() if ln in other.get(p, {})} for p, ls in added.items()}

        def read(p):
            try:
                return git("show", f":{p}").decode("utf-8", "replace")
            except subprocess.CalledProcessError:
                return None
    read = functools.lru_cache(maxsize=None)(read)
    top = os.path.realpath(git("rev-parse", "--show-toplevel").decode().strip())
    problems = scan(added, read, top)
    if not problems:
        return 0
    for path, ln, rule, msg, raw in problems:
        print(f"{path}:{ln}: [{rule}] {msg}\n    > {raw[:160]}", file=sys.stderr)
    print(f"\n捷径检查没通过：{len(problems)} 处（约定见 docs/shortcut-guard.md）。按每条提示把捷径修掉；"
          f"修不了的在 {TODO_FILE} 登记待办并在注释里写上编号，或按提示的写法附上理由。", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())

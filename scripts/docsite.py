#!/usr/bin/env python3
"""Render the repository's markdown into a static documentation site.

    python3 scripts/docsite.py SITE_DIR
    python3 scripts/docsite.py SITE_DIR --wasm path/to/wlel.wasm

The compiler has no dependencies, and neither does its documentation: this
uses only the Python standard library. The markdown subset is the one the
repository actually writes — headings, fenced code, lists, tables, block
quotes, inline code, bold/italic, links and images, raw HTML passthrough —
which is why it is ~400 lines instead of a pandoc dependency.

`scripts/docsite.py --check` renders every page and fails if a page would
contain an unrendered fenced block or an unresolved link target, which is what
CI runs.
"""

from __future__ import annotations

import html
import os
import re
import shutil
import sys
from dataclasses import dataclass, field

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = "https://github.com/WildanDeveloper/wlel"

# ---------------------------------------------------------------------------
# inline markdown
# ---------------------------------------------------------------------------

INLINE_CODE = re.compile(r"(`+)(.+?)\1", re.S)
IMAGE = re.compile(r"!\[([^\]]*)\]\(([^)\s]+)(?:\s+\"([^\"]*)\")?\)")
LINK = re.compile(r"\[([^\]]+)\]\(([^)\s]+)(?:\s+\"([^\"]*)\")?\)")
BOLD = re.compile(r"\*\*(.+?)\*\*", re.S)
ITALIC = re.compile(r"(?<![\*\w])\*(?!\*)([^*\n]+?)\*(?![\*\w])")
STRIKE = re.compile(r"~~(.+?)~~", re.S)
AUTOLINK = re.compile(r"<(https?://[^>\s]+)>")
BR = re.compile(r"  \n")


def _link_target(href: str) -> str:
    """Rewrite repository-relative targets for the generated site."""
    if href.startswith(("#", "http://", "https://", "mailto:")):
        return href
    if href.startswith("assets/"):
        return "../" + href
    if href in PAGE_OF:
        return PAGE_OF[href]
    # relative directory links used by editors/README.md
    if href in ("vscode-wlel", "tree-sitter-wlel", "linguist"):
        return {"vscode-wlel": "vscode.html", "tree-sitter-wlel": "tree-sitter.html"}.get(
            href, "linguist.html"
        )
    if href.startswith("docs/") and href.endswith(".md"):
        name = os.path.basename(href)[:-3]
        return f"{name}.html"
    # everything else is a file in the repository (LICENSE, install.sh, ...)
    return f"{REPO}/blob/main/{href.lstrip('./')}"


def inline(text: str) -> str:
    """Inline markdown -> HTML.

    Code spans are stashed as placeholders first, so `[`a`](b)` (a link whose
    label is code) still parses as one link, and `**x**` inside backticks
    stays literal.
    """
    codes: list[str] = []

    def stash_code(m):
        codes.append(m.group(2))
        return f"\x01{len(codes) - 1}\x01"

    text = INLINE_CODE.sub(stash_code, text)
    out = _inline_text(text)

    def unstash(m):
        return "<code>" + html.escape(codes[int(m.group(1))].strip()) + "</code>"

    return re.sub(r"\x01(\d+)\x01", unstash, out)


def _inline_text(text: str) -> str:
    if not text:
        return ""
    # protect raw HTML spans (the README has a <div align="center"> block)
    html_spans = []

    def stash(m):
        html_spans.append(m.group(0))
        return f"\x00{len(html_spans) - 1}\x00"

    # a real tag is `<name>`, `<name attr>`, `</name>`; this deliberately does
    # not match `<https://...>` (an autolink) or `<T>` in prose
    text = re.sub(r"</?[a-zA-Z][a-zA-Z0-9-]*(?:\s[^>]*)?/?>", stash, text)
    text = html.escape(text, quote=False)
    text = AUTOLINK.sub(lambda m: f'<a href="{m.group(1)}">{m.group(1)}</a>', text)
    text = IMAGE.sub(
        lambda m: '<img src="{}" alt="{}"{}>'.format(
            _link_target(m.group(2)),
            html.escape(m.group(1)),
            f' title="{html.escape(m.group(3))}"' if m.group(3) else "",
        ),
        text,
    )
    text = LINK.sub(
        lambda m: '<a href="{}"{}>{}</a>'.format(
            _link_target(m.group(2)),
            f' title="{html.escape(m.group(3))}"' if m.group(3) else "",
            m.group(1),
        ),
        text,
    )
    text = BOLD.sub(r"<strong>\1</strong>", text)
    text = STRIKE.sub(r"<del>\1</del>", text)
    text = ITALIC.sub(r"<em>\1</em>", text)
    text = BR.sub("<br>\n", text)

    def unstash(m):
        return html_spans[int(m.group(1))]

    return re.sub(r"\x00(\d+)\x00", unstash, text)


# ---------------------------------------------------------------------------
# block markdown
# ---------------------------------------------------------------------------

HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*#*\s*$")
FENCE = re.compile(r"^```([A-Za-z0-9_+-]*)\s*$")
HR = re.compile(r"^(?:---+|\*\*\*+|___+)\s*$")
UL = re.compile(r"^(\s*)[-*+]\s+(.*)$")
OL = re.compile(r"^(\s*)\d+\.\s+(.*)$")
TASK = re.compile(r"^\[([ xX])\]\s+(.*)$")
QUOTE = re.compile(r"^>\s?(.*)$")
TABLE_SEP = re.compile(r"^\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)+\|?\s*$")


def slug(text: str) -> str:
    text = re.sub(r"`([^`]*)`", r"\1", text)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"[^\w\s-]", "", text.lower())
    return re.sub(r"[\s_]+", "-", text).strip("-") or "section"


@dataclass
class Toc:
    entries: list[tuple[int, str, str]] = field(default_factory=list)


def render(text: str, toc: Toc | None = None, page: str = "") -> str:
    lines = text.replace("\r\n", "\n").split("\n")
    out: list[str] = []
    i = 0
    while i < len(lines):
        line = lines[i]

        # fenced code
        m = FENCE.match(line)
        if m:
            lang = m.group(1)
            i += 1
            body = []
            while i < len(lines) and not FENCE.match(lines[i]):
                body.append(lines[i])
                i += 1
            i += 1  # closing fence
            cls = f' class="language-{lang}"' if lang else ""
            out.append(f"<pre><code{cls}>" + html.escape("\n".join(body)) + "</code></pre>")
            continue

        # headings
        m = HEADING.match(line)
        if m:
            level = len(m.group(1))
            title = m.group(2)
            ident = slug(title)
            if level == 1 and page == "index":
                # the landing page provides its own hero
                out.append(f'<h1 id="{ident}">{inline(title)}</h1>')
            else:
                out.append(f'<h{level} id="{ident}">{inline(title)}</h{level}>')
            if toc is not None and 2 <= level <= 3:
                toc.entries.append((level, ident, re.sub(r"[`*]", "", title)))
            i += 1
            continue

        if HR.match(line):
            out.append("<hr>")
            i += 1
            continue

        # raw HTML block (README hero, details/summary in editors docs)
        if line.lstrip().startswith("<") and not line.lstrip().startswith("<http"):
            block = []
            while i < len(lines) and lines[i].strip():
                block.append(lines[i])
                i += 1
            out.append("\n".join(block))
            continue

        # tables
        if line.strip().startswith("|") and i + 1 < len(lines) and TABLE_SEP.match(lines[i + 1]):
            header_cells = split_row(line)
            i += 2
            rows = []
            while i < len(lines) and lines[i].strip().startswith("|"):
                rows.append(split_row(lines[i]))
                i += 1
            out.append("<table>")
            out.append("<thead><tr>" + "".join(f"<th>{inline(c)}</th>" for c in header_cells) + "</tr></thead>")
            out.append("<tbody>")
            for row in rows:
                out.append("<tr>" + "".join(f"<td>{inline(c)}</td>" for c in row) + "</tr>")
            out.append("</tbody></table>")
            continue

        # block quote
        if QUOTE.match(line):
            body = []
            while i < len(lines) and (QUOTE.match(lines[i]) or not lines[i].strip()):
                m2 = QUOTE.match(lines[i])
                body.append(m2.group(1) if m2 else "")
                i += 1
            out.append("<blockquote>" + render("\n".join(body), None, page) + "</blockquote>")
            continue

        # lists
        if UL.match(line) or OL.match(line):
            block, i = collect_list(lines, i)
            out.append(render_list(block, page))
            continue

        # paragraph
        if not line.strip():
            i += 1
            continue
        para = []
        while i < len(lines) and lines[i].strip() and not starts_block(lines, i):
            para.append(lines[i])
            i += 1
        out.append("<p>" + inline("\n".join(para).strip()) + "</p>")

    return "\n".join(out)


def starts_block(lines: list[str], i: int) -> bool:
    line = lines[i]
    if FENCE.match(line) or HEADING.match(line) or HR.match(line):
        return True
    if QUOTE.match(line) or UL.match(line) or OL.match(line):
        return True
    if line.strip().startswith("|") and i + 1 < len(lines) and TABLE_SEP.match(lines[i + 1]):
        return True
    if line.lstrip().startswith("<") and not line.lstrip().startswith("<http"):
        return True
    return False


def split_row(line: str) -> list[str]:
    line = line.strip()
    if line.startswith("|"):
        line = line[1:]
    if line.endswith("|"):
        line = line[:-1]
    cells = re.split(r"(?<!\\)\|", line)
    return [c.strip().replace("\\|", "|") for c in cells]


def collect_list(lines: list[str], i: int) -> tuple[list[str], int]:
    block = []
    while i < len(lines):
        line = lines[i]
        if not line.strip():
            # a blank line continues the list only if the next line is indented
            if i + 1 < len(lines) and lines[i + 1][:1] in (" ", "\t"):
                block.append("")
                i += 1
                continue
            break
        if UL.match(line) or OL.match(line) or line[:1] in (" ", "\t"):
            block.append(line)
            i += 1
            continue
        break
    return block, i


def render_list(block: list[str], page: str) -> str:
    """Render one list block, recursing into indented sub-lists."""
    html_text, _ = _render_items(block, 0, _indent_of(block, 0))
    return html_text


def _indent_of(lines: list[str], i: int) -> int:
    m = UL.match(lines[i]) or OL.match(lines[i])
    return len(m.group(1).expandtabs(4)) if m else 0


def _render_items(lines: list[str], idx: int, indent: int) -> tuple[str, int]:
    ordered = None
    items: list[str] = []

    while idx < len(lines):
        line = lines[idx]
        if not line.strip():
            idx += 1
            continue
        m_ul, m_ol = UL.match(line), OL.match(line)
        m = m_ul or m_ol
        if not m:
            break
        current = len(m.group(1).expandtabs(4))
        if current < indent:
            break
        if current > indent:
            # an over-indented line with no parent item: treat as text
            idx += 1
            continue

        is_ordered = bool(m_ol)
        if ordered is None:
            ordered = is_ordered

        body = m.group(2)
        idx += 1

        # gather this item's continuation lines and any nested list
        children: list[str] = []
        while idx < len(lines):
            nxt = lines[idx]
            if not nxt.strip():
                # blank line: belongs to the item only if indented content follows
                j = idx
                while j < len(lines) and not lines[j].strip():
                    j += 1
                if j < len(lines) and (UL.match(lines[j]) or OL.match(lines[j])):
                    if _indent_of(lines, j) > indent:
                        children.append("")
                        idx += 1
                        continue
                break
            mm = UL.match(nxt) or OL.match(nxt)
            if mm:
                if len(mm.group(1).expandtabs(4)) > indent:
                    children.append(nxt)
                    idx += 1
                    continue
                break
            if nxt[:1] in (" ", "\t"):
                children.append(nxt)
                idx += 1
                continue
            break

        task = TASK.match(body)
        if task:
            checked = task.group(1).lower() == "x"
            body = f'<input type="checkbox" disabled{" checked" if checked else ""}> ' + task.group(2)

        rendered = inline(body)

        if children:
            cont: list[str] = []
            nested: list[str] = []
            for c in children:
                if UL.match(c) or OL.match(c):
                    nested.append(c)
                elif c.strip():
                    cont.append(c.strip())
            if cont:
                rendered += " " + inline(" ".join(cont))
            if nested:
                sub_indent = _indent_of(nested, 0)
                sub, _ = _render_items(nested, 0, sub_indent)
                rendered += sub

        items.append("<li>" + rendered + "</li>")

    tag = "ol" if ordered else "ul"
    return f"<{tag}>{''.join(items)}</{tag}>", idx


# ---------------------------------------------------------------------------
# pages
# ---------------------------------------------------------------------------

PAGE_OF = {
    "README.md": "index.html",
    "docs/spec.md": "spec.html",
    "docs/cross-compile.md": "cross-compile.html",
    "CHANGELOG.md": "changelog.html",
    "CONTRIBUTING.md": "contributing.html",
    "SECURITY.md": "security.html",
    "editors/README.md": "editors.html",
    "editors/vscode-wlel/README.md": "vscode.html",
    "editors/tree-sitter-wlel/README.md": "tree-sitter.html",
    "editors/linguist/README.md": "linguist.html",
    "install.sh": REPO + "/blob/main/install.sh",
}

NAV = [
    ("index.html", "Home"),
    ("spec.html", "Spec"),
    ("editors.html", "Editors"),
    ("playground/index.html", "Playground"),
    ("changelog.html", "Changelog"),
]

TEMPLATE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{description}">
<link rel="icon" href="{root}assets/logo.png">
<link rel="stylesheet" href="{root}style.css">
</head>
<body>
<header>
  <a class="brand" href="{root}index.html">
    <img src="{root}assets/logo.png" alt="" width="28" height="28">
    <span>Wlel</span>
  </a>
  <nav>
    {nav}
    <a href="{repo}">GitHub</a>
  </nav>
</header>
<main>
{body}
</main>
<footer>
  <span>Apache-2.0 · <a href="{repo}/blob/main/LICENSE">license</a></span>
  <a href="{repo}">WildanDeveloper/wlel</a>
</footer>
</body>
</html>
"""


def page(title: str, body: str, out: str, root: str = "", current: str = "") -> None:
    nav = "\n    ".join(
        f'<a href="{root}{href}"{" class=\"current\"" if href == current else ""}>{label}</a>'
        for href, label in NAV
    )
    document_title = "Wlel — the arena-first systems language" if title == "Wlel" else f"{title} — Wlel"
    html_text = TEMPLATE.format(
        title=html.escape(document_title),
        description=html.escape(
            "Wlel — the arena-first systems programming language. Docs, the frozen spec, and a browser playground."
        ),
        root=root,
        nav=nav,
        body=body,
        repo=REPO,
    )
    with open(out, "w", encoding="utf-8") as fh:
        fh.write(html_text)


def render_page(source: str, heading: str, out: str, root: str, current: str, sidebar: bool) -> None:
    text = open(os.path.join(ROOT, source), encoding="utf-8").read()
    toc = Toc()
    body = render(text, toc, "index" if source == "README.md" else "")
    if sidebar and toc.entries:
        links = "".join(
            f'<li class="lvl-{level}"><a href="#{ident}">{html.escape(name)}</a></li>'
            for level, ident, name in toc.entries
        )
        body = (
            '<div class="docs-layout"><nav class="docs-nav"><ol>'
            + links
            + '</ol></nav><article class="docs-body">'
            + body
            + "</article></div>"
        )
    page(heading, body, out, root=root, current=current)


def build(out_dir: str, wasm: str | None) -> None:
    os.makedirs(out_dir, exist_ok=True)

    assets = os.path.join(out_dir, "assets")
    os.makedirs(assets, exist_ok=True)
    shutil.copy2(os.path.join(ROOT, "assets/logo.png"), os.path.join(assets, "logo.png"))

    pages = [
        ("README.md", "Wlel", "index.html", True, False),
        ("docs/spec.md", "Language specification", "spec.html", True, True),
        ("editors/README.md", "Editor support", "editors.html", True, True),
        ("editors/vscode-wlel/README.md", "VS Code", "vscode.html", True, True),
        ("editors/tree-sitter-wlel/README.md", "tree-sitter", "tree-sitter.html", True, True),
        ("editors/linguist/README.md", "GitHub Linguist", "linguist.html", True, True),
        ("docs/cross-compile.md", "Cross-compiling", "cross-compile.html", True, True),
        ("CHANGELOG.md", "Changelog", "changelog.html", True, True),
        ("CONTRIBUTING.md", "Contributing", "contributing.html", True, True),
        ("SECURITY.md", "Security", "security.html", True, True),
    ]
    for source, title, name, root_slash, sidebar in pages:
        render_page(source, title, os.path.join(out_dir, name), "", name, sidebar)

    playground = os.path.join(out_dir, "playground")
    os.makedirs(playground, exist_ok=True)
    for name in ("index.html", "app.js", "style.css"):
        shutil.copy2(os.path.join(ROOT, "playground", name), os.path.join(playground, name))
    if wasm and os.path.isfile(wasm):
        shutil.copy2(wasm, os.path.join(playground, "wlel.wasm"))
    shutil.copy2(os.path.join(ROOT, "playground/style.css"), os.path.join(out_dir, "style.css"))

    # the landing page starts with the repository README's hero
    index = os.path.join(out_dir, "index.html")
    text = open(index, encoding="utf-8").read()
    text = text.replace("<main>", '<main><div class="landing">', 1).replace("</main>", "</div></main>", 1)
    with open(index, "w", encoding="utf-8") as fh:
        fh.write(text)


def check() -> int:
    """Render every page and look for signs of an unrendered document."""
    failures = []
    for source, _title, _name, _root, _sidebar in [
        ("README.md", "", "", "", False),
        ("docs/spec.md", "", "", "", True),
        ("CHANGELOG.md", "", "", "", True),
        ("docs/cross-compile.md", "", "", "", True),
        ("CONTRIBUTING.md", "", "", "", True),
        ("SECURITY.md", "", "", "", True),
        ("editors/README.md", "", "", "", True),
        ("editors/vscode-wlel/README.md", "", "", "", True),
        ("editors/tree-sitter-wlel/README.md", "", "", "", True),
        ("editors/linguist/README.md", "", "", "", True),
    ]:
        text = open(os.path.join(ROOT, source), encoding="utf-8").read()
        body = render(text)
        if "```" in body:
            failures.append(f"{source}: an unrendered fenced block survived")
        # a markdown link that should have become an anchor; code blocks are
        # excluded because `](x: T)` there is source code, not a link
        prose = re.sub(r"<pre>.*?</pre>", "", body, flags=re.S)
        prose = re.sub(r"<code>.*?</code>", "", prose, flags=re.S)
        leftovers = re.findall(r"\]\((?!https?:|#|mailto:)[^)]*\)", prose)
        if leftovers:
            failures.append(f"{source}: unrendered link(s): {leftovers[:3]}")
    if failures:
        for f in failures:
            print("docsite: " + f, file=sys.stderr)
        return 1
    print("docsite: all pages render")
    return 0


def main(argv: list[str]) -> int:
    if len(argv) >= 2 and argv[1] == "--check":
        return check()
    if len(argv) < 2:
        print("usage: docsite.py OUT_DIR [--wasm FILE]", file=sys.stderr)
        return 2
    wasm = None
    if "--wasm" in argv:
        wasm = argv[argv.index("--wasm") + 1]
    build(argv[1], wasm)
    print(f"docsite: wrote {argv[1]}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

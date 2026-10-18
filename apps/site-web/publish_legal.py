#!/usr/bin/env python3
"""Convert docs/privacy-policy.md and docs/terms.md into pages/privacy.html and
pages/terms.html. Fix: blank line before every markdown list so bullets become
real <ul>/<ol>, and keep a space on each side of inline <code> when it sits next
to a letter or digit (avoids "Thefl_session").

Usage (from repo root or this directory):
  python3 apps/site-web/publish_legal.py
  python3 apps/site-web/publish_legal.py --privacy path/to/v9.md --terms path/to/v6.md

Needs: pip install markdown  (uses the sane_lists extension).
Then rebuild the site: python3 apps/site-web/build.py
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

try:
    import markdown
except ImportError:
    sys.exit("Missing dependency: pip install markdown")

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent  # repo root
DEFAULT_PRIVACY = ROOT / "docs" / "privacy-policy.md"
DEFAULT_TERMS = ROOT / "docs" / "terms.md"
OUT_PRIVACY = HERE / "pages" / "privacy.html"
OUT_TERMS = HERE / "pages" / "terms.html"

# Intro line immediately followed by a list item — markdown needs a blank line.
_LIST_GAP = re.compile(r"(?m)^((?![*+-] |\d+\. )[^\n]*\S)\n(?=[*+-] |\d+\. )")


def prepare(md: str) -> str:
    s = md
    s = re.sub(r"\A# .*\n\n", "", s)  # page supplies its own h1
    s = re.sub(r"\A\*DRAFT[^\n]*\*\n\n", "", s)
    s = s.replace("Lucrona [OPERATOR NAME]", "Lucrona")
    s = re.sub(r" ?\[CONFIRM once Coder ships[^\]]*\]", "", s)
    s = re.sub(r"\ALast updated: \[DATE\]\n\n", "", s)
    s = re.sub(r"(?m)^Last updated:? .+\n\n", "", s)
    s = s.replace("[DATE]", "19 October 2026")
    # Ensure a blank line before every list so python-markdown emits <ul>/<ol>
    # instead of leaving "- item" inside a <p> (which browsers show as run-ons).
    prev = None
    while prev != s:
        prev = s
        s = _LIST_GAP.sub(r"\1\n\n", s)
    return s


def ensure_code_spaces(html: str) -> str:
    """Keep a surrounding space when <code>…</code> sits flush against a word char."""
    html = re.sub(r"(?<=\w)<code>", " <code>", html)
    html = re.sub(r"</code>(?=\w)", "</code> ", html)
    return html


def md_to_body(md: str) -> str:
    body = markdown.markdown(prepare(md), extensions=["sane_lists", "fenced_code"])
    body = ensure_code_spaces(body)
    body = re.sub(
        r'(?<![\w/@">])support@playfraglink\.com',
        '<a href="mailto:support@playfraglink.com">support@playfraglink.com</a>',
        body,
    )
    body = re.sub(
        r'<strong><a (href="mailto:[^"]+")>([^<]+)</a></strong>',
        r'<a \1><strong>\2</strong></a>',
        body,
    )
    return body


def wrap(title: str, page: str, h1: str, body: str) -> str:
    out = (
        f"<!-- title: {title} · Fraglink | page: {page} -->\n"
        '<div class="mid wrap"><section class="panel"><div class="body prose" style="padding:20px 24px">\n'
        f'<h1>{h1}</h1>\n<p class="meta">Last updated 19 October 2026</p>\n'
        f"{body}\n"
        "</div></section></div>\n"
    )
    # No leftover draft markers in visible text.
    visible = re.sub(r"<[^>]+>", "", out.split("-->", 1)[1])
    for bad in ["[", "]", "DRAFT", "CONFIRM", "OPERATOR", "Coder", "Private preview"]:
        if bad in visible:
            raise SystemExit(f"refusing to write {page}: leftover marker {bad!r}")
    # Every markdown-style dash list must have been converted.
    if re.search(r"<p>[^<]*(?:</strong>)?\n- ", out) or re.search(
        r"</strong>\n- ", out
    ):
        raise SystemExit(
            f"refusing to write {page}: list still inside a <p> (blank-line fix failed)"
        )
    return out


def convert(src: Path, title: str, page: str, h1: str, dest: Path) -> None:
    body = md_to_body(src.read_text(encoding="utf-8"))
    dest.write_text(wrap(title, page, h1, body), encoding="utf-8")
    print(f"wrote {dest.relative_to(ROOT)} from {src}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--privacy", type=Path, default=DEFAULT_PRIVACY)
    ap.add_argument("--terms", type=Path, default=DEFAULT_TERMS)
    args = ap.parse_args()
    convert(args.privacy, "Privacy Policy", "privacy", "Privacy Policy", OUT_PRIVACY)
    convert(args.terms, "Terms of Use", "terms", "Terms of Use", OUT_TERMS)


if __name__ == "__main__":
    main()

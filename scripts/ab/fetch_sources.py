#!/usr/bin/env python3
"""抓社区书源合集，合并成一份书源列表（M6「全量真机基准重测」的第一步）。

    python3 scripts/ab/fetch_sources.py [--out target/legado-ab-full/sources.json]

做法：解析 <https://yuedu.miaogongzi.net/gx.html> 里
`yuedu://booksource/importonline?src=<URL>` 的下载地址，逐个拉下来（有 gzip 的）、把每个文件里的
书源挨个合进一个数组，写成 `sources.json`（`target/` 不进版本库，随时可重跑）。

这一步只依赖书源合集本身，不碰任何书源站点；下一页的 `scripts/ab/gen_cases.py`
（用我们自己的 URL 层）再去打真实站点生成对拍请求行。
"""

from __future__ import annotations

import argparse
import gzip
import json
import re
import sys
import urllib.request

LIST_PAGE = "https://yuedu.miaogongzi.net/gx.html"
UA = "Mozilla/5.0 (legado-rust-engine ab)"


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(request, timeout=60) as response:
        body = response.read()
    if body[:2] == b"\x1f\x8b":
        return gzip.decompress(body)
    return body


def collect_urls(page: str) -> list[str]:
    urls: list[str] = []
    for match in re.finditer(r"importonline\?src=([^\"'&]+)", page):
        url = match.group(1)
        if url.startswith("//"):
            url = "https:" + url
        if url not in urls:
            urls.append(url)
    return urls


def sources_of(payload) -> list[dict]:
    """一个合集文件里的书源：数组、`{"bookSources": [...]}`、`{"data": [...]}` 都认"""
    if isinstance(payload, list):
        return [item for item in payload if isinstance(item, dict)]
    if isinstance(payload, dict):
        for key in ("bookSources", "bookSourceList", "data", "items"):
            value = payload.get(key)
            if isinstance(value, list):
                return [item for item in value if isinstance(item, dict)]
        if "bookSourceUrl" in payload:
            return [payload]
    return []


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="target/legado-ab-full/sources.json")
    parser.add_argument("--page", default=LIST_PAGE)
    args = parser.parse_args()

    page = fetch(args.page).decode("utf-8", "replace")
    urls = collect_urls(page)
    if not urls:
        print(f"❌ {args.page} 里没找到 importonline 链接（页面改版了？）", file=sys.stderr)
        return 1
    print(f"合集 {len(urls)} 个")

    merged: list[dict] = []
    seen: set[str] = set()
    failed = 0
    for url in urls:
        try:
            payload = json.loads(fetch(url).decode("utf-8-sig", "replace"))
        except Exception as error:
            failed += 1
            print(f"  ✗ {url}: {type(error).__name__} {error}")
            continue
        sources = sources_of(payload)
        added = 0
        for source in sources:
            key = source.get("bookSourceUrl")
            if not isinstance(key, str) or not key or key in seen:
                continue
            seen.add(key)
            merged.append(source)
            added += 1
        print(f"  ✓ {url.rsplit('/', 1)[-1][:48]}: {len(sources)} 个，新增 {added}")

    import os

    os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(merged, handle, ensure_ascii=False, indent=1)
    print(f"合计去重后 {len(merged)} 个书源 → {args.out}（合集失败 {failed} 个）")
    return 0 if merged else 1


if __name__ == "__main__":
    raise SystemExit(main())

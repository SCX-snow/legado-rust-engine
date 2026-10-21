#!/usr/bin/env python3
"""按 Conventional Commits 计算下一个版本号，并按需改写仓库内的版本引用。

用法（在仓库根目录执行，默认只计算不落盘）:
    python3 scripts/release_version.py
    python3 scripts/release_version.py --apply --notes-file target/release-notes.md \
        --github-output "$GITHUB_OUTPUT"

提交格式: <type>(<scope>)!: <subject>，正文里可以写 BREAKING CHANGE: ...

    feat: ...          minor
    fix: / perf: ...   patch
    <type>!: ...       major（0.x 阶段按 minor，避免过早 1.0）
    其它 type（chore/docs/refactor/test/ci/build/style）不触发发版
    chore(release): ... 由本脚本生成，会被忽略

没有历史 tag 时：把当前 Cargo.toml 版本作为首个正式版本打 tag，不改版本号。
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TAG_RE = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
COMMIT_RE = re.compile(
    r"^(?P<type>[A-Za-z][A-Za-z0-9_-]*)(?:\((?P<scope>[^)]*)\))?(?P<breaking>!)?:\s+(?P<subject>.+)$"
)
BREAKING_BODY_RE = re.compile(r"^BREAKING[ -]CHANGE:\s*(?P<note>.+)$", re.M)
RANK = {"none": 0, "patch": 1, "minor": 2, "major": 3}
CATEGORY = {
    "feat": "Features",
    "fix": "Bug Fixes",
    "perf": "Performance",
    "refactor": "Refactoring",
    "docs": "Documentation",
    "test": "Tests",
    "build": "Build",
    "ci": "CI",
    "style": "Style",
    "chore": "Chores",
}
CATEGORY_ORDER = [
    "Features",
    "Bug Fixes",
    "Performance",
    "Refactoring",
    "Documentation",
    "Tests",
    "Build",
    "CI",
    "Style",
    "Chores",
    "Other",
]


def run(args, check=True):
    return subprocess.run(args, cwd=ROOT, capture_output=True, text=True, check=check)


def git(*args, check=True):
    return run(["git", *args], check=check).stdout.strip()


def parse_semver(text):
    match = TAG_RE.match(text)
    return tuple(int(part) for part in match.groups()) if match else None


def format_version(version):
    return ".".join(str(part) for part in version)


def current_version():
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^\[package\][^\[]*?^version = "([^"]+)"', text, re.M | re.S)
    if not match:
        sys.exit("release_version: 根 Cargo.toml 里找不到 [package] version")
    return match.group(1)


def last_tag():
    described = git("describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", check=False)
    if described and parse_semver(described):
        return described
    tags = sorted(
        (tag for tag in git("tag", "--list", "v[0-9]*").splitlines() if parse_semver(tag)),
        key=parse_semver,
    )
    for tag in reversed(tags):
        if run(["git", "merge-base", "--is-ancestor", tag, "HEAD"], check=False).returncode == 0:
            return tag
    return None


def commits_since(tag):
    revision_range = tag + "..HEAD" if tag else "HEAD"
    raw = git("log", "--no-merges", "--format=%s%x00%B%x1e", revision_range)
    commits = []
    for record in raw.split("\x1e"):
        record = record.strip("\n")
        if not record:
            continue
        subject, _, body = record.partition("\x00")
        commits.append((subject.strip(), body))
    return commits


def classify(commits):
    bump = "none"
    categorized = []
    for subject, body in commits:
        match = COMMIT_RE.match(subject)
        if not match:
            categorized.append(("Other", subject, False))
            continue
        ctype = match.group("type").lower()
        scope = (match.group("scope") or "").strip()
        description = match.group("subject").strip()
        if ctype == "chore" and scope == "release":
            continue
        breaking = bool(match.group("breaking")) or bool(BREAKING_BODY_RE.search(body))
        if breaking and RANK["major"] > RANK[bump]:
            bump = "major"
        elif ctype == "feat" and RANK["minor"] > RANK[bump]:
            bump = "minor"
        elif ctype in ("fix", "perf") and RANK["patch"] > RANK[bump]:
            bump = "patch"
        categorized.append((CATEGORY.get(ctype, "Other"), description, breaking))
    return bump, categorized


def next_version(version, bump):
    major, minor, patch = version
    if bump == "major":
        if major == 0:
            return (0, minor + 1, 0)
        return (major + 1, 0, 0)
    if bump == "minor":
        return (major, minor + 1, 0)
    if bump == "patch":
        return (major, minor, patch + 1)
    return None


def forced_version(version, bump):
    major, minor, patch = version
    if bump == "major":
        return (major + 1, 0, 0)
    if bump == "minor":
        return (major, minor + 1, 0)
    return (major, minor, patch + 1)


def manifests():
    return [ROOT / "Cargo.toml", *sorted((ROOT / "crates").glob("*/Cargo.toml"))]


def rewrite_manifests(old, new):
    package_pattern = re.compile(r'^version = "' + re.escape(old) + r'"$', re.M)
    workspace_dep_pattern = re.compile(
        r'^(legado-[a-z0-9-]+ = \{ version = ")' + re.escape(old) + r'(")', re.M
    )
    total = 0
    for path in manifests():
        text = path.read_text(encoding="utf-8")
        text, package_count = package_pattern.subn('version = "' + new + '"', text)
        text, dep_count = workspace_dep_pattern.subn(lambda match: match.group(1) + new + match.group(2), text)
        if package_count + dep_count == 0:
            sys.exit("release_version: " + str(path) + " 里没有 version = \"" + old + "\"")
        if 'version = "' + old + '"' in text:
            sys.exit("release_version: " + str(path) + " 里还有没替换掉的 " + old)
        path.write_text(text, encoding="utf-8")
        total += package_count + dep_count
    return total


def rewrite_readmes(old, new):
    updated = []
    targets = (
        ("README.md", re.compile(r"The current version is \*\*" + re.escape(old) + r"\*\*")),
        ("README_ZH.md", re.compile(r"当前版本 \*\*" + re.escape(old) + r"\*\*")),
    )
    for name, pattern in targets:
        path = ROOT / name
        if not path.exists():
            continue
        text = path.read_text(encoding="utf-8")
        if pattern.search(text):
            path.write_text(
                pattern.sub(lambda match: match.group(0).replace(old, new), text),
                encoding="utf-8",
            )
            updated.append(name)
    return updated


def update_lockfile():
    result = subprocess.run(["cargo", "update", "--workspace"], cwd=ROOT, capture_output=True, text=True)
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        sys.exit("release_version: cargo update --workspace 失败")


def render_notes(tag, previous, categorized):
    lines = ["## " + tag, ""]
    lines.append(("Changes since " + previous) if previous else "First release.")
    lines.append("")
    breaking, groups = [], {}
    for category, description, is_breaking in categorized:
        if is_breaking:
            breaking.append(description)
        groups.setdefault(category, []).append(description)
    if breaking:
        lines += ["### Breaking Changes", ""] + ["- " + item for item in breaking] + [""]
    for category in CATEGORY_ORDER:
        items = groups.get(category)
        if not items:
            continue
        lines += ["### " + category, ""] + ["- " + item for item in items] + [""]
    return "\n".join(lines).rstrip() + "\n"


def write_github_output(path, values):
    with open(path, "a", encoding="utf-8") as handle:
        for key, value in values.items():
            handle.write(str(key) + "=" + str(value) + "\n")


def main():
    parser = argparse.ArgumentParser(description="Calculate the next semver release from conventional commits.")
    parser.add_argument("--apply", action="store_true", help="真正改写 Cargo.toml / README / Cargo.lock")
    parser.add_argument("--dry-run", action="store_true", help="只计算不写文件（默认行为，显式传更直观）")
    parser.add_argument("--bump", choices=["auto", "patch", "minor", "major"], default="auto")
    parser.add_argument("--notes-file", help="release notes 输出路径")
    parser.add_argument("--github-output", help="GitHub Actions 的 GITHUB_OUTPUT 文件")
    args = parser.parse_args()

    version_text = current_version()
    version = parse_semver("v" + version_text)
    if version is None:
        sys.exit("release_version: 版本号不是 semver: " + version_text)

    previous_tag = last_tag()
    commits = commits_since(previous_tag) if previous_tag else commits_since(None)
    bump, categorized = classify(commits)
    initial = previous_tag is None and args.bump == "auto"

    if initial:
        target = version
        bump = "initial"
    elif args.bump != "auto":
        target = forced_version(version, args.bump)
        bump = args.bump
    elif bump != "none":
        target = next_version(version, bump)
    else:
        target = None

    tag = "v" + format_version(target) if target else ""
    new_version = format_version(target) if target else version_text
    changed = target is not None and new_version != version_text
    readme_files = []
    manifest_files = 0

    if changed and args.apply:
        manifest_files = rewrite_manifests(version_text, new_version)
        readme_files = rewrite_readmes(version_text, new_version)
        update_lockfile()

    notes = render_notes(tag, previous_tag or "", categorized) if tag else ""
    if args.notes_file and notes:
        notes_path = Path(args.notes_file)
        notes_path.parent.mkdir(parents=True, exist_ok=True)
        notes_path.write_text(notes, encoding="utf-8")

    result = {
        "changed": changed,
        "initial": initial,
        "version": new_version,
        "tag": tag,
        "bump": bump,
        "previous_tag": previous_tag or "",
        "commits": len(categorized),
        "manifests_updated": manifest_files,
        "readmes_updated": readme_files,
    }
    if args.github_output:
        write_github_output(args.github_output, {
            "changed": str(changed).lower(),
            "initial": str(initial).lower(),
            "version": new_version,
            "tag": tag,
            "bump": bump,
        })
    print(json.dumps(result, ensure_ascii=False))
    if notes:
        print()
        print(notes)
    if not tag:
        print("没有需要发版的提交（只改了 chore/docs/refactor 等类型）")


if __name__ == "__main__":
    main()

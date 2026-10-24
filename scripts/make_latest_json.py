"""Writes the updater manifest (latest.json) for one release from its signed assets.

Usage: python scripts/make_latest_json.py <tag> <assets-dir> <owner/repo> [notes-file]

Needs <name>.sig next to the Windows setup .exe and/or the macOS .app.tar.gz (made by
`tauri build` with createUpdaterArtifacts, or by `tauri signer sign`). The universal macOS
bundle serves both Apple Silicon and Intel. Exits 2 when there is nothing signed.
"""
import datetime
import json
import pathlib
import sys


def first_paragraph(path):
    if not path or not pathlib.Path(path).is_file():
        return ""
    lines = []
    for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines():
        if line.strip():
            lines.append(line.strip())
        elif lines:
            break
    return " ".join(lines)


def manifest(tag, assets, repo, notes=""):
    base = f"https://github.com/{repo}/releases/download/{tag}/"
    platforms = {}

    def add(keys, pattern):
        for f in sorted(assets.glob(pattern)):
            sig = f.with_name(f.name + ".sig")
            if sig.is_file():
                entry = {"signature": sig.read_text(encoding="utf-8").strip(), "url": base + f.name}
                for k in keys:
                    platforms[k] = entry
                return

    add(["windows-x86_64"], "*-setup.exe")
    add(["darwin-aarch64", "darwin-x86_64"], "*.app.tar.gz")
    if not platforms:
        return None
    return {
        "version": tag.removeprefix("v"),
        "notes": notes,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }


def main():
    tag, assets, repo = sys.argv[1], pathlib.Path(sys.argv[2]), sys.argv[3]
    m = manifest(tag, assets, repo, first_paragraph(sys.argv[4] if len(sys.argv) > 4 else None))
    if m is None:
        print("no signed updater assets; latest.json not written", file=sys.stderr)
        sys.exit(2)
    (assets / "latest.json").write_text(json.dumps(m, indent=2) + "\n", encoding="utf-8")
    print("latest.json:", ", ".join(sorted(m["platforms"])))


if __name__ == "__main__":
    main()

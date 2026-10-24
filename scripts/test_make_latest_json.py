"""Self-check for make_latest_json.py: python -I scripts/test_make_latest_json.py"""
import pathlib
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from make_latest_json import first_paragraph, manifest  # noqa: E402

with tempfile.TemporaryDirectory() as d:
    d = pathlib.Path(d)
    assert manifest("v1.2.3", d, "o/r") is None, "nothing signed -> no manifest"
    (d / "Yappaflow_1.2.3_x64-setup.exe").write_bytes(b"x")
    (d / "Yappaflow_1.2.3_x64-setup.exe.sig").write_text("winsig\n")
    (d / "Yappaflow_1.2.3_universal.app.tar.gz").write_bytes(b"x")
    (d / "Yappaflow_1.2.3_universal.app.tar.gz.sig").write_text("macsig")
    (d / "Yappaflow_1.2.3_universal.dmg").write_bytes(b"x")  # not an updater asset
    m = manifest("v1.2.3", d, "o/r", "Notes.")
    assert m["version"] == "1.2.3" and m["notes"] == "Notes."
    assert set(m["platforms"]) == {"windows-x86_64", "darwin-aarch64", "darwin-x86_64"}
    w = m["platforms"]["windows-x86_64"]
    assert w == {"signature": "winsig", "url": "https://github.com/o/r/releases/download/v1.2.3/Yappaflow_1.2.3_x64-setup.exe"}
    assert m["platforms"]["darwin-x86_64"]["url"].endswith("universal.app.tar.gz")
    (d / "notes.md").write_text("\nFirst line\nsame paragraph.\n\nSecond.\n")
    assert first_paragraph(d / "notes.md") == "First line same paragraph."
print("ok")

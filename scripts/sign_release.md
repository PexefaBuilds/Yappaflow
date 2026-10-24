# Signing a release's updates by hand

Only needed when CI built a release **without** the updater key secrets (see `docs/RELEASE_CHECKLIST.md`). Once the secrets exist, `release.yml` does all of this itself. Run it on the draft, before publishing.

Windows PowerShell, repo root, `TAG` = the release (e.g. `v0.9.2`). The key never leaves this PC.

```powershell
$TAG = 'v0.9.2'; $R = 'PexefaBuilds/yappaflow'; $D = "$env:TEMP\yf-$TAG"
New-Item -ItemType Directory -Force $D | Out-Null
gh release download $TAG --repo $R --dir $D --pattern '*-setup.exe' --pattern '*.dmg'
```

macOS bundle: the updater needs a `.app.tar.gz`, which only `tauri build` with updater artifacts makes. Rebuild it from the `.dmg` on a Mac (`COPYFILE_DISABLE` keeps macOS metadata files out of the archive):

```powershell
$V = $TAG.TrimStart('v')
scp "$D\Yappaflow_${V}_universal.dmg" mac:/tmp/
ssh mac "m=`$(hdiutil attach -nobrowse -readonly /tmp/Yappaflow_${V}_universal.dmg | tail -n1 | cut -f3); COPYFILE_DISABLE=1 tar -czf /tmp/Yappaflow_${V}_universal.app.tar.gz -C `"`$m`" Yappaflow.app; hdiutil detach -quiet `"`$m`"; rm /tmp/Yappaflow_${V}_universal.dmg"
scp "mac:/tmp/Yappaflow_${V}_universal.app.tar.gz" $D\
```

Sign both, write `latest.json`, upload, and refresh `SHA256SUMS`:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = "$env:USERPROFILE\.tauri\yappaflow-updater.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = Get-Content -Raw "$env:USERPROFILE\.tauri\yappaflow-updater.key.password"
npx tauri signer sign "$D\Yappaflow_${V}_x64-setup.exe"
npx tauri signer sign "$D\Yappaflow_${V}_universal.app.tar.gz"
Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
python -I scripts/make_latest_json.py $TAG $D $R "docs/release-notes/$TAG.md"
gh release upload $TAG --repo $R --clobber "$D\latest.json" "$D\Yappaflow_${V}_x64-setup.exe.sig" "$D\Yappaflow_${V}_universal.app.tar.gz" "$D\Yappaflow_${V}_universal.app.tar.gz.sig"
```

Then regenerate `SHA256SUMS` from all assets (`gh release download` everything, `sha256sum * > SHA256SUMS`, upload with `--clobber`). When the release is published, `updater-beta.yml` copies `latest.json` to the Beta channel.

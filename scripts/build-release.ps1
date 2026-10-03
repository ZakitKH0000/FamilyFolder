# Установщик для раздачи: собирает, подписывает ключом обновлений и кладёт в dist\ вместе с .sig.
# Подпись нужна, чтобы устройства семьи приняли установщик как обновление (crates/app/src/updater.rs).
# Ключ — .keys\updater.key, пароль к нему — .keys\updater.pass (оба не в git!).
# Потеряете ключ — новые версии придётся ставить вручную.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$key = Join-Path $root '.keys\updater.key'
$pass = Join-Path $root '.keys\updater.pass'

if (-not (Test-Path $key)) {
    New-Item -ItemType Directory -Force (Split-Path $key) | Out-Null
    [Guid]::NewGuid().ToString('N') | Set-Content $pass -NoNewline
}
$pw = (Get-Content $pass -Raw).Trim()
if (-not (Test-Path $key)) {
    npx tauri signer generate -w $key -p $pw --ci
    Copy-Item "$key.pub" (Join-Path $root 'crates\app\updater.pub') -Force
}

$version = (Get-Content (Join-Path $root 'crates\app\tauri.conf.json') -Raw | ConvertFrom-Json).version
Push-Location (Join-Path $root 'crates\app')
try {
    npx --prefix ..\.. tauri build
    if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }
} finally { Pop-Location }

$exe = Get-ChildItem (Join-Path $root 'target\release\bundle\nsis') -Filter "*_${version}_x64-setup.exe" | Select-Object -First 1
if (-not $exe) { throw "installer $version not found" }
npx tauri signer sign -f $key -p $pw $exe.FullName | Out-Null
if (-not (Test-Path "$($exe.FullName).sig")) { throw 'signing failed' }

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
Copy-Item $exe.FullName, "$($exe.FullName).sig" $dist -Force
Write-Host "Ready: dist\$($exe.Name) (+ .sig)"

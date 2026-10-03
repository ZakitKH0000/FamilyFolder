# Пакет Microsoft Store (MSIX). Программа та же, что в установщике: режим Store она узнаёт сама
# (shell::is_packaged: обновляет Store, автозапуск — задача пакета). Подписывает пакет Microsoft при публикации.
#   .\scripts\build-store.ps1         → dist\FamilyFolder-<версия>.msix — загрузить в Partner Center
#   .\scripts\build-store.ps1 -Test   → проверочный пакет на этом компьютере (нужен «Режим разработчика»):
#                                       стоит рядом с обычной программой, запуск — family-folder-test.exe
param([switch]$Test, [switch]$NoBuild)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$sdk = Split-Path (Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\makeappx.exe' | Sort-Object FullName | Select-Object -Last 1).FullName
$conf = Get-Content (Join-Path $root 'crates\app\tauri.conf.json') -Raw | ConvertFrom-Json
$version = "$($conf.version).0"   # последняя часть версии в Store — всегда 0

if (-not $NoBuild) {
    Push-Location (Join-Path $root 'crates\app')
    try {
        npx --prefix ..\.. tauri build --no-bundle
        if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }
    } finally { Pop-Location }
}

$testName = 'ZakirKhalilov.FamilyFolder.Test'
if ($Test) {
    # Проверочный пакет работает прямо из папки сборки — сначала убрать прежний.
    Get-AppxPackage -Name $testName | Remove-AppxPackage
}
$pkg = Join-Path $root ('target\store\' + $(if ($Test) { 'test' } else { 'store' }))
if (Test-Path $pkg) { Remove-Item $pkg -Recurse -Force }
New-Item -ItemType Directory -Force $pkg | Out-Null
Copy-Item (Join-Path $root 'target\release\ObshayaPapka.exe') $pkg
Copy-Item (Join-Path $root 'crates\app\store\Assets') $pkg -Recurse

$m = Get-Content (Join-Path $root 'crates\app\store\AppxManifest.xml') -Raw -Encoding UTF8
if ($Test) {
    $alias = @'

        <uap3:Extension Category="windows.appExecutionAlias" Executable="ObshayaPapka.exe" EntryPoint="Windows.FullTrustApplication">
          <uap3:AppExecutionAlias>
            <desktop:ExecutionAlias Alias="family-folder-test.exe" />
          </uap3:AppExecutionAlias>
        </uap3:Extension>
'@
    $id = @{ NAME = $testName; PUBLISHER = 'CN=FamilyFolderTest'; DISPLAY = 'Family Folder Test'; EXTENSIONS = $alias }
} else {
    # Из Partner Center → Product identity.
    $id = @{ NAME = 'ZakirKhalilov.FamilyFolder'; PUBLISHER = 'CN=6EE97347-029C-41FF-9D64-B20990F5A6AB'; DISPLAY = 'Family Folder'; EXTENSIONS = '' }
}
$id.VERSION = $version
foreach ($k in $id.Keys) { $m = $m.Replace("{{$k}}", $id[$k]) }
[IO.File]::WriteAllText((Join-Path $pkg 'AppxManifest.xml'), $m, (New-Object Text.UTF8Encoding $false))

# Указатель картинок разных размеров (resources.pri): без него Windows не подберёт значок под масштаб.
$pri = Join-Path $root 'target\store\priconfig.xml'
& "$sdk\makepri.exe" createconfig /cf $pri /dq en-US /pv 10.0.0 /o | Out-Null
& "$sdk\makepri.exe" new /pr $pkg /cf $pri /mn (Join-Path $pkg 'AppxManifest.xml') /of (Join-Path $pkg 'resources.pri') /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'makepri failed' }

if ($Test) {
    Add-AppxPackage -Register (Join-Path $pkg 'AppxManifest.xml')
    Write-Host 'Проверочный пакет установлен: family-folder-test.exe'
} else {
    New-Item -ItemType Directory -Force (Join-Path $root 'dist') | Out-Null
    $out = Join-Path $root "dist\FamilyFolder-$($conf.version).msix"
    & "$sdk\makeappx.exe" pack /d $pkg /p $out /o | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'makeappx failed' }
    Write-Host "Ready: $out"
}

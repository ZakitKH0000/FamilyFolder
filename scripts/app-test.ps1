# Запуск тестового экземпляра программы со своей папкой данных и отладочным портом окна.
#   .\scripts\app-test.ps1 -Name A -Port 9301 [-Exe ...]   (остановить: -Stop)
param([Parameter(Mandatory)][string]$Name, [int]$Port = 9301, [string]$Root = "$env:TEMP\obshaya-apptest",
      [string]$Exe = "$PSScriptRoot\..\target\debug\ObshayaPapka.exe", [string[]]$AppArgs = @(), [switch]$Stop)
$data = "$Root\$Name-data"
if ($Stop) {
  if (Test-Path "$data\pid") { Stop-Process -Id (Get-Content "$data\pid") -Force -ErrorAction SilentlyContinue; Remove-Item "$data\pid" }
  return
}
New-Item -ItemType Directory -Force $data | Out-Null
$env:OBSHAYA_DATA_DIR = $data
$env:OBSHAYA_FOLDER = "$Root\$Name Общая"
$env:OBSHAYA_NO_TRASH = $null
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"
$p = if ($AppArgs.Count) { Start-Process -FilePath $Exe -ArgumentList $AppArgs -PassThru -WindowStyle Minimized } else { Start-Process -FilePath $Exe -PassThru -WindowStyle Minimized }
$p.Id | Out-File "$data\pid" -Encoding ascii
for ($i = 0; $i -lt 60; $i++) {
  Start-Sleep -Milliseconds 500
  try { Invoke-RestMethod "http://127.0.0.1:$Port/json" -TimeoutSec 1 | Out-Null; "экземпляр $Name запущен (pid $($p.Id), порт $Port)"; return } catch {}
}
"экземпляр $Name не открыл порт отладки"

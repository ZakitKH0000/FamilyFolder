# Локальная проверка probe: отправитель и получатель на одном ПК.
#   .\scripts\local-test.ps1 -File <файл> [-Resume]
# -Resume: получатель обрывается на ~80 МБ и запускается заново (проверка докачки).
# Ввод подаётся через -RedirectStandardInput: конвейер "..." | exe в PowerShell 5.1 зависает.
param(
    [Parameter(Mandatory)][string]$File,
    [string]$Exe = "$PSScriptRoot\..\target\release\obshaya-test.exe",
    [string]$Dir = "$env:TEMP\obshaya-test-run",
    [switch]$Resume
)
$ErrorActionPreference = "Stop"
Get-Process obshaya-test -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item $Dir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$Dir\send", "$Dir\recv" | Out-Null
Copy-Item $Exe "$Dir\send\obshaya-test.exe"
Copy-Item $Exe "$Dir\recv\obshaya-test.exe"

function Show-Log($path) {
    Get-Content $path -Encoding UTF8 | ForEach-Object { ($_ -split "`r")[-1] } | Where-Object { $_.Trim() }
}

$sender = Start-Process "$Dir\send\obshaya-test.exe" -ArgumentList "`"$File`"" -PassThru -WindowStyle Hidden `
    -RedirectStandardOutput "$Dir\sender.log" -RedirectStandardError "$Dir\sender.err"
$code = $null
for ($i = 0; $i -lt 120 -and -not $code; $i++) {
    Start-Sleep -Milliseconds 500
    $m = Select-String -Path "$Dir\sender.log" -Pattern '\b[0-9a-f]{64}\b' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($m) { $code = $m.Matches[0].Value }
}
if (-not $code) { Stop-Process -Id $sender.Id -Force; throw "отправитель не выдал код" }
[IO.File]::WriteAllText("$Dir\input.txt", "2`n$code`n`n`n")

function Start-Receiver($log) {
    Start-Process "$Dir\recv\obshaya-test.exe" -PassThru -WindowStyle Hidden `
        -RedirectStandardInput "$Dir\input.txt" -RedirectStandardOutput $log -RedirectStandardError "$log.err"
}

if ($Resume) {
    $r = Start-Receiver "$Dir\recv1.log"
    for ($i = 0; $i -lt 1200; $i++) {
        Start-Sleep -Milliseconds 50
        $part = Get-ChildItem "$Dir\recv\Получено\*.part" -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($part -and $part.Length -ge 80MB) { Stop-Process -Id $r.Id -Force; break }
    }
    "получатель оборван на $([math]::Round($part.Length / 1MB, 1)) МБ"
}

$r = Start-Receiver "$Dir\recv.log"
if (-not $r.WaitForExit(600000)) { Stop-Process -Id $r.Id -Force }
Start-Sleep -Seconds 1
Stop-Process -Id $sender.Id -Force -ErrorAction SilentlyContinue

"=== получатель"; Show-Log "$Dir\recv.log" | Select-Object -Last 6
"=== отправитель"; Show-Log "$Dir\sender.log" | Select-Object -Last 4
$got = Get-ChildItem "$Dir\recv\Получено" -File | Where-Object Extension -ne ".part" | Select-Object -First 1
if ($got -and (Get-FileHash $got.FullName).Hash -eq (Get-FileHash $File).Hash) { "РЕЗУЛЬТАТ: OK, файлы совпадают" }
else { "РЕЗУЛЬТАТ: ОШИБКА, файл не получен или отличается" }

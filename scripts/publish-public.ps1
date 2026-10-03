# Выкладывает код на открытую страницу github.com/ZakitKH0000/FamilyFolder (папка showcase\ — её клон).
# Туда идёт снимок последнего сохранённого состояния (HEAD) без истории и без рабочих заметок:
# CLAUDE.md, PLAN.md и .claude\ на открытую страницу не попадают никогда.
#   .\scripts\publish-public.ps1 -Message "1.4.3: ..."
param([Parameter(Mandatory)][string]$Message)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$pub = Join-Path $root 'showcase'
if (-not (Test-Path (Join-Path $pub '.git'))) {
    git clone https://github.com/ZakitKH0000/FamilyFolder.git $pub
    git -C $pub config user.name 'Zakir'
    git -C $pub config user.email '329148760+ZakitKH0000@users.noreply.github.com'
}
if (git -C $root status --porcelain) { throw 'Есть несохранённые изменения — сначала сохраните их (git commit).' }
# Файлы из архива уже с нужными окончаниями строк — без пересчёта (и без предупреждений git на каждый файл).
git -C $pub config core.autocrlf false

Get-ChildItem $pub -Force | Where-Object { $_.Name -ne '.git' } | Remove-Item -Recurse -Force
$zip = Join-Path $env:TEMP 'family-folder-public.zip'
git -C $root archive --format=zip -o $zip HEAD -- . ':(exclude)CLAUDE.md' ':(exclude)PLAN.md' ':(exclude).claude'
Expand-Archive $zip $pub -Force
Remove-Item $zip
foreach ($f in 'CLAUDE.md', 'PLAN.md', '.claude') {
    if (Test-Path (Join-Path $pub $f)) { throw "$f попал в снимок — остановлено" }
}

git -C $pub add -A
if (-not (git -C $pub status --porcelain)) { Write-Host 'Нечего выкладывать: на странице уже это состояние.'; return }
git -C $pub commit -q -m $Message
git -C $pub push -q origin main
Write-Host 'Выложено: https://github.com/ZakitKH0000/FamilyFolder'

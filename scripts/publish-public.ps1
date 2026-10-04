# Выкладывает код на открытую страницу github.com/ZakitKH0000/FamilyFolder (папка showcase\ — её клон).
# Туда идёт снимок последнего сохранённого состояния (HEAD) без истории и без рабочих заметок:
# AGENTS.md, CLAUDE.md, PLAN.md, WORKLOG.md, .claude\, .codex\ и черновики promo\ не публикуются.
#   .\scripts\publish-public.ps1 -Message "1.4.3: ..."
param([Parameter(Mandatory)][string]$Message)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$pub = Join-Path $root 'showcase'
# Проверяем точный путь перед рекурсивной очисткой клона.
$root = (Resolve-Path -LiteralPath $root).Path
$pub = [IO.Path]::GetFullPath($pub)
if ($pub -ne (Join-Path $root 'showcase')) { throw 'Неожиданный путь публичного клона' }
if (-not (Test-Path (Join-Path $pub '.git'))) {
    git clone https://github.com/ZakitKH0000/FamilyFolder.git $pub
    if ($LASTEXITCODE -ne 0) { throw 'Не удалось клонировать публичный репозиторий' }
    git -C $pub config user.name 'Zakir'
    git -C $pub config user.email '329148760+ZakitKH0000@users.noreply.github.com'
}
if (git -C $root status --porcelain) { throw 'Есть несохранённые изменения — сначала сохраните их (git commit).' }
if (git -C $pub status --porcelain) { throw 'В публичном клоне есть несохранённые изменения' }
if ((git -C $pub remote get-url origin) -ne 'https://github.com/ZakitKH0000/FamilyFolder.git') { throw 'Неожиданный origin публичного клона' }
if ((Get-Item -LiteralPath $pub).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Публичный клон не должен быть ссылкой' }
if (Get-ChildItem -LiteralPath $pub -Force -Recurse -Attributes ReparsePoint) { throw 'В публичном клоне есть ссылки — очистка остановлена' }
# Файлы из архива уже с нужными окончаниями строк — без пересчёта (и без предупреждений git на каждый файл).
git -C $pub config core.autocrlf false

Get-ChildItem -LiteralPath $pub -Force | Where-Object { $_.Name -ne '.git' } | ForEach-Object { Remove-Item -LiteralPath $_.FullName -Recurse -Force }
$zip = Join-Path $env:TEMP 'family-folder-public.zip'
git -C $root archive --format=zip -o $zip HEAD -- . ':(exclude)AGENTS.md' ':(exclude)CLAUDE.md' ':(exclude)PLAN.md' ':(exclude)WORKLOG.md' ':(exclude).claude' ':(exclude).codex' ':(exclude)promo'
if ($LASTEXITCODE -ne 0) { throw 'Не удалось создать публичный снимок' }
Expand-Archive $zip $pub -Force
Remove-Item $zip
foreach ($f in 'AGENTS.md', 'CLAUDE.md', 'PLAN.md', 'WORKLOG.md', '.claude', '.codex', 'promo') {
    if (Test-Path (Join-Path $pub $f)) { throw "$f попал в снимок — остановлено" }
}

git -C $pub add -A
if ($LASTEXITCODE -ne 0) { throw 'Не удалось подготовить публичный коммит' }
if (-not (git -C $pub status --porcelain)) { Write-Host 'Нечего выкладывать: на странице уже это состояние.'; return }
git -C $pub commit -q -m $Message
if ($LASTEXITCODE -ne 0) { throw 'Не удалось сохранить публичный коммит' }
git -C $pub push -q origin main
if ($LASTEXITCODE -ne 0) { throw 'Не удалось отправить публичный коммит' }
Write-Host 'Выложено: https://github.com/ZakitKH0000/FamilyFolder'

# Focused, deterministic checks for the pre-session configuration (strategist.config.ps1) and its
# integration in strategist.ps1. Simulated Codex and Claude, throwaway Git repositories, a
# throwaway LOCALAPPDATA: no model is called and the user's real config is never touched. Run:
#   powershell -NoProfile -ExecutionPolicy Bypass -File strategist.config.tests.ps1
# Keep this file ASCII-only (it runs in Windows PowerShell 5.1).
$ErrorActionPreference = 'Stop'
$tool = $PSScriptRoot
$root = Join-Path ([IO.Path]::GetTempPath()) ("strategist-cfgtests-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path "$root\bin", "$root\codex-home\rules", "$root\tmp", "$root\appdata" | Out-Null
$env:TEMP = "$root\tmp"; $env:TMP = "$root\tmp"
$env:LOCALAPPDATA = "$root\appdata"
$env:MOCK_DIR = $root
$env:CODEX_HOME = "$root\codex-home"
$env:GIT_CEILING_DIRECTORIES = $root
$env:PATH = "$root\bin;$env:PATH"
$git = (Get-Command git.exe).Source

$script:failures = 0
function Check([string]$id, [string]$name, [bool]$ok, [string]$detail = '') {
    if (-not $ok) { $script:failures++ }
    "{0} {1,-4} {2}{3}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $id, $name, $(if ($detail) { "  [$detail]" })
}
function Throws([scriptblock]$code) { try { & $code | Out-Null; $null } catch { "$($_.Exception.Message)" } }

. "$tool\strategist.config.ps1"
$cfgPath = Get-OrqConfigPath

function New-Repo([string]$name) {
    $dir = "$root\$name"
    New-Item -ItemType Directory -Path $dir | Out-Null
    & $git -C $dir init -q .; & $git -C $dir config user.email t@t; & $git -C $dir config user.name t
    [IO.File]::WriteAllText("$dir\a.txt", "a`n"); & $git -C $dir add -A; & $git -C $dir commit -q -m init
    $dir
}
$repo = New-Repo 'repo'
$spaced = New-Repo 'repo com espacos'
$plain = "$root\not-git"; New-Item -ItemType Directory -Path $plain | Out-Null

function Cfg([string]$project, [string]$oTool = 'codex', [string]$oModel = '', [string]$oEffort = '', [string]$eTool = 'claude', [string]$eModel = '', [string]$eEffort = '') {
    [pscustomobject]@{ version = 1; project = $project
        orchestrator = [pscustomobject]@{ tool = $oTool; model = $oModel; effort = $oEffort }
        executor = [pscustomobject]@{ tool = $eTool; model = $eModel; effort = $eEffort } }
}

# --- configuration: storage and validation ---
Check 'C01' 'config path is under LOCALAPPDATA\orquestrador' ($cfgPath -eq "$root\appdata\orquestrador\config.json")
Check 'C02' 'absent file -> $null (legacy)' ($null -eq (Read-OrqConfig $cfgPath))
Save-OrqConfig (Cfg $repo 'codex' 'gpt-x' 'high' 'claude' 'claude-opus-x[1m]' 'max') $cfgPath
$back = Read-OrqConfig $cfgPath
Check 'C03' 'save then load keeps every field' ($back.project -eq $repo -and $back.orchestrator.model -eq 'gpt-x' -and $back.orchestrator.effort -eq 'high' -and $back.executor.model -eq 'claude-opus-x[1m]' -and $back.executor.effort -eq 'max')
Check 'C04' 'no leftover .tmp after save' (-not (Test-Path "$cfgPath.tmp"))
Save-OrqConfig (Cfg $spaced) $cfgPath
$back = Read-OrqConfig $cfgPath
Check 'C05' 'folder with spaces saves, loads and resolves to Git' ($back.project -eq $spaced -and (Get-OrqGitRoot $back.project) -eq $spaced)
$back2 = Cfg $repo
Check 'C06' 'empty model/effort stays empty and yields no extra args' ((Get-OrqClaudeArgs $back2.executor).Count -eq 0 -and (Get-OrqCodexArgs $back2.orchestrator).Count -eq 0)

Set-Content -LiteralPath $cfgPath -Value '{ not json' -Encoding ascii
$e = Throws { Read-OrqConfig $cfgPath }
Check 'C07' 'invalid JSON throws a useful message' ($e -match 'JSON ilegivel' -and $e -match [regex]::Escape($cfgPath)) $e
Set-Content -LiteralPath $cfgPath -Value (Cfg $repo 'codex' '' 'minimal' | ConvertTo-Json -Depth 4) -Encoding ascii
$e = Throws { Read-OrqConfig $cfgPath }
Check 'C08' 'invalid value in file throws, no silent fallback' ($e -match "esforco 'minimal' invalido")  $e

$e = Throws { Save-OrqConfig (Cfg $repo 'codex' '' '' 'codex') $cfgPath }
Check 'C09' 'Codex as executor rejected before any session, with the short reason' ($e -match 'executor' -and $e -match 'indisponivel' -and $e -match 'commit requer Git fora do sandbox') $e
$e = Throws { Save-OrqConfig (Cfg $repo 'claude') $cfgPath }
Check 'C10' 'Claude as orchestrator rejected (deferred)' ($e -match 'orchestrator' -and $e -match 'adiado') $e
$e = Throws { Save-OrqConfig (Cfg $repo 'gemini') $cfgPath }
Check 'C11' 'unknown tool rejected' ($e -match "'gemini' desconhecida") $e

$okEf = $true
foreach ($v in 'low', 'medium', 'high', 'xhigh', 'max') { if (@(Test-OrqConfig (Cfg $repo 'codex' '' '' 'claude' '' $v) | Where-Object { $_ -match 'esforco' }).Count) { $okEf = $false } }
Check 'C12' 'valid Claude efforts accepted' $okEf
$okEf = $true
foreach ($v in 'low', 'medium', 'high', 'xhigh', 'max', 'ultra') { if (@(Test-OrqConfig (Cfg $repo 'codex' '' $v) | Where-Object { $_ -match 'esforco' }).Count) { $okEf = $false } }
Check 'C13' 'valid Codex efforts accepted' $okEf
Check 'C14' 'Claude effort invalid for Claude (minimal) and case-sensitive' ((@(Test-OrqConfig (Cfg $repo 'codex' '' '' 'claude' '' 'minimal')) -match 'esforco').Count -eq 1 -and (@(Test-OrqConfig (Cfg $repo 'codex' '' '' 'claude' '' 'HIGH')) -match 'esforco').Count -eq 1)
Check 'C15' 'Codex effort invalid (minimal, not in installed catalog); Claude rejects ultra' ((@(Test-OrqConfig (Cfg $repo 'codex' '' 'minimal')) -match 'esforco').Count -eq 1 -and (@(Test-OrqConfig (Cfg $repo 'codex' '' '' 'claude' '' 'ultra')) -match 'esforco').Count -eq 1)

$badModels = '-evil', 'a b', 'x"y', '$(calc)', 'a;b', 'a|b', '`x'
$allBad = $true
foreach ($m in $badModels) { if (-not (@(Test-OrqConfig (Cfg $repo 'codex' '' '' 'claude' $m)) -match 'modelo').Count) { $allBad = $false; $m } }
Check 'C16' 'model with spaces, quotes, shell syntax or leading dash rejected' $allBad
$a = Get-OrqClaudeArgs (Cfg $repo 'codex' '' '' 'claude' 'my-model:v1[1m]' 'high').executor
Check 'C17' 'custom model is passed as a literal element' ($a.Count -eq 4 -and $a[0] -eq '--model' -and $a[1] -eq 'my-model:v1[1m]' -and $a[2] -eq '--effort' -and $a[3] -eq 'high')
$a = Get-OrqCodexArgs (Cfg $repo 'codex' 'gpt-5.x' 'low').orchestrator
Check 'C18' 'Codex args: -m model and -c model_reasoning_effort' ($a.Count -eq 4 -and $a[0] -eq '-m' -and $a[1] -eq 'gpt-5.x' -and $a[2] -eq '-c' -and $a[3] -eq 'model_reasoning_effort=low')
$a = Get-OrqCodexArgs (Cfg $repo 'codex' '' 'low').orchestrator
Check 'C19' 'roles are independent: only effort set -> no -m' ($a.Count -eq 2 -and $a[0] -eq '-c')

Check 'C20' 'missing folder rejected' ((@(Test-OrqConfig (Cfg "$root\nope")) -match 'nao existe').Count -eq 1)
Check 'C21' 'folder outside Git rejected, Git not initialized' ((@(Test-OrqConfig (Cfg $plain)) -match 'repositorio Git').Count -eq 1 -and -not (Test-Path "$plain\.git"))
Check 'C22' 'empty folder rejected' ((@(Test-OrqConfig (Cfg '')) -match 'nao definida').Count -eq 1)
$sub = "$repo\sub dir"; New-Item -ItemType Directory -Path $sub | Out-Null
Check 'C23' 'subfolder resolves to the Git top level' ((Get-OrqGitRoot $sub) -eq $repo)

Set-Content "$root\bin\fake-claude.ps1" -Encoding ascii @'
if ($args -contains '--version') { '9.9.9 (fake)'; exit 0 }
if ($args -contains '--help') { '  -p, --print'; '  --permission-mode <mode> (choices: "acceptEdits", "auto")'; '  --permission-prompts <target>'; '  --disallowedTools <tools...>'; '  --model <model>   Model for the session (e.g.'; "  'opus' or 'sonnet') or a full name."; '  --effort <level>'; exit 0 }
$ms = New-Object IO.MemoryStream; [Console]::OpenStandardInput().CopyTo($ms)
Add-Content -LiteralPath "$env:MOCK_DIR\claude-args.txt" -Value ($args -join '|')
'fake claude: feito'
exit 0
'@
Set-Content "$root\bin\claude.cmd" -Encoding ascii '@powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0fake-claude.ps1" %*'
Set-Content "$root\bin\fake-codex.ps1" -Encoding ascii @'
Set-Content "$env:MOCK_DIR\args.txt" ($args -join '|')
$s = $args[[Array]::IndexOf($args, '-C') + 1]
Set-Content "$env:MOCK_DIR\files.txt" ((Get-ChildItem -LiteralPath $s -Force | ForEach-Object Name) -join '|')
Copy-Item -LiteralPath "$s\session-config.json" "$env:MOCK_DIR\frozen-copy.json" -ErrorAction SilentlyContinue
Set-Content "$env:MOCK_DIR\session.txt" $s
while (-not (Test-Path "$env:MOCK_DIR\stop")) { Start-Sleep -Milliseconds 150 }
exit 0
'@
Set-Content "$root\bin\codex.cmd" -Encoding ascii '@powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0fake-codex.ps1" %*'

# --- wizard (scripted keys; Read/Write/ReadKey injected) ---
$script:allSaid = New-Object Collections.ArrayList
$script:acts = @{}
function Chars([string]$s) { $s.ToCharArray() | ForEach-Object { [string]$_ } }
function Run-Wizard($keys, [string]$default = $repo) {
    $script:q = New-Object Collections.Queue
    foreach ($k in @($keys)) { $script:q.Enqueue($k) }
    $script:said = New-Object Collections.ArrayList
    $r = Invoke-OrqMenu -ConfigPath $cfgPath -DefaultProject $default `
        -ReadKey { while ($script:q.Count) { $t = $script:q.Dequeue(); if ("$t" -like 'run:*') { & $script:acts["$t"]; continue }; return $t }; $null } `
        -Write { param($t, $c) $null = $script:said.Add($t); $null = $script:allSaid.Add($t) }
    "$r"
}
# Line mode: only -Read is given (what a redirected stdin gets).
function Run-Lines($lines, [string]$default = $repo) {
    $script:q = New-Object Collections.Queue
    foreach ($k in @($lines)) { $script:q.Enqueue($k) }
    $script:said = New-Object Collections.ArrayList
    $r = Invoke-OrqMenu -ConfigPath $cfgPath -DefaultProject $default `
        -Read { param($p) if ($script:q.Count) { $script:q.Dequeue() } else { $null } } `
        -Write { param($t, $c) $null = $script:said.Add($t); $null = $script:allSaid.Add($t) }
    "$r"
}
$E6 = @('Enter') * 6   # project, orch model, orch effort, exec model, exec effort, review (save+start)
$cacheFile = "$env:CODEX_HOME\models_cache.json"
$catalogJson = '{"models":[{"slug":"gpt-6-sol","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"},{"effort":"max"},{"effort":"ultra"}]},{"slug":"gpt-6-luna","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"},{"effort":"high"},{"effort":"xhigh"},{"effort":"max"}]},{"slug":"gpt-hidden","visibility":"hide","supported_reasoning_levels":[{"effort":"low"}]},{"slug":5},{"display_name":"no slug"}]}'

if (Test-Path $cfgPath) { Remove-Item -LiteralPath $cfgPath -Force }
$r = Run-Wizard $E6
$saved = Read-OrqConfig $cfgPath
Check 'W01' 'defaults: Enter through every step saves and starts' ($r -eq 'start' -and $saved.project -eq $repo -and $saved.orchestrator.tool -eq 'codex' -and $saved.executor.tool -eq 'claude' -and -not $saved.orchestrator.model -and -not $saved.executor.effort)
$said1 = $said -join "`n"
Check 'W02' 'compact header, progress trail, key footer, no numbered list' ($said1 -match 'Projeto - etapa 1/4' -and $said1 -match '\[Projeto\] > Orquestrador > Executor > Revisao' -and $said1 -match 'Orquestrador: modelo - etapa 2/4 \| ferramenta: codex' -and $said1 -match 'Setas: escolher \| Enter: confirmar \| Esc: sair' -and $said1 -match 'Esc: voltar' -and $said1 -notmatch '(?m)^\s*>?\s*\d\) ')
Check 'W03' 'selection marker is ASCII and the single-tool steps are not asked' ($said1 -match '(?m)^> Padrao atual do CLI' -and $said1 -notmatch 'ferramenta - etapa' -and $said1 -notmatch 'Orquestrador: ferramenta' -and $said1 -notmatch 'Executor: ferramenta')

Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Wizard @('Esc')
Check 'W04' 'Esc on the first step leaves without saving' ($r -eq 'exit' -and -not (Test-Path $cfgPath))
$r = Run-Wizard @('Enter', 'Enter', 'Down', 'Enter', 'Esc', 'Esc', 'Esc', 'Esc')
Check 'W05' 'Esc walks back through the steps and out, nothing written' ($r -eq 'exit' -and -not (Test-Path $cfgPath))
Check 'W06' 'going back keeps the draft (effort cursor returns on the chosen value)' ((@($said -match '^> low$')).Count -ge 1 -and (@($said -match 'Orquestrador: esforco - etapa 2/4')).Count -ge 2)
# Draft edited, back to the start, forward again, saved: the edit is still there.
$r = Run-Wizard (@('Enter', 'Enter', 'Down', 'Enter', 'Esc', 'Esc', 'Esc', 'Enter', 'Enter') + @('Enter') * 5)
$saved = Read-OrqConfig $cfgPath
Check 'W07' 'back to project and forward again preserves choices made later' ($r -eq 'start' -and $saved.orchestrator.effort -eq 'low')

# EOF in every step: exit, no save.
$eofOk = $true
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
for ($n = 0; $n -le 5; $n++) {
    $r = Run-Wizard (@('Enter') * $n)
    if ($r -ne 'exit' -or (Test-Path $cfgPath)) { $eofOk = $false }
}
Check 'W08' 'closed input at any step exits and never saves' $eofOk
$r = Run-Wizard (@('Enter') * 5 + @('Ctrl+C'))
Check 'W09' 'Ctrl+C on the review neither saves nor starts' ($r -eq 'exit' -and -not (Test-Path $cfgPath))

# Review actions.
$r = Run-Wizard (@('Enter') * 5 + @('Down', 'Enter'))
$saved = Read-OrqConfig $cfgPath
Check 'W10' 'review: Salvar e sair saves and exits' ($r -eq 'exit' -and $saved.project -eq $repo)
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Wizard (@('Enter') * 5 + @('Down', 'Down', 'Enter', 'Esc', 'Esc', 'Esc', 'Esc', 'Esc', 'Esc'))
Check 'W11' 'review: Voltar goes back, nothing saved' ($r -eq 'exit' -and -not (Test-Path $cfgPath) -and (@($said -match 'Orquestrador: esforco')).Count -ge 1)
$r = Run-Wizard (@('Enter') * 5 + @('Down', 'Down', 'Down', 'Enter'))
Check 'W12' 'review: Sair sem salvar exits, nothing saved' ($r -eq 'exit' -and -not (Test-Path $cfgPath))
$r = Run-Wizard (@('Enter') * 5 + @('Esc', 'Enter', 'Enter'))
$saved = Read-OrqConfig $cfgPath
Check 'W13' 'review: Esc returns to the last step; confirming again reaches the review' ($r -eq 'start' -and $saved.project -eq $repo)
$rev = $said -join "`n"
Check 'W14' 'review shows project and both roles and the four actions' ($rev -match 'Projeto: ' -and $rev -match 'Orquestrador: codex \| modelo: padrao \| esforco: padrao' -and $rev -match 'Executor: claude \| modelo: padrao' -and $rev -match 'Salvar e iniciar' -and $rev -match 'Salvar e sair' -and $rev -match '(?m)^\s+Voltar$' -and $rev -match 'Sair sem salvar')

# No initial valid config: fill it through the wizard, with a typed folder.
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Wizard (@('Enter') + (Chars $spaced) + @('Enter') + @('Enter') * 5) ''
$saved = Read-OrqConfig $cfgPath
Check 'W15' 'no config and no default folder: typed path fills the wizard' ($r -eq 'start' -and $saved.project -eq $spaced)
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Wizard (@('Down', 'Enter') + (Chars $plain) + @('Enter', 'Esc'))
Check 'W16' 'folder outside Git is refused with a short message, nothing saved' ($r -eq 'exit' -and -not (Test-Path $cfgPath) -and (@($said -match '^ERRO: .*repositorio Git')).Count -eq 1)

# Model/effort dependency (local catalog), preserving unrelated fields.
Set-Content -LiteralPath $cacheFile -Value $catalogJson -Encoding ascii
Save-OrqConfig (Cfg $repo 'codex' 'gpt-6-sol' 'ultra' 'claude' 'claude-exe-x' 'high') $cfgPath
$r = Run-Wizard @('Enter', 'Enter', 'Esc', 'Down', 'Enter', 'Enter', 'Enter', 'Enter', 'Enter')
$saved = Read-OrqConfig $cfgPath
$ni = ($said | ForEach-Object { $_ }).IndexOf("Esforco 'ultra' nao vale para gpt-6-luna; voltou ao padrao.")
$before = if ($ni -gt 0) { @($said[0..$ni]) } else { @() }
$after = if ($ni -ge 0) { @($said[$ni..([Math]::Min($ni + 12, $said.Count - 1))]) } else { @() }
Check 'W17' 'Codex model in the catalog offers only its levels; hidden model not suggested' ((@($before -match '^(> |  )ultra$')).Count -ge 1 -and (@($after -match '^(> |  )ultra$')).Count -eq 0 -and (@($said -match 'gpt-hidden')).Count -eq 0 -and (@($said -match 'cache local')).Count -ge 1)
Check 'W18' 'changing the model re-fits only the dependent effort and says so' ($r -eq 'start' -and $ni -ge 0 -and $saved.orchestrator.model -eq 'gpt-6-luna' -and $saved.orchestrator.effort -eq '' -and $saved.executor.model -eq 'claude-exe-x' -and $saved.executor.effort -eq 'high' -and $saved.project -eq $repo) "ni=$ni"
$cat = @(Get-OrqCodexCatalog)
Check 'W19' 'catalog parse: ignores junk entries, keeps levels per model' ($cat.Count -eq 3 -and ($cat | Where-Object Slug -eq 'gpt-6-luna').Levels.Count -eq 5 -and -not ($cat | Where-Object Slug -eq 'gpt-hidden').Listed)
$okCat = $true
foreach ($bad in '{ not json', '[1,2]', '{"models":"x"}', '{"models":[1,"a",null]}', '', '{"models":[{"slug":"m","supported_reasoning_levels":"high"}]}') {
    Set-Content -LiteralPath $cacheFile -Value $bad -Encoding ascii
    $c = @(try { Get-OrqCodexCatalog } catch { $okCat = $false })
    $r = Run-Wizard $E6
    if ($r -ne 'start') { $okCat = $false }
}
Remove-Item -LiteralPath $cacheFile -Force
$c = @(Get-OrqCodexCatalog)
Check 'W20' 'cache unreadable / other schema / absent: tolerated, wizard still completes' ($okCat -and $c.Count -eq 0)
$env:CODEX_HOME = ''
$noThrow = $true; try { $null = @(Get-OrqCodexCatalog) } catch { $noThrow = $false }
$env:CODEX_HOME = "$root\codex-home"
Check 'W21' 'without CODEX_HOME the user default is used without failing' $noThrow
$said1 = $said -join "`n"
Save-OrqConfig (Cfg $repo 'codex' 'unknown-model' 'ultra') $cfgPath
$r = Run-Wizard @('Enter', 'Enter', 'Enter', 'Esc', 'Esc', 'Esc', 'Esc')
Check 'W22' 'unknown/default model: all tool efforts offered with a short, non-claiming hint' ((@($said -match 'ultra')).Count -ge 1 -and (@($said -match 'Nao conferido com o modelo; a ferramenta confirma')).Count -ge 1 -and (@($said -match 'cache local')).Count -eq 0)

# Custom model and Claude aliases.
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Wizard (@('Enter', 'Down', 'Enter') + (Chars 'my-model:v1') + @('Enter') + @('Enter') * 4)
$saved = Read-OrqConfig $cfgPath
Check 'W23' 'custom model typed in the wizard is saved as typed' ($r -eq 'start' -and $saved.orchestrator.model -eq 'my-model:v1')
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Wizard (@('Enter', 'Down', 'Enter') + (Chars 'a b') + @('Enter'))
Check 'W24' 'invalid custom model: short message, same screen, no file' ($r -eq 'exit' -and -not (Test-Path $cfgPath) -and (@($said -match "^ERRO: modelo 'a b' invalido")).Count -eq 1)
$al = @(Get-OrqClaudeAliasesFromHelp "  --model <model>   Model for the current session. Provide`n  an alias for the latest model (e.g.`n  'fable', 'opus', or 'sonnet') or a`n  model's full name.`n  --mcp-config <c>  x")
Check 'W25' 'Claude aliases come only from the help text' ($al.Count -eq 3 -and $al[1] -eq 'opus' -and @(Get-OrqClaudeAliasesFromHelp "  --model <model>  Model`n  --other x").Count -eq 0 -and @(Get-OrqClaudeAliasesFromHelp '').Count -eq 0)
# Real `claude --help` (2.1.296): padded lines push the next `--option` past 400 chars; -n, --name follows.
$pad = ' ' * 38
$realHelp = @(
    "  --fallback-model <model>              Enable automatic fallback to specified",
    "  --model <model>                       Model for the current session. Provide",
    "$pad  an alias for the latest model (e.g.",
    "$pad  'fable', 'opus', or 'sonnet') or a",
    "$pad  model's full name.",
    "  -n, --name <name>                     Set a display name for this session",
    "$pad  (shown in the prompt box, /resume",
    "$pad  picker, and terminal title)",
    "  --no-chrome                           Disable Claude in Chrome integration",
    "  --mcp-config <configs...>             Load MCP servers from JSON files or strings (e.g. 'other', 'ignored')") -join "`r`n"
$al = @(Get-OrqClaudeAliasesFromHelp $realHelp)
Check 'W25b' 'real claude help with wrapped padding yields exactly fable, opus, sonnet' (($al -join ',') -ceq 'fable,opus,sonnet') ($al -join ',')
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Wizard (@('Enter') * 3 + @('Down', 'Enter', 'Enter', 'Enter'))
$saved = Read-OrqConfig $cfgPath
Check 'W26' 'Claude executor suggests the aliases confirmed by its help' ($r -eq 'start' -and $saved.executor.model -eq 'opus' -and (@($said -match '^(> |  )sonnet$')).Count -ge 1)

# Unavailable tools are listed but not selectable (a second valid tool is added just here).
$script:OrqTools.executor['aider'] = $null
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Wizard (@('Enter') * 3 + @('Down', 'Enter') + @('Esc', 'Esc', 'Esc', 'Esc', 'Esc', 'Esc'))
$script:OrqTools.executor.Remove('aider')
$toolText = $said -join "`n"
Check 'W27' 'tool choice lists unavailable tool with a short reason; it is skipped and not pickable' ($toolText -match 'Executor: ferramenta - etapa 3/4' -and $toolText -match '- codex \(indisponivel\)' -and $toolText -match 'ferramenta: aider' -and $toolText -notmatch 'Executor: modelo [^
]*ferramenta: codex' -and -not (Test-Path $cfgPath))
$all = $script:allSaid -join "`n"
Check 'W28' 'screens never mention execpolicy/forbidden/Codex versions' ($all -notmatch 'execpolicy|forbidden|0\.162' -and $all -cnotmatch 'INDISPONIVEL') ([regex]::Match($all, '.{0,40}(execpolicy|forbidden|0\.162).{0,40}').Value)

# Validation error at save keeps the file and the draft.
$gone = New-Repo 'ephemeral'
Save-OrqConfig (Cfg $gone 'codex' 'keep-me') $cfgPath
$before = [IO.File]::ReadAllText($cfgPath)
$script:acts['run:del'] = { Remove-Item -LiteralPath $gone -Recurse -Force }
$r = Run-Wizard (@('Enter') * 5 + @('run:del', 'Enter', 'Esc'))
Check 'W29' 'save error: short message, saved file untouched, nothing started' ($r -eq 'exit' -and [IO.File]::ReadAllText($cfgPath) -ceq $before -and (@($said -match '^ERRO: nao salvo\. project: a pasta nao existe')).Count -eq 1)

# Line fallback (redirected stdin / no console).
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Lines @('1', '', '', '', '', '2')
$saved = Read-OrqConfig $cfgPath
$lt = $said -join "`n"
Check 'W30' 'line mode: numbers/empty line, documented fallback, saves and exits' ($r -eq 'exit' -and $saved.project -eq $repo -and $lt -match 'Entrada por linhas' -and $lt -match '(?m)^> 1\) ' -and $lt -notmatch 'Setas:')
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Lines @('')
Check 'W31' 'line mode: closed input exits, nothing saved' ($r -eq 'exit' -and -not (Test-Path $cfgPath))
$r = Run-Lines @('', 'b', 'b', 'q')
Check 'W32' 'line mode: b goes back, q quits, nothing saved' ($r -eq 'exit' -and -not (Test-Path $cfgPath))
Set-Content -LiteralPath $cfgPath -Value '{ broken' -Encoding ascii
$r = Run-Lines @('q')
Check 'W33' 'invalid saved file: warns once, never starts, file kept' ($r -eq 'exit' -and (@($said -match '^AVISO: ')).Count -eq 1 -and (Get-Content $cfgPath -Raw) -match 'broken')
# Digits are ignored in key mode (state and selection kept); they still pick in line mode.
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Wizard @('2', '1', '9', '0', 'Esc')
$kt = $said -join "`n"
Check 'W35' 'key mode: digits do not select or advance; Esc still leaves without saving' ($r -eq 'exit' -and -not (Test-Path $cfgPath) -and (@($said -match 'etapa 2/4')).Count -eq 0 -and (@($said -match 'Tecla sem efeito')).Count -ge 1)
$r = Run-Wizard (@('3', 'Enter') * 1 + @('Enter') * 5)
$saved = Read-OrqConfig $cfgPath
Check 'W36' 'key mode: a stray digit keeps the choice; Enter confirms normally' ($r -eq 'start' -and $saved.project -eq $repo)
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Lines @('1', '', '', '', '', '2')
Check 'W37' 'line mode: numbers still select; cancel (b, q) saves nothing' ($r -eq 'exit' -and (Test-Path $cfgPath))
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$r = Run-Lines @('1', 'b', 'q')
Check 'W38' 'line mode: cancel does not save' ($r -eq 'exit' -and -not (Test-Path $cfgPath))

$r = Run-Wizard (@('Enter') * 5 + @('Enter'))
$saved = Read-OrqConfig $cfgPath
Check 'W34' 'invalid saved file is replaced only on an explicit save' ($r -eq 'start' -and $saved.project -eq $repo)


function Start-Launcher([string[]]$cmdArgs, [string]$stdin, [string]$cwd) {
    Remove-Item "$root\session.txt", "$root\stop", "$root\args.txt", "$root\files.txt", "$root\frozen-copy.json", "$root\claude-args.txt" -ErrorAction SilentlyContinue
    $out = "$root\launcher-" + [Guid]::NewGuid().ToString('N').Substring(0, 8) + '.txt'
    [IO.File]::WriteAllText("$out.in", $stdin)
    $p = Start-Process cmd.exe -ArgumentList (@('/c', "`"$tool\strategist.cmd`"") + $cmdArgs) -WorkingDirectory $cwd -WindowStyle Hidden -PassThru `
        -RedirectStandardInput "$out.in" -RedirectStandardOutput $out -RedirectStandardError "$out.err"
    $null = $p.Handle
    [pscustomobject]@{ Proc = $p; Out = $out }
}
function Wait-Session($l, [int]$seconds = 60) {
    for ($i = 0; $i -lt $seconds * 5 -and -not (Test-Path "$root\session.txt") -and -not $l.Proc.HasExited; $i++) { Start-Sleep -Milliseconds 200 }
}
function End-Launcher($l) {
    Set-Content "$root\stop" 0
    if (-not $l.Proc.WaitForExit(60000)) { & cmd.exe /c "taskkill /PID $($l.Proc.Id) /T /F >nul 2>&1" }
    $l.Proc.ExitCode
}
function Delegate([string]$sess, [string]$task, [string]$cwd) {
    $ErrorActionPreference = 'Continue'
    Push-Location $cwd
    $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$sess\delegate-claude.ps1" $task 2>&1 | ForEach-Object { "$_" }
    Pop-Location
    [pscustomobject]@{ Exit = $LASTEXITCODE; Text = ($out -join "`n") }
}

# -Configure through the .cmd with stdin redirected: line mode, opens and leaves without a model.
Remove-Item -LiteralPath $cfgPath -Force -ErrorAction SilentlyContinue
$l = Start-Launcher @('-Configure') '' $repo
$null = $l.Proc.WaitForExit(60000)
$o1 = "$(Get-Content $l.Out -Raw -ErrorAction SilentlyContinue)"
Check 'I01' '-Configure with empty stdin: wizard opens, exits 0, no session, no file, no model' ($l.Proc.ExitCode -eq 0 -and -not (Test-Path "$root\session.txt") -and -not (Test-Path $cfgPath) -and $o1 -match 'Projeto - etapa 1/4' -and $o1 -notmatch 'forbidden') "exit=$($l.Proc.ExitCode)"
$l = Start-Launcher @('-Configure') "q`n" $repo
$null = $l.Proc.WaitForExit(60000)
Check 'I01b' '-Configure with q on stdin leaves without starting or saving' ($l.Proc.ExitCode -eq 0 -and -not (Test-Path "$root\session.txt") -and -not (Test-Path $cfgPath)) "exit=$($l.Proc.ExitCode)"

# Legacy: no argument, no config -> same launch as before (no -m / -c, no frozen file).
$l = Start-Launcher @() '' $repo
Wait-Session $l
$legacyArgs = "$(Get-Content "$root\args.txt" -ErrorAction SilentlyContinue)"
$legacyFiles = "$(Get-Content "$root\files.txt" -ErrorAction SilentlyContinue)"
$sess = "$(Get-Content "$root\session.txt" -ErrorAction SilentlyContinue)".Trim()
$d = Delegate $sess 'TAREFA: legado' $repo
$legacyClaude = "$(Get-Content "$root\claude-args.txt" -ErrorAction SilentlyContinue)"
$code = End-Launcher $l
Check 'I02' 'legacy start keeps the critical Codex flags and adds nothing' ($legacyArgs -match '--sandbox\|read-only\|--ask-for-approval\|on-request' -and $legacyArgs -notmatch '-m\||model_reasoning_effort' -and $legacyFiles -notmatch 'session-config' -and $code -eq 0) $legacyArgs
Check 'I03' 'legacy delegation: Claude flags exactly as before' ($d.Exit -eq 0 -and $legacyClaude -eq '-p|--permission-mode|auto|--permission-prompts|none|--disallowedTools|Bash(git push*)|PowerShell(git push*)|Bash(git reset --hard*)|PowerShell(git reset --hard*)|Bash(git clean*)|PowerShell(git clean*)|Bash(npm publish*)|PowerShell(npm publish*)|Bash(pnpm publish*)|PowerShell(pnpm publish*)') $legacyClaude

# Configured start through the wizard (6 empty lines = keep every step, then Salvar e iniciar), project with spaces, then freeze check.
Save-OrqConfig (Cfg $spaced 'codex' 'gpt-orq' 'low' 'claude' 'claude-exe' 'high') $cfgPath
$l = Start-Launcher @('-Configure') "`n`n`n`n`n`n`n" $repo   # 7: project, orch model, orch effort, exec tool, exec model, exec effort, review
Wait-Session $l
$cArgs = "$(Get-Content "$root\args.txt" -ErrorAction SilentlyContinue)"
$files = "$(Get-Content "$root\files.txt" -ErrorAction SilentlyContinue)"
$sess = "$(Get-Content "$root\session.txt" -ErrorAction SilentlyContinue)".Trim()
Check 'I04' 'wizard start: Codex gets -m, -c effort and keeps sandbox/approval' ($cArgs -match '--sandbox\|read-only\|--ask-for-approval\|on-request' -and $cArgs -match '\|-m\|gpt-orq' -and $cArgs -match '\|-c\|model_reasoning_effort=low') $cArgs
Check 'I05' 'frozen copy is in the session folder' ($files -match 'session-config\.json')
# The global file changes (and even breaks) after the start: delegations keep the frozen copy.
Save-OrqConfig (Cfg $repo 'codex' 'other' 'high' 'claude' 'claude-other' 'max') $cfgPath
$d1 = Delegate $sess 'TAREFA: um' $spaced
$a1 = "$(Get-Content "$root\claude-args.txt" -ErrorAction SilentlyContinue | Select-Object -Last 1)"
Set-Content -LiteralPath $cfgPath -Value '{ broken' -Encoding ascii
Remove-Item "$root\claude-args.txt"
$d2 = Delegate $sess 'TAREFA: dois' $spaced
$a2 = "$(Get-Content "$root\claude-args.txt" -ErrorAction SilentlyContinue | Select-Object -Last 1)"
Check 'I06' 'delegations use the frozen values, not the changed global file' ($d1.Exit -eq 0 -and $d2.Exit -eq 0 -and $a1 -match '--model\|claude-exe\|--effort\|high\|--disallowedTools' -and $a2 -eq $a1 -and $a1 -match '^-p\|--permission-mode\|auto\|--permission-prompts\|none\|') "$a1 // $a2"
Check 'I07' 'deny rules still present with model/effort' ($a1 -match 'Bash\(git push\*\)' -and $a1 -match 'PowerShell\(pnpm publish\*\)')
$code = End-Launcher $l
Check 'I08' 'session ends cleanly and the frozen file is removed with the session' ($code -eq 0 -and -not (Test-Path -LiteralPath $sess)) "exit=$code"

# Plain start (no flag) with a saved config uses it; an invalid one stops before Codex opens.
Save-OrqConfig (Cfg $repo 'codex' 'gpt-plain' '' 'claude' '' '') $cfgPath
$l = Start-Launcher @() '' $plain
Wait-Session $l
$cArgs = "$(Get-Content "$root\args.txt" -ErrorAction SilentlyContinue)"
$null = End-Launcher $l
Check 'I09' 'no flag + saved config: uses it even from a folder outside Git' ($cArgs -match '\|-m\|gpt-plain' -and $cArgs -notmatch 'model_reasoning_effort') $cArgs
Set-Content -LiteralPath $cfgPath -Value '{ broken' -Encoding ascii
$l = Start-Launcher @() '' $repo
$null = $l.Proc.WaitForExit(60000)
$err = "$(Get-Content "$($l.Out).err" -Raw -ErrorAction SilentlyContinue)$(Get-Content $l.Out -Raw -ErrorAction SilentlyContinue)"
Check 'I10' 'invalid saved file stops the start, Codex not opened' ($l.Proc.ExitCode -ne 0 -and -not (Test-Path "$root\session.txt") -and $err -match 'JSON ilegivel') "exit=$($l.Proc.ExitCode)"

# Old session without the frozen artifact still works (legacy delegation) and is cleaned.
$old = "$root\tmp\strategist-" + [Guid]::NewGuid().ToString('N')
New-Item -ItemType Directory -Path $old | Out-Null
$ErrorActionPreference = 'Continue'
$oldOut = 'TAREFA: sessao antiga' | & powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$tool\strategist.ps1" -Session $old -Project $repo 2>&1 | ForEach-Object { "$_" }
$d = [pscustomobject]@{ Exit = $LASTEXITCODE; Text = ($oldOut -join "`n") }
$ErrorActionPreference = 'Stop'
Check 'I11' 'session without session-config.json delegates as before' ($d.Exit -eq 0 -and $d.Text -match 'validation_status=passed') "exit=$($d.Exit)"

Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
"failures=$script:failures"
exit $(if ($script:failures) { 1 } else { 0 })

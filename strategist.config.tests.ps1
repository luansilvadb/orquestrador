# Focused, deterministic checks for the session configuration (strategist.config.ps1) and its
# integration in strategist.ps1. Simulated Codex and Claude, throwaway Git repositories, a
# throwaway LOCALAPPDATA: no model is called and the user's real config is never touched. Run:
#   powershell -NoProfile -ExecutionPolicy Bypass -File strategist.config.tests.ps1
# Keep this file ASCII-only (it runs in Windows PowerShell 5.1).
. "$PSScriptRoot\strategist.tests.common.ps1"
function Throws([scriptblock]$code) { try { & $code | Out-Null; $null } catch { "$($_.Exception.Message)" } }

. "$tool\strategist.config.ps1"
$cfgPath = Get-OrqConfigPath

$repo = New-Repo 'repo'
$spaced = New-Repo 'repo com espacos'
$plain = "$root\not-git"; New-Item -ItemType Directory -Path $plain, (Split-Path $cfgPath) | Out-Null

function Cfg([string]$project, [string]$oTool = 'codex', [string]$oModel = '', [string]$oEffort = '', [string]$eTool = 'claude', [string]$eModel = '', [string]$eEffort = '') {
    [pscustomobject]@{ project = $project
        orchestrator = [pscustomobject]@{ tool = $oTool; model = $oModel; effort = $oEffort }
        executor = [pscustomobject]@{ tool = $eTool; model = $eModel; effort = $eEffort } }
}
# The file is edited by hand: the tests write it the same way, with no validation on the way in.
function Save-Cfg($cfg) { [IO.File]::WriteAllText($cfgPath, ($cfg | ConvertTo-Json -Depth 4)) }

# --- configuration: storage and validation ---
Check 'C01' 'config path is under LOCALAPPDATA\orquestrador' ($cfgPath -eq "$root\appdata\orquestrador\config.json")
Check 'C02' 'absent file -> $null (the caller applies the defaults)' ($null -eq (Read-OrqConfig $cfgPath))
Save-Cfg (Cfg $repo 'codex' 'gpt-x' 'high' 'claude' 'claude-opus-x[1m]' 'max')
$back = Read-OrqConfig $cfgPath
Check 'C03' 'load keeps every field' ($back.project -eq $repo -and $back.orchestrator.model -eq 'gpt-x' -and $back.orchestrator.effort -eq 'high' -and $back.executor.model -eq 'claude-opus-x[1m]' -and $back.executor.effort -eq 'max')
Save-Cfg (Cfg $spaced)
$back = Read-OrqConfig $cfgPath
Check 'C05' 'folder with spaces loads and resolves to Git' ($back.project -eq $spaced -and (Get-OrqGitRoot $back.project) -eq $spaced)
$back2 = Cfg $repo
Check 'C06' 'empty model/effort stays empty and yields no extra args' ((Get-OrqClaudeArgs $back2.executor).Count -eq 0 -and (Get-OrqCodexArgs $back2.orchestrator).Count -eq 0)

Set-Content -LiteralPath $cfgPath -Value '{ not json' -Encoding ascii
$e = Throws { Read-OrqConfig $cfgPath }
Check 'C07' 'invalid JSON throws a useful message' ($e -match 'JSON ilegivel' -and $e -match [regex]::Escape($cfgPath)) $e
Save-Cfg (Cfg $repo 'codex' '' 'minimal')
$e = Throws { Read-OrqConfig $cfgPath }
Check 'C08' 'invalid value in file throws, no silent fallback' ($e -match "esforco 'minimal' invalido")  $e

Check 'C09' 'Codex as executor is accepted' (-not @(Test-OrqConfig (Cfg $repo 'codex' '' '' 'codex')).Count)
Check 'C10' 'Claude as orchestrator rejected' ((@(Test-OrqConfig (Cfg $repo 'claude')) -match "orchestrator: ferramenta 'claude' desconhecida").Count -eq 1)
Check 'C11' 'unknown tool rejected' ((@(Test-OrqConfig (Cfg $repo 'gemini')) -match "'gemini' desconhecida").Count -eq 1)

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

# --- integration: the launcher and its delegations, through strategist.cmd ---
# No config file: the defaults, over the repository of the cwd (no -m / -c, no model for Claude).
Remove-Item -LiteralPath $cfgPath -Force
Start-Session
$defaultArgs = "$(Get-Content "$root\args.txt" -ErrorAction SilentlyContinue)"
$d = Delegate 'padrao'
$defaultClaude = "$(Get-Content "$root\claude-args.txt" -ErrorAction SilentlyContinue)"
$code = Stop-Session
Check 'I02' 'no config: keeps the critical Codex flags and adds nothing' ($defaultArgs -match '--sandbox\|read-only\|--ask-for-approval\|on-request' -and $defaultArgs -notmatch '-m\||model_reasoning_effort' -and $code -eq 0) $defaultArgs
Check 'I03' 'no config: the delegation calls Claude with the fixed flags only' ($d.Exit -eq 0 -and $defaultClaude -eq '-p|--permission-mode|auto|--permission-prompts|none|--disallowedTools|Bash(git push*)|PowerShell(git push*)|Bash(git reset --hard*)|PowerShell(git reset --hard*)|Bash(git clean*)|PowerShell(git clean*)|Bash(npm publish*)|PowerShell(npm publish*)|Bash(pnpm publish*)|PowerShell(pnpm publish*)') $defaultClaude

# Saved config, project with spaces, started from another folder; then the freeze check.
Save-Cfg (Cfg $spaced 'codex' 'gpt-orq' 'low' 'claude' 'claude-exe' 'high')
Start-Session
$cArgs = "$(Get-Content "$root\args.txt" -ErrorAction SilentlyContinue)"
Check 'I04' 'saved config: Codex gets -m, -c effort and keeps sandbox/approval' ($cArgs -match '--sandbox\|read-only\|--ask-for-approval\|on-request' -and $cArgs -match '\|-m\|gpt-orq' -and $cArgs -match '\|-c\|model_reasoning_effort=low') $cArgs
Check 'I05' 'frozen copy is in the session folder' (Test-Path "$sess\session-config.json")
# The global file changes (and even breaks) after the start: delegations keep the frozen copy.
Save-Cfg (Cfg $repo 'codex' 'other' 'high' 'claude' 'claude-other' 'max')
$d1 = Delegate 'um'
$a1 = "$(Get-Content "$root\claude-args.txt" -ErrorAction SilentlyContinue | Select-Object -Last 1)"
Set-Content -LiteralPath $cfgPath -Value '{ broken' -Encoding ascii
Remove-Item "$root\claude-args.txt"
$d2 = Delegate 'dois'
$a2 = "$(Get-Content "$root\claude-args.txt" -ErrorAction SilentlyContinue | Select-Object -Last 1)"
Check 'I06' 'delegations use the frozen values, not the changed global file' ($d1.Exit -eq 0 -and $d2.Exit -eq 0 -and $a1 -match '--model\|claude-exe\|--effort\|high\|--disallowedTools' -and $a2 -eq $a1 -and $a1 -match '^-p\|--permission-mode\|auto\|--permission-prompts\|none\|') "$a1 // $a2"
Check 'I07' 'deny rules still present with model/effort' ($a1 -match 'Bash\(git push\*\)' -and $a1 -match 'PowerShell\(pnpm publish\*\)')
$code = Stop-Session
Check 'I08' 'session ends cleanly and the frozen file is removed with the session' ($code -eq 0 -and -not (Test-Path -LiteralPath $sess)) "exit=$code"

# A saved config is used even from a folder outside Git; an invalid one stops before Codex opens.
Save-Cfg (Cfg $repo 'codex' 'gpt-plain')
Start-Session -cwd $plain
$cArgs = "$(Get-Content "$root\args.txt" -ErrorAction SilentlyContinue)"
Stop-Session | Out-Null
Check 'I09' 'saved config: used even from a folder outside Git' ($cArgs -match '\|-m\|gpt-plain' -and $cArgs -notmatch 'model_reasoning_effort') $cArgs
Set-Content -LiteralPath $cfgPath -Value '{ broken' -Encoding ascii
try { Start-Session } catch { }
$err = "$(Get-Content "$launcherOut.err" -Raw -ErrorAction SilentlyContinue)$(Get-Content $launcherOut -Raw -ErrorAction SilentlyContinue)"
Check 'I10' 'invalid saved file stops the start, Codex not opened' ($proc.HasExited -and $proc.ExitCode -ne 0 -and -not (Test-Path "$root\session.txt") -and $err -match 'JSON ilegivel') "exit=$($proc.ExitCode)"

# Codex as executor: sandbox pinned on the call, rules ignored.
Save-Cfg (Cfg $repo 'codex' '' '' 'codex' 'gpt-exe' 'low')
Start-Session
$d = Delegate 'TAREFA: pelo codex'
$xArgs = "$(Get-Content "$root\codex-exec-args.txt" -ErrorAction SilentlyContinue)"
$xIn = "$(Get-Content "$root\codex-exec-stdin.txt" -Raw -ErrorAction SilentlyContinue)"
Stop-Session | Out-Null
Check 'I12' 'Codex executor: exec with explicit workspace-write, rules ignored, model/effort, prompt by stdin' ($d.Exit -eq 0 -and $d.Text -match 'validation_status=passed' -and $xArgs -eq 'exec|--sandbox|workspace-write|--ignore-rules|-m|gpt-exe|-c|model_reasoning_effort=low' -and $xIn -match 'TAREFA: pelo codex' -and -not (Test-Path "$root\claude-args.txt")) "exit=$($d.Exit) $xArgs"
Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
"failures=$script:failures"
exit $(if ($script:failures) { 1 } else { 0 })

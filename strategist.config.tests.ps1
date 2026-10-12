# Focused, deterministic checks for the session configuration (strategist.config.ps1) and its
# onboarding page, and their integration in strategist.ps1. Simulated Codex and Claude, throwaway Git repositories, a
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

function Cfg([string]$oTool = 'codex', [string]$oModel = '', [string]$oEffort = '', [string]$eTool = 'claude', [string]$eModel = '', [string]$eEffort = '') {
    [pscustomobject]@{
        orchestrator = [pscustomobject]@{ tool = $oTool; model = $oModel; effort = $oEffort }
        executor = [pscustomobject]@{ tool = $eTool; model = $eModel; effort = $eEffort } }
}
# Written as a hand edit would be, with no validation on the way in.
function Save-Cfg($cfg) { [IO.File]::WriteAllText($cfgPath, ($cfg | ConvertTo-Json -Depth 4)) }

# --- configuration: storage and validation ---
Check 'C01' 'config path is under LOCALAPPDATA\orquestrador' ($cfgPath -eq "$root\appdata\orquestrador\config.json")
$back = Read-OrqConfig $cfgPath
Check 'C02' 'absent file -> the defaults, no project field' (-not $back.PSObject.Properties['project'] -and $back.orchestrator.tool -eq 'codex' -and $back.executor.tool -eq 'claude' -and -not $back.executor.model)
Save-Cfg (Cfg 'codex' 'gpt-x' 'high' 'claude' 'claude-opus-x[1m]' 'max')
$back = Read-OrqConfig $cfgPath
Check 'C03' 'load keeps every field' ($back.orchestrator.model -eq 'gpt-x' -and $back.orchestrator.effort -eq 'high' -and $back.executor.model -eq 'claude-opus-x[1m]' -and $back.executor.effort -eq 'max')
$back2 = Cfg
Check 'C06' 'empty model/effort stays empty and yields no extra args' ((Get-OrqArgs $back2.executor).Count -eq 0 -and (Get-OrqArgs $back2.orchestrator).Count -eq 0)

Set-Content -LiteralPath $cfgPath -Value '{ not json' -Encoding ascii
$e = Throws { Read-OrqConfig $cfgPath }
Check 'C07' 'invalid JSON throws a useful message' ($e -match 'JSON ilegivel' -and $e -match [regex]::Escape($cfgPath)) $e
Save-Cfg (Cfg 'codex' '' 'minimal')
$e = Throws { Read-OrqConfig $cfgPath }
Check 'C08' 'invalid value in file throws, no silent fallback' ($e -match "esforco 'minimal' invalido")  $e

Check 'C09' 'Codex as executor is accepted' (-not @(Test-OrqConfig (Cfg 'codex' '' '' 'codex')).Count)
Check 'C10' 'Claude as orchestrator is accepted, with Claude or Codex as executor' (-not @(Test-OrqConfig (Cfg 'claude')).Count -and -not @(Test-OrqConfig (Cfg 'claude' '' '' 'codex')).Count)
Check 'C11' 'unknown tool rejected' ((@(Test-OrqConfig (Cfg 'gemini')) -match "'gemini' desconhecida").Count -eq 1)

$okEf = $true
foreach ($v in 'low', 'medium', 'high', 'xhigh', 'max') { if (@(Test-OrqConfig (Cfg 'codex' '' '' 'claude' '' $v) | Where-Object { $_ -match 'esforco' }).Count) { $okEf = $false } }
Check 'C12' 'valid Claude efforts accepted' $okEf
$okEf = $true
foreach ($v in 'low', 'medium', 'high', 'xhigh', 'max', 'ultra') { if (@(Test-OrqConfig (Cfg 'codex' '' $v) | Where-Object { $_ -match 'esforco' }).Count) { $okEf = $false } }
Check 'C13' 'valid Codex efforts accepted' $okEf
Check 'C14' 'Claude effort invalid for Claude (minimal) and case-sensitive' ((@(Test-OrqConfig (Cfg 'codex' '' '' 'claude' '' 'minimal')) -match 'esforco').Count -eq 1 -and (@(Test-OrqConfig (Cfg 'codex' '' '' 'claude' '' 'HIGH')) -match 'esforco').Count -eq 1)
Check 'C15' 'Codex effort invalid (minimal, not in installed catalog); Claude rejects ultra' ((@(Test-OrqConfig (Cfg 'codex' '' 'minimal')) -match 'esforco').Count -eq 1 -and (@(Test-OrqConfig (Cfg 'codex' '' '' 'claude' '' 'ultra')) -match 'esforco').Count -eq 1)

$badModels = '-evil', 'a b', 'x"y', '$(calc)', 'a;b', 'a|b', '`x'
$allBad = $true
foreach ($m in $badModels) { if (-not (@(Test-OrqConfig (Cfg 'codex' '' '' 'claude' $m)) -match 'modelo').Count) { $allBad = $false; $m } }
Check 'C16' 'model with spaces, quotes, shell syntax or leading dash rejected' $allBad
$a = Get-OrqArgs (Cfg 'codex' '' '' 'claude' 'my-model:v1[1m]' 'high').executor
Check 'C17' 'custom model is passed as a literal element' ($a.Count -eq 4 -and $a[0] -eq '--model' -and $a[1] -eq 'my-model:v1[1m]' -and $a[2] -eq '--effort' -and $a[3] -eq 'high')
$a = Get-OrqArgs (Cfg 'codex' 'gpt-5.x' 'low').orchestrator
Check 'C18' 'Codex args: -m model and -c model_reasoning_effort' ($a.Count -eq 4 -and $a[0] -eq '-m' -and $a[1] -eq 'gpt-5.x' -and $a[2] -eq '-c' -and $a[3] -eq 'model_reasoning_effort=low')
$a = Get-OrqArgs (Cfg 'codex' '' 'low').orchestrator
Check 'C19' 'roles are independent: only effort set -> no -m' ($a.Count -eq 2 -and $a[0] -eq '-c')

Set-Content -LiteralPath $cfgPath -Value '{ "version": 1, "project": "C:\\antigo", "executor": { "model": "so-o-modelo" } }' -Encoding ascii
$back = Read-OrqConfig $cfgPath
Check 'C24' 'partial file is laid over the defaults; a project saved by an older version is ignored' (-not $back.PSObject.Properties['project'] -and $back.orchestrator.tool -eq 'codex' -and $back.executor.tool -eq 'claude' -and $back.executor.model -eq 'so-o-modelo' -and -not $back.executor.effort)
Save-OrqConfig $cfgPath (Cfg 'claude' 'opus' 'max' 'codex' 'gpt-a' 'ultra')
$back = Read-OrqConfig $cfgPath
Check 'C25' 'saved answers load back' ($back.orchestrator.tool -eq 'claude' -and $back.orchestrator.model -eq 'opus' -and $back.orchestrator.effort -eq 'max' -and $back.executor.tool -eq 'codex' -and $back.executor.model -eq 'gpt-a' -and $back.executor.effort -eq 'ultra')
$cm = @(Get-OrqModels 'claude'); $xm = @(Get-OrqModels 'codex')
Check 'C26' 'Claude models are the aliases; Codex models come from the CLI, hidden ones left out, each with its efforts' ((($cm | ForEach-Object { $_.name }) -join ',') -eq 'fable,opus,sonnet,haiku' -and (($xm | ForEach-Object { $_.name }) -join ',') -eq 'gpt-a,gpt-luna' -and ($xm[1].efforts -join ',') -eq 'low,medium') "$(($xm | ForEach-Object { $_.name }) -join ',')"
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
Check 'I02' 'no config: keeps the critical Codex flags and adds nothing' ($defaultArgs -match '--sandbox\|read-only\|--ask-for-approval\|never' -and $defaultArgs -notmatch '-m\||model_reasoning_effort' -and $code -eq 0) $defaultArgs
Check 'I03' 'no config: the delegation calls Claude with the fixed flags only' ($d.Exit -eq 0 -and $defaultClaude -eq '-p|--permission-mode|auto|--permission-prompts|none|--disallowedTools|Bash(git push*)|PowerShell(git push*)|Bash(git reset --hard*)|PowerShell(git reset --hard*)|Bash(git clean*)|PowerShell(git clean*)|Bash(npm publish*)|PowerShell(npm publish*)|Bash(pnpm publish*)|PowerShell(pnpm publish*)') $defaultClaude

# Saved config, started inside a project with spaces in its path; then the freeze check.
Save-Cfg (Cfg 'codex' 'gpt-orq' 'low' 'claude' 'claude-exe' 'high')
Start-Session -cwd $spaced
$cArgs = "$(Get-Content "$root\args.txt" -ErrorAction SilentlyContinue)"
Check 'I04' 'saved config: Codex gets -m, -c effort and keeps sandbox/approval' ($cArgs -match '--sandbox\|read-only\|--ask-for-approval\|never' -and $cArgs -match '\|-m\|gpt-orq' -and $cArgs -match '\|-c\|model_reasoning_effort=low') $cArgs
Check 'I05' 'frozen copy is in the session folder, and the instructions name the project' ((Test-Path "$sess\session-config.json") -and (Get-Content "$sess\AGENTS.md" -Raw).Contains($spaced) -and (Get-Content "$sess\AGENTS.md" -Raw) -notmatch '\{\{')
# The global file changes (and even breaks) after the start: delegations keep the frozen copy.
Save-Cfg (Cfg 'codex' 'other' 'high' 'claude' 'claude-other' 'max')
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

# Outside a Git repository nothing starts, saved answers or not; an invalid file stops the start too.
Save-Cfg (Cfg 'codex' 'gpt-plain')
try { Start-Session -cwd $plain } catch { }
$err = "$(Get-Content "$launcherOut.err" -Raw -ErrorAction SilentlyContinue)$(Get-Content $launcherOut -Raw -ErrorAction SilentlyContinue)"
Check 'I09' 'folder outside Git: the start stops, Codex not opened, Git not initialized' ($proc.HasExited -and $proc.ExitCode -ne 0 -and -not (Test-Path "$root\session.txt") -and $err -match 'repositorio Git' -and -not (Test-Path "$plain\.git")) "exit=$($proc.ExitCode)"
Set-Content -LiteralPath $cfgPath -Value '{ broken' -Encoding ascii
try { Start-Session } catch { }
$err = "$(Get-Content "$launcherOut.err" -Raw -ErrorAction SilentlyContinue)$(Get-Content $launcherOut -Raw -ErrorAction SilentlyContinue)"
Check 'I10' 'invalid saved file stops the start, Codex not opened' ($proc.HasExited -and $proc.ExitCode -ne 0 -and -not (Test-Path "$root\session.txt") -and $err -match 'JSON ilegivel') "exit=$($proc.ExitCode)"

# Codex as executor: sandbox pinned on the call, rules ignored.
Save-Cfg (Cfg 'codex' '' '' 'codex' 'gpt-exe' 'low')
Start-Session
$d = Delegate 'TAREFA: pelo codex'
$xArgs = "$(Get-Content "$root\codex-exec-args.txt" -ErrorAction SilentlyContinue)"
$xIn = "$(Get-Content "$root\codex-exec-stdin.txt" -Raw -ErrorAction SilentlyContinue)"
Stop-Session | Out-Null
Check 'I12' 'Codex executor: exec with explicit workspace-write, rules ignored, model/effort, prompt by stdin' ($d.Exit -eq 0 -and $d.Text -match 'validation_status=passed' -and $xArgs -eq 'exec|--sandbox|workspace-write|--ignore-rules|-m|gpt-exe|-c|model_reasoning_effort=low' -and $xIn -match 'TAREFA: pelo codex' -and -not (Test-Path "$root\claude-args.txt")) "exit=$($d.Exit) $xArgs"

# --- the onboarding page, answered over HTTP ---
function Rules { @(Get-ChildItem "$root\codex-home\rules" -Filter 'strategist-*.rules' -ErrorAction SilentlyContinue) }
# Nothing saved: the page shows the defaults and the project, and sending them back saves them.
Remove-Item -LiteralPath $cfgPath -Force
Start-Session
$out = "$(Get-Content -LiteralPath $launcherOut -Raw)"; Stop-Session | Out-Null
$back = Read-OrqConfig $cfgPath
Check 'M01' 'first use: the page carries the project and the defaults, and sending them saves them' ((Test-Path -LiteralPath $cfgPath) -and $pageData.project -eq $repo -and $pageData.config.orchestrator.tool -eq 'codex' -and $pageData.config.executor.tool -eq 'claude' -and $posted[0].Status -eq 200 -and $back.orchestrator.tool -eq 'codex' -and $back.executor.tool -eq 'claude') "status=$($posted[0].Status)"
Check 'M02' 'nothing is asked in the terminal: the address, then the summary of what was saved' ($out -match '(?m)^Onboarding: http://127\.0\.0\.1:\d+/[0-9a-f]{32}/' -and $out.Contains("Projeto     : $repo") -and $out -match 'Orquestrador: codex' -and $out -notmatch 'Enter para iniciar')
$luna = @(@($pageData.tools | Where-Object { $_.name -eq 'codex' }).models | Where-Object { $_.name -eq 'gpt-luna' })
Check 'M04' 'the page gets each tool with its roles, efforts and models, each model with the efforts it accepts' ((@($pageData.tools | ForEach-Object { $_.name }) -join ',') -eq 'claude,codex' -and ($luna.efforts -join ',') -eq 'low,medium' -and $page -notmatch 'gpt-hidden' -and $page -notmatch '\{\{')
# Claude (opus, high) orchestrates, Codex (gpt-luna, medium) executes; the page then opens with these.
Start-Session -post (Cfg 'claude' 'opus' 'high' 'codex' 'gpt-luna' 'medium')
$oArgs = "$(Get-Content "$root\args.txt" -ErrorAction SilentlyContinue)"
$back = Read-OrqConfig $cfgPath
Check 'M03' 'the answers of the page are saved as the next default' ($back.orchestrator.tool -eq 'claude' -and $back.orchestrator.model -eq 'opus' -and $back.orchestrator.effort -eq 'high' -and $back.executor.tool -eq 'codex' -and $back.executor.model -eq 'gpt-luna' -and $back.executor.effort -eq 'medium') ($back | ConvertTo-Json -Depth 4 -Compress)
Check 'M05' 'Claude as orchestrator: restricted to reading and the launcher, nothing asked, no MCP, in the project folder' ($oArgs -eq "--restricted|--strict-mcp-config|--tools|Read,Glob,Grep,PowerShell|--settings|$sess\claude-settings.json|--permission-mode|dontAsk|--append-system-prompt-file|$sess\AGENTS.md|--model|opus|--effort|high" -and "$(Get-Content "$root\cwd.txt")" -eq $repo) $oArgs
$allow = @((Get-Content "$sess\claude-settings.json" -Raw | ConvertFrom-Json).permissions.allow)
Check 'M06' 'Claude as orchestrator: the only command allowed is this session''s launcher, and no Codex rule is written' (($allow -join '|') -eq "Read|Glob|Grep|PowerShell($sess\delegate.ps1 *)" -and -not (Rules).Count) ($allow -join '|')
$d = Delegate 'TAREFA: claude orquestra, codex executa'
$xArgs = "$(Get-Content "$root\codex-exec-args.txt" -ErrorAction SilentlyContinue | Select-Object -Last 1)"
Check 'M07' 'Claude orchestrator, Codex executor: the delegation runs with the answers' ($d.Exit -eq 0 -and $d.Text -match 'validation_status=passed' -and $xArgs -eq 'exec|--sandbox|workspace-write|--ignore-rules|-m|gpt-luna|-c|model_reasoning_effort=medium') "exit=$($d.Exit) $xArgs"
$code = Stop-Session 6
Check 'M08' 'Claude orchestrator: its exit code is passed on and the session folder is removed' ($code -eq 6 -and -not (Test-Path -LiteralPath $sess)) "exit=$code"
# Invalid answers go back to the page as errors and nothing starts; the valid ones that follow do. A typed model is kept.
Start-Session -post (Cfg 'gemini'), (Cfg 'claude' 'x y' 'high' 'claude' '' 'ultra'), (Cfg 'claude' 'meu-modelo:v1' 'high' 'claude')
$shown = $pageData.config; Stop-Session | Out-Null
$back = Read-OrqConfig $cfgPath
$refused = @(($posted[1].Text | ConvertFrom-Json).errors)
Check 'M09' 'invalid answers are refused with the reasons and the page keeps waiting; a typed model is kept' ($posted[0].Status -eq 400 -and "$(($posted[0].Text | ConvertFrom-Json).errors)" -match "'gemini' desconhecida" -and $posted[1].Status -eq 400 -and $refused.Count -eq 2 -and $posted[2].Status -eq 200 -and $back.orchestrator.model -eq 'meu-modelo:v1' -and $back.orchestrator.effort -eq 'high' -and $back.executor.tool -eq 'claude' -and -not $back.executor.model) "$(($posted | ForEach-Object { $_.Status }) -join ',') $($posted[1].Text)"
Check 'M10' 'the page opens with the answers saved by the start before it' ($shown.orchestrator.model -eq 'opus' -and $shown.executor.model -eq 'gpt-luna' -and $shown.executor.effort -eq 'medium')
Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
"failures=$script:failures"
exit $(if ($script:failures) { 1 } else { 0 })

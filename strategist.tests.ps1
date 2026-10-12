# Few, deterministic checks of what must not break: the four pairs start, the orchestrator can
# only delegate and run the global skill programs, a delegation returns the executor's answer and
# Git's list of changed files, the onboarding page saves the choices, and a dead session's
# leftovers are cleaned without touching a live one. Simulated Codex and Claude, throwaway Git
# repositories: no model is called and nothing of the user is touched. Run:
#   powershell -NoProfile -ExecutionPolicy Bypass -File strategist.tests.ps1
# Keep this file ASCII-only (it runs in Windows PowerShell 5.1).
. "$PSScriptRoot\strategist.tests.common.ps1"
. "$tool\strategist.config.ps1"
$cfgPath = Get-OrqConfigPath
$rulesDir = "$root\codex-home\rules"
$repo = New-Repo 'repo'
$plain = "$root\not-git"; New-Item -ItemType Directory -Path $plain | Out-Null

function Cfg([string]$oTool = 'codex', [string]$oModel = '', [string]$oEffort = '', [string]$eTool = 'claude', [string]$eModel = '', [string]$eEffort = '') {
    [pscustomobject]@{
        orchestrator = [pscustomobject]@{ tool = $oTool; model = $oModel; effort = $oEffort }
        executor = [pscustomobject]@{ tool = $eTool; model = $eModel; effort = $eEffort } }
}
function Rules { @(Get-ChildItem $rulesDir -Filter 'strategist-*.rules' -ErrorAction SilentlyContinue) }

# --- the four pairs: start, lock of the orchestrator, one delegation, clean end ---
foreach ($pair in @('codex', 'claude'), @('codex', 'codex'), @('claude', 'claude'), @('claude', 'codex')) {
    $o, $e = $pair; $id = "$o+$e"
    Start-Session -post (Cfg $o '' '' $e)
    $oArgs = "$(Get-Content "$root\args.txt" -ErrorAction SilentlyContinue)"
    if ($o -eq 'codex') {
        $rule = @(Get-Content "$rulesDir\$(Split-Path $sess -Leaf).rules" -ErrorAction SilentlyContinue)
        Check $id 'Codex orchestrator: read-only sandbox, nothing asked; the rule frees the launcher and the skill programs' ($oArgs -match '--sandbox\|read-only\|--ask-for-approval\|never' -and $rule.Count -eq 2 -and $rule[0].Contains("$sess\delegate.ps1".Replace('\', '\\')) -and $rule[1] -match '^prefix_rule\(pattern=\["node", \[' -and $rule[1].Contains($skillProgram.Replace('\', '\\'))) $oArgs
    }
    else {
        $allow = @((Get-Content "$sess\claude-settings.json" -Raw | ConvertFrom-Json).permissions.allow) -join '|'
        Check $id 'Claude orchestrator: restricted, no edit tool, global skills as plugin; only the launcher and the skill programs allowed; no Codex rule' ($oArgs -eq "--restricted|--strict-mcp-config|--tools|Read,Glob,Grep,PowerShell,Skill|--settings|$sess\claude-settings.json|--permission-mode|dontAsk|--append-system-prompt-file|$sess\AGENTS.md|--plugin-dir|$root\home\.agents|--add-dir|$root\home\.agents" -and $allow -eq "Read|Glob|Grep|Skill|PowerShell($sess\delegate.ps1 *)|PowerShell(node $skillProgram *)|PowerShell(node $($skillProgram.Replace('\', '/')) *)" -and -not (Rules).Count) "$oArgs // $allow"
    }
    $d = Delegate 'STEP 1'
    Check $id 'delegation: executor answer, exit code and Git list of changed files, closed by FIM' ($d.Exit -eq 0 -and $d.Text -match "fake $e( exec)?: feito" -and $d.Text -match 'executor_exit: 0' -and $d.Text -match '(?m)^\?\? progress\.txt' -and $d.Text.TrimEnd().EndsWith('--- FIM ---')) "exit=$($d.Exit) $($d.Text)"
    $code = Stop-Session 6
    Check $id 'end: the orchestrator exit code is passed on; session folder and rule removed' ($code -eq 6 -and -not (Test-Path -LiteralPath $sess) -and -not (Rules).Count) "exit=$code"
}

# --- delegation that goes wrong ---
Start-Session -post (Cfg 'codex' '' '' 'claude' 'claude-exe' 'high')
$d = Delegate 'BADFIX'
Check 'D1' 'failing executor: its exit code and answer come back, still closed by FIM' ($d.Exit -eq 5 -and $d.Text -match 'ERRO=compilacao' -and $d.Text -match 'executor_exit: 5 \(FALHOU\)' -and $d.Text.TrimEnd().EndsWith('--- FIM ---')) "exit=$($d.Exit)"
$d = Delegate '   '
Check 'D2' 'empty task: wrapper error, exit 4, executor not called again' ($d.Exit -eq 4 -and $d.Text -match 'wrapper_error: Task vazia' -and @(Get-Content "$root\calls.txt").Count -eq 1) "exit=$($d.Exit) $($d.Text)"
$a = "$(Get-Content "$root\claude-args.txt" | Select-Object -Last 1)"
Check 'D3' 'Claude executor: fixed flags, chosen model and effort, deny rules' ($a -match '^-p\|--permission-mode\|auto\|--permission-prompts\|none\|--model\|claude-exe\|--effort\|high\|--disallowedTools\|Bash\(git push\*\)' -and $a -match 'PowerShell\(pnpm publish\*\)') $a
Stop-Session | Out-Null

# --- the onboarding page ---
$back = Read-OrqConfig $cfgPath
Check 'P1' 'the choices sent by the page are saved as the next default' ($back.orchestrator.tool -eq 'codex' -and $back.executor.model -eq 'claude-exe' -and $back.executor.effort -eq 'high')
Start-Session -post (Cfg 'gemini'), (Cfg 'claude' 'opus' 'high' 'codex' 'gpt-luna' 'medium')
$back = Read-OrqConfig $cfgPath
Check 'P2' 'the page opens with the saved choices, the project and the models of each tool' ($pageData.project -eq $repo -and $pageData.config.executor.model -eq 'claude-exe' -and (@(@($pageData.tools | Where-Object { $_.name -eq 'codex' }).models | ForEach-Object { $_.name }) -join ',') -eq 'gpt-a,gpt-luna' -and $page -notmatch '\{\{')
Check 'P3' 'invalid choices are refused with the reason and the page keeps waiting; the valid ones start and are saved' ($posted[0].Status -eq 400 -and "$(($posted[0].Text | ConvertFrom-Json).errors)" -match "'gemini' desconhecida" -and $posted[1].Status -eq 200 -and $back.orchestrator.model -eq 'opus' -and $back.executor.tool -eq 'codex') "$(($posted | ForEach-Object { $_.Status }) -join ',')"
$xArgs = "$(Get-Content "$root\args.txt")"
Check 'P4' 'model and effort reach the orchestrator' ($xArgs -match '\|--model\|opus\|--effort\|high$') $xArgs
Stop-Session | Out-Null
try { Start-Session -cwd $plain } catch { }
$err = "$(Get-Content "$launcherOut.err" -Raw -ErrorAction SilentlyContinue)$(Get-Content $launcherOut -Raw -ErrorAction SilentlyContinue)"
Check 'P5' 'folder outside Git: the start stops and nothing opens' ($proc.HasExited -and $proc.ExitCode -ne 0 -and -not (Test-Path "$root\session.txt") -and $err -match 'repositorio Git') "exit=$($proc.ExitCode)"

# --- leftovers of a killed Codex session: removed only when the owner is provably gone ---
function New-Leftover([string]$owner) {
    $id = 'strategist-' + [Guid]::NewGuid().ToString('N'); $d = "$root\tmp\$id"
    New-Item -ItemType Directory -Path $d | Out-Null
    foreach ($f in 'AGENTS.md', 'delegate.cmd', 'delegate.ps1') { Set-Content "$d\$f" 'x' }
    [IO.File]::WriteAllText("$d\owner", "$owner $id")
    [IO.File]::WriteAllText("$rulesDir\$id.rules", "prefix_rule(pattern=[`"$("$d\delegate.ps1".Replace('\', '\\'))`"], decision=`"allow`", justification=`"Delegacao autorizada ao executor`")`nprefix_rule(pattern=[`"node`", [`"x`"]], decision=`"allow`")`n")
    $d
}
function Kept($d) { (Test-Path "$d\delegate.ps1") -and (Test-Path "$rulesDir\$(Split-Path $d -Leaf).rules") }
$alive = New-Leftover "$PID $((Get-Process -Id $PID).StartTime.ToFileTimeUtc())"   # owner running: this test process
$dead = New-Leftover '999996 123'                                                    # no such process
Set-Content "$rulesDir\outra.rules" 'prefix_rule(pattern=["git", "status"], decision="allow")'
Save-OrqConfig $cfgPath (Cfg)
Start-Session
Check 'L1' 'dead session: its folder and rule are removed at the next start' (-not (Test-Path $dead) -and -not (Test-Path "$rulesDir\$(Split-Path $dead -Leaf).rules"))
Check 'L2' 'live session and unrelated rule: untouched' ((Kept $alive) -and (Kept $sess) -and (Test-Path "$rulesDir\outra.rules"))
Stop-Session | Out-Null

Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
"failures=$script:failures"
exit $(if ($script:failures) { 1 } else { 0 })

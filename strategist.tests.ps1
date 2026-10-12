# Deterministic checks for strategist.ps1, with a simulated Codex and a simulated Claude in
# throwaway Git repositories. No model is called and nothing outside a temporary folder is
# touched. Run:  powershell -NoProfile -ExecutionPolicy Bypass -File strategist.tests.ps1
# Keep this file ASCII-only (it runs in Windows PowerShell 5.1).
#
# What it can and cannot show: the wrapper facts (numbering, stagnation brake, audit, exit
# codes, cleanup) are tested for real. The "loop" tests drive a scripted strategist that
# applies the decision order of strategist.md mechanically; they show that the report carries
# what those decisions need, not what a real model decides.
$ErrorActionPreference = 'Stop'
$tool = $PSScriptRoot
$root = Join-Path ([IO.Path]::GetTempPath()) ("strategist-tests-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path "$root\bin", "$root\codex-home\rules", "$root\tmp", "$root\outside" | Out-Null
$env:TEMP = "$root\tmp"; $env:TMP = "$root\tmp"   # sessions are created here, not in the real temporary folder

# Simulated Claude: acts on the current folder according to keywords in the task.
Set-Content "$root\bin\fake-claude.ps1" -Encoding ascii @'
if ($args -contains '--version') { '9.9.9 (fake)'; exit 0 }
if ($args -contains '--help') { '  -p, --print'; '  --permission-mode <mode> (choices: "acceptEdits", "auto")'; '  --permission-prompts <target>'; '  --disallowedTools <tools...>'; exit 0 }
$ms = New-Object IO.MemoryStream; [Console]::OpenStandardInput().CopyTo($ms)
$task = ([Text.Encoding]::UTF8.GetString($ms.ToArray()) -split 'TAREFA:')[1].Trim()
foreach ($try in 1..20) { try { Add-Content -LiteralPath "$env:MOCK_DIR\calls.txt" -Value $task -ErrorAction Stop; break } catch { Start-Sleep -Milliseconds 50 } }
$here = (Get-Location).Path
function Append($file, $text) { [IO.File]::AppendAllText("$here\$file", $text) }
switch -Regex ($task) {
    '^STEP (\d+)'  { Append 'progress.txt' "step $($Matches[1])`n" }
    'FIX'          { if ($task -notmatch 'BADFIX') { [IO.File]::WriteAllText("$here\ok.flag", "ok`n") } }
    'BREAK$'       { Remove-Item "$here\ok.flag" -ErrorAction SilentlyContinue; Append 'progress.txt' "changed`n" }
    '^TOUCH'       { Append 'noise.txt' "$task`n" }
    '^CHECK'       { if (Test-Path "$here\ok.flag") { 'TEST_RESULT=pass exit=0' } else { 'TEST_RESULT=fail exit=1' } }
    '^DIAG'        { 'DIAGNOSTICO=o teste falha porque ok.flag nao existe' }
    '^LIE'         { 'Concluido com sucesso: todos os testes passam.' }
    '^WS'          { Append 'tracked.txt' "bad   `n" }
    '^EDITPRE'     { Append 'pre.txt' "clean line`n" }
    # Exact bytes prepared by the test; then left on disk, staged, or committed (that path only).
    '^PUT (\S+) ?(ADD|COMMIT)?' {
        $name = $Matches[1]; $how = $Matches[2]
        [IO.File]::Copy("$env:MOCK_DIR\payload", "$here\$name", $true)
        if ($how) { & git -c core.safecrlf=false add -- $name }
        if ($how -eq 'COMMIT') { & git -c core.safecrlf=false commit -q -m put -- $name }
    }
    '^BREAKGIT'    { Rename-Item "$here\.git" '.git.off' }
    '^COMMIT'      { & git commit -q --allow-empty -m vazio }
    '^BRANCH'      { & git checkout -q -b outra }
    '^DETACH'      { & git checkout -q --detach }
    '^BADFIX'      { 'ERRO=compilacao falhou em modulo X'; exit 5 }
    '^DEADTOOL'    { 'ERRO=ferramenta indisponivel'; exit 9 }
    '^FLAKY'       { if (-not (Test-Path "$env:MOCK_DIR\flaky")) { Set-Content "$env:MOCK_DIR\flaky" x; 'ERRO=ferramenta indisponivel'; exit 9 } }
    '^CRASH'       { [Environment]::Exit(137) }
    '^SLOW'        { Start-Sleep -Seconds 6 }
}
'fake claude: feito'
exit 0
'@
Set-Content "$root\bin\claude.cmd" -Encoding ascii '@powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0fake-claude.ps1" %*'

# Simulated Codex: holds the session open until the test asks it to leave with a given code.
Set-Content "$root\bin\fake-codex.ps1" -Encoding ascii @'
if ($env:MOCK_CODEX -eq 'hang') { Start-Sleep -Seconds 600 }   # never shows the session
Set-Content "$env:MOCK_DIR\args.txt" ($args -join ' ')
Set-Content "$env:MOCK_DIR\session.txt" $args[[Array]::IndexOf($args, '-C') + 1]
while (-not (Test-Path "$env:MOCK_DIR\stop")) { Start-Sleep -Milliseconds 150 }
exit [int](Get-Content "$env:MOCK_DIR\stop")
'@
Set-Content "$root\bin\codex.cmd" -Encoding ascii '@powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0fake-codex.ps1" %*'

$env:MOCK_DIR = $root
$env:CODEX_HOME = "$root\codex-home"       # the session rule never reaches the real Codex folder
$env:GIT_CEILING_DIRECTORIES = $root       # Git never climbs out of the test folder
$env:PATH = "$root\bin;$env:PATH"
$git = (Get-Command git.exe).Source

$script:failures = 0
function Check([string]$id, [string]$name, [bool]$ok, [string]$detail = '') {
    if (-not $ok) { $script:failures++ }
    "{0} {1,-4} {2}{3}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $id, $name, $(if ($detail) { "  [$detail]" })
}

function New-Repo([string]$autocrlf = 'false') {
    $script:repo = "$root\repo-" + [Guid]::NewGuid().ToString('N').Substring(0, 8)
    New-Item -ItemType Directory -Path $repo | Out-Null
    & $git -C $repo init -q .; & $git -C $repo config user.email t@t; & $git -C $repo config user.name t; & $git -C $repo config core.autocrlf $autocrlf
    foreach ($f in 'tracked', 'pre') { [IO.File]::WriteAllText("$repo\$f.txt", "$f`n") }
    & $git -C $repo add -A; & $git -C $repo commit -q -m init
    [IO.File]::AppendAllText("$repo\pre.txt", "old bad   `n")     # whitespace that was there before any delegation
}

# Waiting with a limit exists only in these tests, so that a simulated process that hangs
# fails its test instead of blocking the suite. Nothing like it exists in strategist.ps1: a
# real delegation takes as long as it needs.
# Only a Process object this file started is ever ended, with its children: the object is
# that very instance, never a name and never a recycled PID.
function Stop-Mock($p) { if (-not $p.HasExited) { & cmd.exe /c "taskkill /PID $($p.Id) /T /F >nul 2>&1"; $p.WaitForExit(15000) | Out-Null } }
function Wait-Mock($p, [int]$seconds = 120) {
    if ($p.WaitForExit($seconds * 1000)) { return }
    Stop-Mock $p
    throw "o processo simulado PID $($p.Id) nao terminou em $seconds s e foi encerrado pelo teste"
}

function Launcher-Out { "$(Get-Content -LiteralPath $launcherOut -Raw -ErrorAction SilentlyContinue)" }
function Start-Session([string]$from = $tool, [int]$seconds = 60) {
    Remove-Item "$root\session.txt", "$root\stop", "$root\calls.txt", "$root\flaky" -ErrorAction SilentlyContinue
    $script:launcherOut = "$root\launcher-" + [Guid]::NewGuid().ToString('N').Substring(0, 8) + '.txt'   # what the launcher prints, warnings included
    $script:proc = Start-Process cmd.exe -ArgumentList '/c', "`"$from\strategist.cmd`"" -WorkingDirectory $repo -WindowStyle Hidden -PassThru -RedirectStandardOutput $launcherOut -RedirectStandardError "$launcherOut.err"
    $null = $proc.Handle   # without this, a redirected Start-Process loses the exit code in 5.1
    for ($i = 0; $i -lt ($seconds * 5) -and -not (Test-Path "$root\session.txt") -and -not $proc.HasExited; $i++) { Start-Sleep -Milliseconds 200 }
    if (-not (Test-Path "$root\session.txt")) { throw 'a sessao simulada nao abriu' }
    $script:sess = (Get-Content "$root\session.txt").Trim()
}

function Stop-Session([int]$code = 0) {
    Set-Content "$root\stop" $code
    Wait-Mock $proc
    $proc.ExitCode
}

function Delegate([string]$task) {
    $ErrorActionPreference = 'Continue'    # in 5.1, stderr of a native command would otherwise stop the test
    $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$sess\delegate-claude.ps1" $task 2>&1 | ForEach-Object { "$_" }
    New-Object psobject -Property @{ Exit = $LASTEXITCODE; Text = ($out -join "`n"); Task = $task }
}

# A field at the start of a line wins; only then one in the middle of a line (identical_attempts,
# pre_executor_failures, unresolved_attempts in a refusal), so explanatory text cannot pass for it.
function Field($r, [string]$name) { if ($r.Text -match "(?m)^$name[=:] ?(\S+)" -or $r.Text -match "(?m) $name[=:] ?(\S+)") { $Matches[1] } }
# The literal "name=" shows up once in the whole output, as the field itself: a reader that
# takes the first occurrence and one that takes the anchored line cannot disagree.
function Sole($r, [string]$name, [string]$value) { @([regex]::Matches($r.Text, "$name=")).Count -eq 1 -and $r.Text -match "(?m)^$name=$value( |$)" }
function Sole-Detail($r) { @($r.Text -split "`n" -match '(validation_status|executor_started)=' | ForEach-Object { $_.Substring(0, [Math]::Min(90, $_.Length)) }) -join ' / ' }

# Whitespace tests: the simulated Claude writes exactly these bytes to $name ("PUT"), and
# leaves them on disk, stages them (ADD) or commits that path (COMMIT). No file name here may
# hold a keyword of the simulated Claude (FIX, BREAK...): its switch runs every match.
function Put([string]$name, [string]$content, [string]$how = '') {
    [IO.File]::WriteAllText("$root\payload", $content)
    Delegate ("PUT $name $how".Trim())
}
# Checked and clean: something was verified, and no problem of any kind was listed.
function Ws-Clean($r) { (Field $r 'whitespace_check') -eq 'passed' -and (Field $r 'validation_status') -eq 'passed' -and $r.Exit -eq 0 -and $r.Text -notmatch 'nenhum arquivo alterado' -and $r.Text -notmatch 'problemas de whitespace' }
# Failed as before: same state, same exit code, and the evidence line Git printed.
function Ws-Failed($r, [string]$evidence) { (Field $r 'whitespace_check') -eq 'failed' -and (Field $r 'validation_status') -eq 'failed' -and $r.Exit -eq 3 -and $r.Text -match "(?m)^novo $evidence trailing whitespace\.$" }
function Ws-Detail($r) { "exit=$($r.Exit); " + (@($r.Text -split "`n" -match '^(whitespace_check|novo |origem |problemas de whitespace)') -join ' / ') }
function Calls { @(Get-Content "$root\calls.txt" -ErrorAction SilentlyContinue) }

# Scripted strategist. After each task that passes validation it asks for a separate CHECK; the
# goal is reached only when that check passes. A failure is retried once; the same error twice,
# or a refusal by the stagnation brake, moves on to the next item; no item left ends the loop
# with a diagnosis.
function Invoke-Loop([string[]]$plan) {
    $i = 0; $lastError = $null; $changes = 0; $diagnosis = 'sem tarefas'
    while ($i -lt $plan.Count) {
        $r = Delegate $plan[$i]
        if ($r.Text -match 'Estagnacao') { $i++; $changes++; continue }
        if ((Field $r 'validation_status') -ne 'passed') {
            $err = if ($r.Text -match '(?m)^ERRO=(.*)$') { $Matches[1].Trim() } else { Field $r 'validation_status' }
            $diagnosis = "falha: $err"
            if ($err -eq $lastError) { $i++; $changes++; $lastError = $null } else { $lastError = $err }
            continue
        }
        $lastError = $null
        $c = Delegate "CHECK depois de: $($plan[$i])"
        if ($c.Text -match 'TEST_RESULT=pass') { return New-Object psobject -Property @{ Goal = 'done'; Changes = $changes; Diagnosis = 'teste passou' } }
        $diagnosis = 'teste ainda falha'
        $i++
    }
    New-Object psobject -Property @{ Goal = 'blocked'; Changes = $changes; Diagnosis = $diagnosis }
}

try {
    "Windows PowerShell $($PSVersionTable.PSVersion); $(& $git --version)"
    Check 'T25' 'roda em Windows PowerShell 5.1 com Git for Windows' ($PSVersionTable.PSVersion.Major -eq 5 -and (& $git --version) -match 'windows')

    # --- numbering, concurrency, normal end ---
    New-Repo; Start-Session
    $numbers = @(); $ok12 = $false
    foreach ($n in 1..25) {
        $r = Delegate "STEP $n"
        $numbers += [int](Field $r 'delegation')
        if ($n -eq 12) { $ok12 = (Field $r 'validation_status') -eq 'passed' -and $numbers[-1] -eq 12 }
    }
    Check 'T3' '12 delegacoes com progresso, sem parada na oitava' $ok12
    Check 'T3b' 'a nona delegacao e aceita, sem heranca do limite antigo' ($numbers[8] -eq 9)
    Check 'T4' '25 delegacoes seguidas, contador correto' (($numbers -join ',') -eq ((1..25) -join ',') -and (Calls).Count -eq 25) ("ultima=$($numbers[-1])")
    $jobs = 1..2 | ForEach-Object { Start-Process powershell.exe -WindowStyle Hidden -PassThru -RedirectStandardOutput "$root\par-$_.txt" -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$sess\delegate-claude.ps1", "TOUCH-paralelo-$_" }
    $jobs | ForEach-Object { Wait-Mock $_ }
    $par = 1..2 | ForEach-Object { if ((Get-Content "$root\par-$_.txt" -Raw) -match '(?m)^delegation=(\d+)') { [int]$Matches[1] } }
    Check 'T15' 'duas delegacoes simultaneas, numeros unicos' (($par | Sort-Object) -join ',' -eq '26,27') ("numeros=$($par -join ',')")
    $rule = @(Get-ChildItem "$root\codex-home\rules" -Filter 'strategist-*.rules')
    $ruleText = if ($rule.Count -eq 1) { Get-Content $rule[0].FullName -Raw } else { '' }
    Check 'T21s' 'a regra da sessao nomeia so o launcher desta sessao (checagem estatica)' ($rule.Count -eq 1 -and @([regex]::Matches($ruleText, 'prefix_rule')).Count -eq 1 -and $ruleText.Contains("$sess\delegate-claude.ps1".Replace('\', '\\')))
    Check 'T20s' 'o Codex abre em sandbox somente leitura, sem bypass (checagem estatica)' ((Get-Content "$root\args.txt") -match '--sandbox read-only' -and (Get-Content "$root\args.txt") -notmatch 'bypass')
    $code = Stop-Session 0
    Check 'T23' 'encerramento normal remove pasta e regra da sessao' ($code -eq 0 -and -not (Test-Path $sess) -and -not @(Get-ChildItem "$root\codex-home\rules" -Filter 'strategist-*.rules').Count)

    # --- wrapper facts: executor failure, audit, whitespace, stagnation brake ---
    New-Repo; Start-Session
    $r = Delegate 'CRASH no meio'
    Check 'T16' 'executor interrompido: falha registrada' ($r.Exit -eq 137 -and (Field $r 'executor_exit') -eq '137' -and (Field $r 'validation_status') -eq 'failed')
    Check 'T16c' 'executor saiu com 137: validation_status e executor_started aparecem uma vez so, no campo real' ((Sole $r 'validation_status' 'failed') -and (Sole $r 'executor_started' 'yes')) (Sole-Detail $r)
    $r = Delegate 'WS novo'
    Check 'T16b' 'depois da falha a sessao continua, com o numero seguinte' ([int](Field $r 'delegation') -eq 2)
    Check 'T18' 'whitespace novo reprova a validacao' ((Field $r 'whitespace_check') -eq 'failed' -and (Field $r 'validation_status') -eq 'failed' -and $r.Exit -eq 3)
    $r = Delegate 'EDITPRE linha limpa'
    Check 'T19' 'whitespace preexistente nao e atribuido a delegacao' ((Field $r 'validation_status') -eq 'passed' -and $r.Text -match '0 novo\(s\), 0 de origem indeterminada, 1 preexistente')
    $a = Delegate 'DEADTOOL x'; $b = Delegate 'DEADTOOL x'; $before = (Calls).Count; $c = Delegate 'DEADTOOL x'
    Check 'T7w' 'freio de estagnacao: 1a e 2a tentativas identicas rodam, a 3a e recusada sem chamar o executor' ((Field $a 'identical_attempts') -eq '1' -and (Field $b 'identical_attempts') -eq '2' -and $c.Text -match 'Estagnacao' -and (Calls).Count -eq $before)
    $d = Delegate 'DEADTOOL com outra abordagem'
    Check 'T7x' 'tarefa diferente depois da recusa volta a rodar, com numero seguinte' ((Field $d 'identical_attempts') -eq '1' -and [int](Field $d 'delegation') -eq 7)
    $r = Delegate 'BREAKGIT'
    Rename-Item "$repo\.git.off" '.git'
    Check 'T17' 'auditoria incompleta nao vira validacao aprovada' ($r.Text -match 'snapshot_status=incomplete' -and (Field $r 'validation_status') -eq 'incomplete' -and $r.Exit -eq 2)
    $code = Stop-Session 9
    Check 'T24' 'encerramento apos falha do Codex: codigo repassado, pasta e regra removidas' ($code -eq 9 -and -not (Test-Path $sess) -and -not @(Get-ChildItem "$root\codex-home\rules" -Filter 'strategist-*.rules').Count)

    # --- a session killed from outside must not leave its rule in force for later sessions ---
    New-Repo; Start-Session
    $dead = $sess
    Stop-Mock $proc
    $left = (Test-Path $dead) -and @(Get-ChildItem "$root\codex-home\rules" -Filter 'strategist-*.rules').Count -eq 1
    Start-Session
    $rulesNow = @(Get-ChildItem "$root\codex-home\rules" -Filter 'strategist-*.rules' | ForEach-Object BaseName)
    Check 'T24b' 'sessao morta a forca: a proxima abertura remove a regra e a pasta que sobraram' ($left -and $rulesNow.Count -eq 1 -and $rulesNow[0] -eq (Split-Path $sess -Leaf) -and -not (Test-Path $dead)) ("sobrou antes=$left; regras agora=$($rulesNow.Count); pasta morta existe=$(Test-Path $dead)")
    Stop-Session 0 | Out-Null

    # --- how a delegation ends: complete report, wrapper failure, still running ---
    New-Repo; Start-Session
    $r = Delegate 'STEP 1'
    Check 'E1' 'fim normal: codigo 0 e relatorio completo' ($r.Exit -eq 0 -and $r.Text -match '(?m)^--- FIM ---$')
    $slow = Start-Process powershell.exe -WindowStyle Hidden -PassThru -RedirectStandardOutput "$root\slow.txt" -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$sess\delegate-claude.ps1", 'SLOW'
    Start-Sleep -Seconds 3
    $running = -not $slow.HasExited
    $partial = "$(Get-Content "$root\slow.txt" -Raw -ErrorAction SilentlyContinue)"
    Wait-Mock $slow
    Check 'E3' 'delegacao lenta: ativa e sem marcador no meio, marcador no fim' ($running -and $partial -notmatch 'FIM' -and (Get-Content "$root\slow.txt" -Raw) -match '(?m)^--- FIM ---')
    Delegate 'DEADTOOL y' | Out-Null; Delegate 'DEADTOOL y' | Out-Null
    $r = Delegate 'DEADTOOL y'
    Check 'E2' 'recusa do wrapper: codigo proprio e fim sem relatorio, nunca o marcador de relatorio completo' ($r.Exit -eq 4 -and $r.Text -match 'wrapper_error: Estagnacao' -and $r.Text -match '(?m)^--- FIM SEM RELATORIO ---$' -and $r.Text -notmatch '(?m)^--- FIM ---$') ("exit=$($r.Exit)")
    Check 'E2b' 'recusa do wrapper: o relatorio diz que o executor nao iniciou' ((Field $r 'executor_started') -eq 'no') ("executor_started=$(Field $r 'executor_started')")
    Check 'E2c' 'recusa pela contagem seguida: validation_status e executor_started aparecem uma vez so, no campo real' ((Sole $r 'validation_status' 'incomplete') -and (Sole $r 'executor_started' 'no')) (Sole-Detail $r)
    Rename-Item "$repo\.git" '.git.off'
    $r = Delegate 'STEP 2'
    Rename-Item "$repo\.git.off" '.git'
    Check 'E5' 'auditoria inicial falha: erro do wrapper, executor nao roda, fim sem relatorio' ($r.Exit -eq 4 -and $r.Text -match 'wrapper_error: Falha de auditoria Git antes' -and $r.Text -match '(?m)^--- FIM SEM RELATORIO ---$' -and (Calls) -notcontains 'STEP 2') ("exit=$($r.Exit)")
    $r = Delegate 'CRASH'
    Check 'E4' 'executor interrompido: falha registrada num relatorio completo' ($r.Exit -eq 137 -and (Field $r 'validation_status') -eq 'failed' -and $r.Text -match '(?m)^--- FIM ---$')
    Check 'E4b' 'executor interrompido: o relatorio diz que o executor iniciou' ((Field $r 'executor_started') -eq 'yes') ("executor_started=$(Field $r 'executor_started')")
    # HEAD is a commit and a branch: a checkout that lands on the same commit is still a change.
    # A commit on the same branch keeps the old line, hashes only: the reference did not change.
    $h0 = & $git -C $repo rev-parse HEAD
    $r = Delegate 'COMMIT vazio no mesmo branch'
    $h = & $git -C $repo rev-parse HEAD
    Check 'H0' 'so o commit muda, no mesmo branch: head_changes no formato antigo hashA -> hashB' ($h -ne $h0 -and $r.Text -match "(?m)^head_changes: $h0 -> $h$" -and (Field $r 'validation_status') -eq 'passed' -and $r.Exit -eq 0) ("$($r.Text -split "`n" -match '^head_changes')")
    $r = Delegate 'DIAG sem tocar em nada'
    Check 'H1' 'branch e commit iguais: head_changes e exatamente (nada)' ($r.Text -match '(?m)^head_changes: \(nada\)$') ("$($r.Text -split "`n" -match '^head_changes')")
    $r = Delegate 'BRANCH no mesmo commit'
    Check 'H2' 'troca de branch no mesmo commit aparece em head_changes, sem mudar a validacao' ($r.Text -match "(?m)^head_changes: $h \(refs/heads/\w+\) -> $h \(refs/heads/outra\)$" -and (Field $r 'validation_status') -eq 'passed' -and $r.Exit -eq 0) ("$($r.Text -split "`n" -match '^head_changes')")
    $r = Delegate 'DETACH no mesmo commit'
    Check 'H3' 'HEAD destacado no mesmo commit e identificado' ($r.Text -match "(?m)^head_changes: $h \(refs/heads/outra\) -> $h \(destacado\)$" -and (Field $r 'validation_status') -eq 'passed' -and $r.Exit -eq 0) ("$($r.Text -split "`n" -match '^head_changes')")
    Stop-Session 0 | Out-Null

    # --- the initial snapshot fails before the executor: counted by cause, never run, resumable ---
    # An untracked file held open with no sharing: Git lists it without opening it, and the
    # snapshot cannot hash it. Nothing here depends on timing. The second call is the audit that
    # strategist.md used to ask for: another task, the same snapshot, the same failure.
    New-Repo; Start-Session
    [IO.File]::WriteAllText("$repo\travado.txt", "em uso`n")
    $hold = [IO.File]::Open("$repo\travado.txt", 'Open', 'Read', 'None')
    try {
        $a = Delegate 'STEP 1'; $b = Delegate 'DIAG auditoria sem edicao do estado'; $c = Delegate 'STEP 1'
        Check 'P1' 'arquivo travado no retrato inicial: erro do wrapper, fim sem relatorio, executor nao iniciado' ($a.Exit -eq 4 -and $a.Text -match 'wrapper_error: Falha de auditoria Git antes' -and $a.Text -match 'snapshot_status=incomplete' -and (Field $a 'executor_started') -eq 'no' -and $a.Text -match '(?m)^--- FIM SEM RELATORIO ---$' -and $a.Text -notmatch '(?m)^--- FIM ---$' -and $a.Text -notmatch 'Estagnacao') ("exit=$($a.Exit); executor_started=$(Field $a 'executor_started')")
        Check 'P2' 'mesma falha do retrato inicial repetida: contada pela causa, mesmo com outra tarefa, e a terceira seguida e estagnacao' ((Field $a 'pre_executor_failures') -eq '1' -and (Field $b 'pre_executor_failures') -eq '2' -and $b.Text -notmatch 'Estagnacao' -and (Field $c 'pre_executor_failures') -eq '3' -and $c.Text -match 'Estagnacao' -and $c.Exit -eq 4 -and (Field $c 'executor_started') -eq 'no') ("contagem=$(Field $a 'pre_executor_failures'),$(Field $b 'pre_executor_failures'),$(Field $c 'pre_executor_failures')")
        Check 'P3' 'retrato inicial falhando: o executor nunca roda e o repositorio fica como estava' ((Calls).Count -eq 0 -and -not (Test-Path "$repo\progress.txt")) ("chamadas=$((Calls).Count)")
        # Another cause while the first one is still there: the count starts over.
        Rename-Item "$repo\.git" '.git.off'
        $e = Delegate 'STEP 1'
        Rename-Item "$repo\.git.off" '.git'
        Check 'P4' 'outra causa antes do executor: a contagem recomeca, sem estagnacao' ($e.Exit -eq 4 -and (Field $e 'pre_executor_failures') -eq '1' -and $e.Text -notmatch 'Estagnacao' -and (Field $e 'executor_started') -eq 'no' -and (Calls).Count -eq 0) ("contagem=$(Field $e 'pre_executor_failures')")
    }
    finally { $hold.Dispose() }
    $r = Delegate 'STEP 1'
    Check 'P5' 'causa removida: a mesma tarefa volta a rodar, com relatorio completo e executor iniciado' ($r.Exit -eq 0 -and (Field $r 'executor_started') -eq 'yes' -and (Field $r 'validation_status') -eq 'passed' -and (Field $r 'identical_attempts') -eq '1' -and [int](Field $r 'delegation') -eq 5 -and $r.Text -match '(?m)^--- FIM ---$' -and (Calls) -contains 'STEP 1') ("exit=$($r.Exit); delegacao=$(Field $r 'delegation')")
    $hold = [IO.File]::Open("$repo\travado.txt", 'Open', 'Read', 'None')
    try { $r = Delegate 'STEP 2' } finally { $hold.Dispose() }
    Check 'P6' 'a mesma causa volta depois de uma delegacao que rodou: a contagem recomeca' ($r.Exit -eq 4 -and (Field $r 'pre_executor_failures') -eq '1' -and $r.Text -notmatch 'Estagnacao' -and (Calls) -notcontains 'STEP 2') ("contagem=$(Field $r 'pre_executor_failures')")
    Stop-Session 0 | Out-Null

    # --- the brake across other delegations: what did not pass is counted by attempt, in a row or not ---
    # The audit in between is the same task over the same state too, but it passes: it is never
    # counted, and it does not bring the failed task back to 1.
    New-Repo; Start-Session
    $a1 = Delegate 'DEADTOOL a'; $q1 = Delegate 'DIAG auditoria'; $a2 = Delegate 'DEADTOOL a'; $q2 = Delegate 'DIAG auditoria'
    $before = (Calls).Count; $a3 = Delegate 'DEADTOOL a'
    Check 'R1' 'A falha, auditoria, A falha, auditoria: a terceira A e recusada sem chamar o executor' ((Field $a1 'unresolved_attempts') -eq '1' -and (Field $a2 'unresolved_attempts') -eq '2' -and (Field $a2 'identical_attempts') -eq '1' -and $a3.Exit -eq 4 -and $a3.Text -match 'wrapper_error: Estagnacao' -and (Field $a3 'unresolved_attempts') -eq '3' -and (Field $a3 'executor_started') -eq 'no' -and $a3.Text -match '(?m)^--- FIM SEM RELATORIO ---$' -and $a3.Text -notmatch '(?m)^--- FIM ---$' -and (Calls).Count -eq $before) ("contagem=$(Field $a1 'unresolved_attempts'),$(Field $a2 'unresolved_attempts'),$(Field $a3 'unresolved_attempts'); exit=$($a3.Exit)")
    Check 'R1b' 'recusa pela contagem de nao aprovadas: validation_status e executor_started aparecem uma vez so, no campo real' ((Field $a3 'validation_status') -eq 'incomplete' -and (Sole $a3 'validation_status' 'incomplete') -and (Sole $a3 'executor_started' 'no')) (Sole-Detail $a3)
    $q3 = Delegate 'DIAG auditoria'; $before = (Calls).Count; $a4 = Delegate 'DEADTOOL a'
    Check 'R2' 'validacao que passa, repetida entre outras tarefas: nunca contada nem recusada' ((Field $q1 'unresolved_attempts') -eq '1' -and (Field $q2 'unresolved_attempts') -eq '1' -and (Field $q3 'unresolved_attempts') -eq '1' -and (Field $q3 'identical_attempts') -eq '1' -and (Field $q3 'validation_status') -eq 'passed' -and $q3.Exit -eq 0) ("contagem=$(Field $q1 'unresolved_attempts'),$(Field $q2 'unresolved_attempts'),$(Field $q3 'unresolved_attempts')")
    Check 'R3' 'outra auditoria depois da recusa nao reabilita a tarefa' ($a4.Exit -eq 4 -and $a4.Text -match 'wrapper_error: Estagnacao' -and (Calls).Count -eq $before) ("exit=$($a4.Exit)")
    # Real change of state, one kind at a time: content on disk, then the index alone (git add of
    # that same content), then HEAD alone (the commit leaves disk and index entries as they
    # were). Each one is another attempt and starts at 1; the wrapper does not ask if it helped.
    Delegate 'TOUCH muda o conteudo' | Out-Null
    $c1 = Delegate 'DEADTOOL a'
    & $git -C $repo add noise.txt
    $c2 = Delegate 'DEADTOOL a'
    Delegate 'COMMIT vazio' | Out-Null
    $c3 = Delegate 'DEADTOOL a'
    Check 'R4' 'conteudo, indice ou HEAD mudou: a mesma tarefa volta a rodar, contando de 1' (@($c1, $c2, $c3 | Where-Object { (Field $_ 'unresolved_attempts') -eq '1' -and (Field $_ 'executor_started') -eq 'yes' -and $_.Exit -eq 9 -and $_.Text -match '(?m)^--- FIM ---$' }).Count -eq 3 -and @((Calls) -eq 'DEADTOOL a').Count -eq 5) ("contagem=$(Field $c1 'unresolved_attempts'),$(Field $c2 'unresolved_attempts'),$(Field $c3 'unresolved_attempts'); chamadas=$(@((Calls) -eq 'DEADTOOL a').Count)")
    Stop-Session 0 | Out-Null

    New-Repo; Start-Session
    $r = 1..2 | ForEach-Object { Delegate 'DEADTOOL a'; Delegate 'BADFIX b' }
    $before = (Calls).Count; $a = Delegate 'DEADTOOL a'; $b = Delegate 'BADFIX b'
    Check 'R5' 'A e B alternadas, as duas falhando: duas de cada rodam, a terceira de cada e recusada' ((($r | ForEach-Object { Field $_ 'unresolved_attempts' }) -join ',') -eq '1,1,2,2' -and (($r | ForEach-Object { Field $_ 'identical_attempts' }) -join ',') -eq '1,1,1,1' -and $a.Exit -eq 4 -and $a.Text -match 'wrapper_error: Estagnacao' -and $b.Exit -eq 4 -and $b.Text -match 'wrapper_error: Estagnacao' -and (Field $b 'executor_started') -eq 'no' -and (Calls).Count -eq $before -and $before -eq 4) ("contagem=$(($r | ForEach-Object { Field $_ 'unresolved_attempts' }) -join ','); chamadas=$((Calls).Count)")
    # The count in a row is what it was, passing or not: two run, the third is refused. That
    # refusal is not an attempt that failed, so later, after something else, the task runs.
    $v1 = Delegate 'DIAG v'; $v2 = Delegate 'DIAG v'; $before = (Calls).Count; $v3 = Delegate 'DIAG v'
    # Measured here, before the delegations below call the executor again and write other slots.
    $v3Calls = (Calls).Count; $v3Slot = @(Get-Content -LiteralPath "$sess\delegation-$([int](Field $v2 'delegation') + 1)" -ErrorAction SilentlyContinue)
    Delegate 'DIAG outra' | Out-Null
    $v4 = Delegate 'DIAG v'; Delegate 'DIAG outra' | Out-Null; $v5 = Delegate 'DIAG v'
    Check 'R6' 'validacao que passa, tres seguidas: o freio original recusa a terceira, pela contagem seguida' ((Field $v1 'identical_attempts') -eq '1' -and (Field $v2 'identical_attempts') -eq '2' -and (Field $v2 'validation_status') -eq 'passed' -and $v3.Exit -eq 4 -and $v3.Text -match 'tentada 3 vezes seguidas' -and $v3.Text -notmatch 'unresolved_attempts' -and (Field $v3 'executor_started') -eq 'no' -and $v3Calls -eq $before -and $v3Slot.Count -eq 2 -and $v3Slot[1] -eq 'refused') ("exit=$($v3.Exit); chamadas antes=$before depois=$v3Calls; slot=$($v3Slot[1])")
    Check 'R7' 'essa recusa nao conta como tentativa fracassada: a validacao volta a rodar depois de outra tarefa' (@($v4, $v5 | Where-Object { $_.Exit -eq 0 -and (Field $_ 'unresolved_attempts') -eq '1' -and (Field $_ 'identical_attempts') -eq '1' }).Count -eq 2) ("contagem=$(Field $v4 'unresolved_attempts'),$(Field $v5 'unresolved_attempts')")
    # A delegation that never got to record how it ended (killed, or still running): the slot
    # holds the attempt alone. Made here by taking the result off two slots that had passed.
    $w1 = Delegate 'DIAG w'; Delegate 'DIAG outra w' | Out-Null; $w2 = Delegate 'DIAG w'; Delegate 'DIAG outra w' | Out-Null
    foreach ($w in $w1, $w2) { $f = "$sess\delegation-$(Field $w 'delegation')"; [IO.File]::WriteAllText($f, @(Get-Content -LiteralPath $f)[0]) }
    $before = (Calls).Count; $w3 = Delegate 'DIAG w'
    Check 'R8' 'tentativa sem resultado registrado conta como nao aprovada: a terceira e recusada' ((Field $w2 'unresolved_attempts') -eq '1' -and $w3.Exit -eq 4 -and (Field $w3 'unresolved_attempts') -eq '3' -and (Field $w3 'executor_started') -eq 'no' -and (Calls).Count -eq $before) ("exit=$($w3.Exit); contagem=$(Field $w3 'unresolved_attempts')")
    Stop-Session 0 | Out-Null

    # --- whitespace: CRLF and Markdown hard breaks are not errors; real ones still fail ---
    # Same checks with Git converting line endings and with Git leaving them alone, over the
    # untracked file, the tracked file on disk, the index and a commit.
    foreach ($auto in 'false', 'true') {
        New-Repo $auto
        [IO.File]::WriteAllText("$repo\crlf.txt", "one`r`ntwo`r`n")
        & $git -C $repo -c core.safecrlf=false add -A; & $git -C $repo commit -q -m crlf
        Start-Session
        $r = Put 'novo-crlf.txt' "a`r`nb`r`n`r`nc`r`n"
        Check "L1$auto" "autocrlf=${auto}: arquivo novo nao rastreado em CRLF passa" (Ws-Clean $r) (Ws-Detail $r)
        $r = Put 'novo-lf.txt' "a`nb`n"
        Check "L2$auto" "autocrlf=${auto}: arquivo novo nao rastreado em LF passa" (Ws-Clean $r) (Ws-Detail $r)
        $a = Put 'tracked.txt' "tracked`r`nmais`r`n"; $b = Put 'crlf.txt' "one`r`ntwo`r`nthree`r`n"
        Check "L3$auto" "autocrlf=${auto}: rastreado reescrito em CRLF, e rastreado em CRLF com linha nova, passam" ((Ws-Clean $a) -and (Ws-Clean $b)) ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
        $r = Put 'ruim-crlf.txt' "a`r`nbad   `r`nc`r`n"
        Check "L4$auto" "autocrlf=${auto}: espacos antes do CRLF em nao rastreado continuam failed, so na linha errada" ((Ws-Failed $r '\[nao rastreado\] ruim-crlf\.txt:2:') -and $r.Text -match '1 novo\(s\)') (Ws-Detail $r)
        $r = Put 'crlf.txt' "one`r`ntwo`r`nthree`r`nbad `r`n"
        Check "L5$auto" "autocrlf=${auto}: espaco antes do CRLF em rastreado continua failed" ((Ws-Failed $r '\[working tree\] crlf\.txt:4:') -and $r.Text -match '1 novo\(s\)') (Ws-Detail $r)
        $a = Put 'idx-crlf.txt' "a`r`nb`r`n" 'ADD'; $b = Put 'idx-ruim.txt' "a`r`nbad   `r`n" 'ADD'
        Check "L6$auto" "autocrlf=${auto}: indice, CRLF passa e erro real continua failed" ((Ws-Clean $a) -and (Ws-Failed $b '\[index\] idx-ruim\.txt:2:') -and $b.Text -notmatch 'idx-ruim\.txt:1:') ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
        $a = Put 'ci-crlf.txt' "a`r`nb`r`n" 'COMMIT'; $b = Put 'ci-ruim.txt' "a`r`nbad   `r`n" 'COMMIT'
        Check "L7$auto" "autocrlf=${auto}: commit, CRLF passa e erro real continua failed" ((Ws-Clean $a) -and $a.Text -notmatch '(?m)^head_changes: \(nada\)$' -and (Ws-Failed $b '\[commit\] ci-ruim\.txt:2:') -and $b.Text -match '1 novo\(s\)') ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
        Stop-Session 0 | Out-Null
    }

    # Markdown, the credit for what was already there, and the repository's own policy.
    New-Repo
    [IO.File]::WriteAllText("$repo\.gitattributes", "rigido.md whitespace=trailing-space`nrigido.txt whitespace=trailing-space`nsolto.txt -whitespace`n")
    [IO.File]::WriteAllText("$repo\antigo.md", "p`r`n")
    & $git -C $repo add .gitattributes antigo.md; & $git -C $repo commit -q -m md
    [IO.File]::WriteAllText("$repo\antigo.md", "p`r`nold bad   `r`nold quebra  `r`n")    # tracked: one real problem and one hard break, before any delegation
    [IO.File]::WriteAllText("$repo\solta.md", "quebra  `nold bad   `n")                  # untracked: the same pair
    Start-Session
    $a = Put 'quebra.md' "linha um  `nlinha dois`n"; $b = Put 'quebra-crlf.md' "linha um  `r`nlinha dois`r`n"
    Check 'M1' 'Markdown: quebra dura (dois espacos apos conteudo) passa, em LF e em CRLF' ((Ws-Clean $a) -and (Ws-Clean $b)) ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
    $r = Put 'ruim.md' "tres   `num `ntab`t`n  `nmista`t  `nfim`n"
    Check 'M2' 'Markdown: tres espacos, um espaco, tab, linha so de espacos e tab antes dos dois espacos continuam failed' ((Ws-Failed $r '\[nao rastreado\] ruim\.md:1:') -and $r.Text -match '5 novo\(s\)' -and @(2..5 | Where-Object { $r.Text -match "ruim\.md:${_}: trailing whitespace" }).Count -eq 4) (Ws-Detail $r)
    $r = Put 'misto.md' "quebra  `nruim   `nfim`n"
    Check 'M3' 'Markdown: quebra valida e erro no mesmo arquivo, so o erro e apontado' ((Ws-Failed $r '\[nao rastreado\] misto\.md:2:') -and $r.Text -match '1 novo\(s\)' -and $r.Text -notmatch 'misto\.md:1:') (Ws-Detail $r)
    $r = Put 'dois.txt' "x  `n"
    Check 'M4' 'dois espacos finais fora de Markdown continuam failed' (Ws-Failed $r '\[nao rastreado\] dois\.txt:1:') (Ws-Detail $r)
    $a = Put 'idx.md' "quebra  `r`nfim`r`n" 'ADD'; $b = Put 'idx-misto.md' "quebra  `nruim   `n" 'ADD'
    Check 'M5' 'Markdown no indice: quebra passa, erro no mesmo arquivo continua failed' ((Ws-Clean $a) -and (Ws-Failed $b '\[index\] idx-misto\.md:2:') -and $b.Text -notmatch 'idx-misto\.md:1:') ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
    $a = Put 'ci.md' "quebra  `nfim`n" 'COMMIT'; $b = Put 'ci-misto.md' "quebra  `nruim   `n" 'COMMIT'
    Check 'M6' 'Markdown em commit: quebra passa, erro no mesmo arquivo continua failed' ((Ws-Clean $a) -and (Ws-Failed $b '\[commit\] ci-misto\.md:2:') -and $b.Text -match '1 novo\(s\)' -and $b.Text -notmatch 'ci-misto\.md:1:') ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
    $r = Put 'antigo.md' "p`r`nold bad   `r`nold quebra  `r`nlimpa`r`n"
    Check 'M7' 'rastreado: erro preexistente segue creditado e a quebra preexistente nao conta' ((Field $r 'validation_status') -eq 'passed' -and $r.Exit -eq 0 -and $r.Text -match '0 novo\(s\), 0 de origem indeterminada, 1 preexistente') (Ws-Detail $r)
    $r = Put 'solta.md' "quebra  `nold bad   `n" 'ADD'
    Check 'M8' 'nao rastreado antigo que so entrou no stage: erro preexistente creditado pelo disco' ((Field $r 'validation_status') -eq 'passed' -and $r.Exit -eq 0 -and $r.Text -match '0 novo\(s\), 0 de origem indeterminada, 1 preexistente') (Ws-Detail $r)
    # Explicit policy wins both ways: an attribute that asks for more, one that asks for nothing.
    $a = Put 'rigido.md' "quebra  `nfim`n"; $b = Put 'rigido.txt' "a`r`nb`r`n"
    Check 'G1' 'atributo whitespace explicito: quebra de Markdown e CR voltam a ser erro nesse caminho' ((Ws-Failed $a '\[nao rastreado\] rigido\.md:1:') -and (Ws-Failed $b '\[nao rastreado\] rigido\.txt:1:') -and $b.Text -match '2 novo\(s\)') ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
    $r = Put 'solto.txt' "x   `n"
    Check 'G2' 'atributo -whitespace explicito: o que o projeto liberou nao e reprovado' (Ws-Clean $r) (Ws-Detail $r)
    & $git -C $repo config core.whitespace trailing-space
    $a = Put 'cfg.md' "quebra  `nfim`n"; $b = Put 'cfg.txt' "a`r`nb`r`n"
    Check 'G3' 'core.whitespace explicito: vale o veredito do Git, sem a adaptacao local' ((Ws-Failed $a '\[nao rastreado\] cfg\.md:1:') -and (Ws-Failed $b '\[nao rastreado\] cfg\.txt:1:')) ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
    Check 'G4' 'a auditoria nao escreve politica: .gitattributes e a configuracao ficam como o teste deixou' ((& $git -C $repo config --get core.whitespace) -eq 'trailing-space' -and -not (& $git -C $repo status --porcelain -- .gitattributes))
    Stop-Session 0 | Out-Null

    # --- the whitespace policy cannot be read: never passed, with or without files touched ---
    # A simulated Git ahead of the real one in the PATH of this test, for this block only: it
    # fails "config --get core.whitespace" or "check-attr" while a marker file exists and hands
    # everything else to the real Git. No configuration is written anywhere. "pre" and "post" are
    # the phases of one delegation: before and after the simulated Claude wrote calls.txt, which
    # Git-Fails removes, so it is called before every delegation.
    Set-Content "$root\bin\git.cmd" -Encoding ascii @"
@echo off
if "%~1"=="config" if "%~2"=="--get" if "%~3"=="core.whitespace" (
  if exist "$root\gitfail-config" exit 3
  if exist "$root\gitfail-config-pre" if not exist "$root\calls.txt" exit 3
  if exist "$root\gitfail-config-post" if exist "$root\calls.txt" exit 3
)
if "%~1"=="check-attr" (
  if exist "$root\gitfail-checkattr-pre" if not exist "$root\calls.txt" exit 128
  if exist "$root\gitfail-checkattr-post" if exist "$root\calls.txt" exit 128
)
"$git" %*
"@
    function Git-Fails([string]$marker) {
        Remove-Item "$root\gitfail-*"; Remove-Item "$root\calls.txt" -ErrorAction SilentlyContinue
        if ($marker) { Set-Content "$root\gitfail-$marker" x }
    }
    # Not checked: a complete report whose three states agree, and the cause in the field.
    function Ws-Unknown($r, [string]$cause) { (Field $r 'whitespace_check') -eq 'unavailable' -and (Sole $r 'validation_status' 'incomplete') -and $r.Exit -eq 2 -and (Field $r 'executor_started') -eq 'yes' -and $r.Text -match "(?m)^whitespace_check=unavailable \(.*$cause" -and $r.Text -notmatch 'nenhum arquivo alterado' -and $r.Text -match '(?m)^--- FIM ---$' }
    $noConfig = 'git config core\.whitespace saiu com 3'; $noAttr = 'git check-attr whitespace saiu com 128'
    New-Repo
    [IO.File]::WriteAllText("$repo\velho.md", "velha`n")
    & $git -C $repo add velho.md; & $git -C $repo commit -q -m velho
    Start-Session
    Git-Fails; $a = Put 'w0.md' "um  `nfim`n"; Git-Fails; $b = Delegate 'DIAG w0 nada muda'
    Check 'W0' 'controle: com o Git simulado sem falha, quebra dura nova passa e delegacao sem alteracao passa' ((Ws-Clean $a) -and (Field $b 'whitespace_check') -eq 'passed' -and (Sole $b 'validation_status' 'passed') -and $b.Exit -eq 0 -and $b.Text -match 'nenhum arquivo alterado') ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
    Git-Fails 'config'; $a = Delegate 'DIAG w1 nada muda'; Git-Fails 'config-pre'; $b = Delegate 'DIAG w1 nada muda, so a fase inicial'
    Check 'W1' 'git config falha na checagem inicial e nada e alterado: unavailable, incomplete e codigo 2, nunca passed' ((Ws-Unknown $a "checagem inicial falhou: $noConfig") -and (Ws-Unknown $b "checagem inicial falhou: $noConfig") -and $b.Text -match '(?m)^\(nada\)$') ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
    Git-Fails 'config-pre'; $r = Put 'w2.txt' "a`n"
    Check 'W2' 'git config falha so na checagem inicial, arquivo novo limpo: incomplete, a checagem final inteira nao a apaga' (Ws-Unknown $r "checagem inicial falhou: $noConfig") (Ws-Detail $r)
    Git-Fails 'config-pre'; $a = Put 'w3.txt' "bad   `n"; Git-Fails 'config-pre'; $b = Put 'tracked.txt' "tracked`nbad   `n"
    Check 'W3' 'git config falha so na checagem inicial: erro em arquivo que nao existia segue novo e failed; em rastreado, origem indeterminada e incomplete' ((Ws-Failed $a '\[nao rastreado\] w3\.txt:1:') -and (Ws-Unknown $b "problema de origem indeterminada; checagem inicial falhou: $noConfig") -and $b.Text -match '(?m)^origem indeterminada \[working tree\] tracked\.txt:2: trailing whitespace\.$' -and $b.Text -match '0 novo\(s\), 1 de origem indeterminada') ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
    Git-Fails 'config-post'; $a = Put 'w4.txt' "a`n"; Git-Fails 'config-post'; $b = Delegate 'DIAG w4 nada muda'
    Check 'W4' 'git config falha so na checagem final: arquivo novo fica incomplete; sem alteracao a consulta nao e feita e nao vira erro' ((Ws-Unknown $a $noConfig) -and $a.Text -notmatch 'checagem inicial falhou' -and (Field $b 'whitespace_check') -eq 'passed' -and (Sole $b 'validation_status' 'passed') -and $b.Exit -eq 0) ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
    Git-Fails 'checkattr-post'; $a = Put 'w5.md' "um  `nfim`n"; Git-Fails 'checkattr-post'; $b = Put 'w5-limpo.md' "um`nfim`n"
    Check 'W5' 'check-attr falha na checagem final: quebra dura nova fica incomplete; .md sem quebra nao o consulta e passa' ((Ws-Unknown $a $noAttr) -and (Ws-Clean $b)) ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
    # Up to here nothing tracked holds a hard break, so the initial check never asks check-attr.
    Git-Fails 'checkattr-pre'; $a = Delegate 'DIAG w6 nada muda, check-attr nao consultado'
    [IO.File]::WriteAllText("$repo\velho.md", "velha quebra  `n")     # tracked hard break, before the delegations below
    Git-Fails; $b = Delegate 'DIAG w6 nada muda, quebra dura antiga'
    Check 'W6' 'check-attr que falharia mas nao e consultado na checagem inicial, e quebra dura antiga com check-attr funcionando: passam' (@($a, $b | Where-Object { (Field $_ 'whitespace_check') -eq 'passed' -and (Sole $_ 'validation_status' 'passed') -and $_.Exit -eq 0 }).Count -eq 2) ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
    Git-Fails 'checkattr-pre'; $a = Delegate 'DIAG w7 nada muda'; Git-Fails 'checkattr-pre'; $b = Put 'w7.txt' "a`n"
    Check 'W7' 'check-attr falha na checagem inicial: sem alteracao e com arquivo novo limpo, unavailable, incomplete e codigo 2' ((Ws-Unknown $a "checagem inicial falhou: $noAttr") -and (Ws-Unknown $b "checagem inicial falhou: $noAttr")) ((Ws-Detail $a) + ' | ' + (Ws-Detail $b))
    Git-Fails
    Check 'W8' 'o Git simulado nao escreveu politica: core.whitespace segue sem valor no repositorio de teste' (-not (& $git -C $repo config --get core.whitespace))
    Stop-Session 0 | Out-Null
    Remove-Item "$root\bin\git.cmd"

    # --- leftovers: only what is provably ours and provably over is removed ---
    $tmp = "$root\tmp"; $rulesDir = "$root\codex-home\rules"
    function New-Leftover([string]$id, [string]$owner) {
        $d = "$tmp\$id"; New-Item -ItemType Directory -Path $d | Out-Null
        foreach ($f in 'AGENTS.md', 'delegate-claude.cmd', 'delegate-claude.ps1', 'delegation-1') { Set-Content "$d\$f" 'x' }
        if ($owner) { [IO.File]::WriteAllText("$d\owner", $owner.Replace('{id}', $id)) }
        [IO.File]::WriteAllText("$rulesDir\$id.rules", "prefix_rule(pattern=[`"$("$d\delegate-claude.ps1".Replace('\', '\\'))`"], decision=`"allow`", justification=`"Delegacao autorizada ao Claude`")`n")
        $d
    }
    function New-Id { 'strategist-' + [Guid]::NewGuid().ToString('N') }
    function Kept($d) { (Test-Path "$d\delegate-claude.ps1") -and (Test-Path "$rulesDir\$(Split-Path $d -Leaf).rules") }
    function Gone($d) { -not (Test-Path $d) -and -not (Test-Path "$rulesDir\$(Split-Path $d -Leaf).rules") }
    $myStart = (Get-Process -Id $PID).StartTime.ToFileTimeUtc()
    $unrelated = New-Leftover 'strategist-teste' $null; Set-Content "$unrelated\importante.txt" 'dados'
    $corrupt = New-Leftover (New-Id) 'lixo'
    $noOwner = New-Leftover (New-Id) $null
    $wrongId = New-Leftover (New-Id) "999996 123 strategist-00000000000000000000000000000000"
    $alive = New-Leftover (New-Id) "$PID $myStart {id}"        # owner running: this test process
    $reused = New-Leftover (New-Id) "$PID 123 {id}"            # same PID, another start time: another instance
    $unreadable = New-Leftover (New-Id) "0 123 {id}"           # a process whose start time cannot be read
    $dead = New-Leftover (New-Id) "999996 123 {id}"            # no such process
    $linked = New-Leftover (New-Id) "999996 123 {id}"
    Set-Content "$root\outside\precioso.txt" 'fora da sessao'
    & cmd.exe /c mklink /J "$linked\atalho" "$root\outside" | Out-Null
    Set-Content "$rulesDir\outra.rules" 'prefix_rule(pattern=["git", "status"], decision="allow")'
    New-Repo; Start-Session
    Check 'C7' 'pasta de nome parecido, sem metadados: preservada inteira' ((Kept $unrelated) -and (Test-Path "$unrelated\importante.txt"))
    Check 'C8' 'metadados corrompidos, ausentes ou de outra sessao: nada e apagado' ((Kept $corrupt) -and (Kept $noOwner) -and (Kept $wrongId))
    Check 'C9' 'sessao com dono ativo: preservada' (Kept $alive)
    Check 'C10' 'PID reutilizado por outra instancia: a sessao antiga nao conta como ativa e e removida' (Gone $reused)
    Check 'C11' 'processo dono impossivel de identificar: nada e apagado' (Kept $unreadable)
    Check 'C13' 'regra de execpolicy nao relacionada: preservada' (Test-Path "$rulesDir\outra.rules")
    Check 'C14' 'sessao propria e encerrada: removida na abertura seguinte' (Gone $dead)
    Check 'C15' 'juncao dentro da sessao nao e seguida: o que esta fora fica intacto' ((Test-Path "$root\outside\precioso.txt") -and -not (Test-Path "$rulesDir\$(Split-Path $linked -Leaf).rules") -and -not (Test-Path "$linked\delegate-claude.ps1"))
    $procA = $proc; $sessA = $sess
    Start-Session
    Check 'C12' 'duas sessoes ao mesmo tempo: uma nao remove a outra' ((Kept $sessA) -and (Kept $sess) -and $sessA -ne $sess)
    Stop-Session 0 | Out-Null; Wait-Mock $procA
    Check 'C6' 'encerramento normal das duas: pastas e regras removidas' ((Gone $sessA) -and (Gone $sess))
    if (Test-Path "$linked\atalho") { [IO.Directory]::Delete("$linked\atalho") }

    # --- failures while the session starts: nothing of the new session may be left behind ---
    function Sessions { @(Get-ChildItem $tmp -Directory -Filter 'strategist-*' | ForEach-Object Name) }
    function Rules { @(Get-ChildItem $rulesDir -Filter 'strategist-*.rules' -ErrorAction SilentlyContinue | ForEach-Object Name) }
    function Try-Start([string]$from = $tool, [int]$seconds = 60) { $script:base = Sessions; try { Start-Session $from $seconds; $true } catch { if (-not $proc.WaitForExit(10000)) { Stop-Mock $proc }; $false } }
    function Leaked { @(Sessions | Where-Object { $base -notcontains $_ }) }
    Get-ChildItem $tmp -Directory | ForEach-Object { Remove-Item -LiteralPath $_.FullName -Recurse -Force }
    Get-ChildItem $rulesDir | Remove-Item -Force

    # an old rule that cannot be read (held open by another process), and one with garbage in it
    $lockedRule = "$rulesDir\$(New-Id).rules"; Set-Content $lockedRule 'x'
    $garbage = New-Leftover (New-Id) "999996 123 {id}"; Set-Content "$rulesDir\$(Split-Path $garbage -Leaf).rules" 'conteudo que nao e o nosso'
    $locker = Start-Process powershell.exe -WindowStyle Hidden -PassThru -ArgumentList '-NoProfile', '-Command', "`$f = [IO.File]::Open('$lockedRule', 'Open', 'ReadWrite', 'None'); Start-Sleep -Seconds 120"
    Start-Sleep -Seconds 2
    New-Repo
    $opened = Try-Start
    $mine = if ($opened) { Split-Path $sess -Leaf }
    if ($opened) { $code = Stop-Session 0 }
    Stop-Mock $locker
    Check 'S2' 'regra antiga ilegivel: a sessao abre, a regra fica, e nada da sessao nova sobra' ($opened -and (Test-Path $lockedRule) -and -not (Leaked).Count) ("abriu=$opened; sobras=$((Leaked).Count)")
    Check 'S3' 'regra antiga com conteudo estranho: regra e pasta preservadas' (Kept $garbage)
    Remove-Item -LiteralPath $garbage -Recurse -Force; Get-ChildItem $rulesDir | Remove-Item -Force

    # the instructions file is missing: AGENTS.md cannot be written
    $broken = "$root\tool-sem-md"; New-Item -ItemType Directory -Path $broken | Out-Null
    Copy-Item "$tool\strategist.cmd", "$tool\strategist.ps1", "$tool\executor.md" $broken
    $opened = Try-Start $broken
    Check 'S5' 'falha ao escrever o AGENTS.md: erro, nenhuma pasta e nenhuma regra sobram' (-not $opened -and $proc.ExitCode -ne 0 -and -not (Leaked).Count -and -not (Rules).Count) ("abriu=$opened; exit=$($proc.ExitCode); sobras=$((Leaked).Count)")

    # the rules folder cannot be created ("rules" is a file)
    $badHome = "$root\home-ruim"; New-Item -ItemType Directory -Path $badHome | Out-Null; Set-Content "$badHome\rules" 'arquivo no lugar da pasta'
    $env:CODEX_HOME = $badHome
    $opened = Try-Start
    $env:CODEX_HOME = "$root\codex-home"
    Check 'S6' 'falha ao escrever a regra: erro, nenhuma pasta sobra e nenhuma regra parcial' (-not $opened -and $proc.ExitCode -ne 0 -and -not (Leaked).Count -and (Get-Content "$badHome\rules") -eq 'arquivo no lugar da pasta') ("abriu=$opened; exit=$($proc.ExitCode); sobras=$((Leaked).Count)")

    # codex is not there to be started
    $savedPath = $env:PATH
    New-Item -ItemType Directory -Path "$root\bin-sem-codex" | Out-Null
    Copy-Item "$root\bin\claude.cmd", "$root\bin\fake-claude.ps1" "$root\bin-sem-codex"
    $env:PATH = "$root\bin-sem-codex;$(Split-Path $git);$env:SystemRoot\System32;$env:SystemRoot\System32\WindowsPowerShell\v1.0"
    $opened = Try-Start
    $env:PATH = $savedPath
    Check 'S7' 'Codex indisponivel: erro, nenhuma pasta e nenhuma regra sobram' (-not $opened -and $proc.ExitCode -ne 0 -and -not (Leaked).Count -and -not (Rules).Count) ("abriu=$opened; exit=$($proc.ExitCode); sobras=$((Leaked).Count)")

    # a file of the session cannot be removed at the end
    New-Repo; Start-Session
    $hold = [IO.File]::Open("$sess\AGENTS.md", 'Open', 'Read', 'None')
    $code = Stop-Session 7
    $hold.Dispose()
    Check 'S8' 'erro na limpeza: codigo do Codex preservado, regra removida, so a pasta da propria sessao fica' ($code -eq 7 -and -not (Rules).Count -and ((Leaked) -join ',') -eq (Split-Path $sess -Leaf)) ("exit=$code; sobras=$((Leaked) -join ',')")
    Remove-Item -LiteralPath $sess -Recurse -Force

    # the session folder cannot even be listed when the session ends (listing denied on a
    # throwaway folder of this test, and allowed again right after)
    function Deny-List([string]$dir) { & icacls.exe $dir /deny "${env:USERNAME}:(RD)" | Out-Null }
    function Allow-List([string]$dir) { & icacls.exe $dir /remove:d $env:USERNAME | Out-Null }
    New-Repo; $base = Sessions; Start-Session
    Deny-List $sess
    $code = Stop-Session 7
    Allow-List $sess
    Check 'S9' 'pasta da sessao impossivel de listar no fim: codigo do Codex preservado e a pasta fica' ($code -eq 7 -and -not (Rules).Count -and ((Leaked) -join ',') -eq (Split-Path $sess -Leaf)) ("exit=$code")
    Remove-Item -LiteralPath $sess -Recurse -Force

    # an error of the start itself (Codex is missing) together with a cleanup that cannot
    # finish. Nothing here depends on timing: the temporary folder of these runs carries an
    # inheritable "deny delete", so whatever session folder is created in it cannot be
    # removed. Ten runs in a row, each one checked on its own.
    $noDelete = "$root\tmp-sem-apagar"; New-Item -ItemType Directory -Path $noDelete | Out-Null
    & icacls.exe $noDelete /deny "${env:USERNAME}:(OI)(CI)(DE,DC)" | Out-Null
    $env:TEMP = $noDelete; $env:TMP = $noDelete
    $env:PATH = "$root\bin-sem-codex;$(Split-Path $git);$env:SystemRoot\System32;$env:SystemRoot\System32\WindowsPowerShell\v1.0"
    Push-Location $repo
    $good = 0; $sample = ''
    foreach ($run in 1..10) {
        $had = @(Get-ChildItem $noDelete -Directory | ForEach-Object Name)
        $out = (& cmd.exe /c "`"$tool\strategist.cmd`" 2>&1" | ForEach-Object { "$_" }) -join "`n"; $code = $LASTEXITCODE
        $new = @(Get-ChildItem $noDelete -Directory | Where-Object { $had -notcontains $_.Name })
        if ($code -ne 0 -and $out -match "'codex'" -and $out -match 'Aviso: a pasta de sessao' -and $new.Count -eq 1 -and -not (Rules).Count) { $good++ }
        else { $sample = "exit=$code; original=$($out -match "'codex'"); aviso=$($out -match 'Aviso: a pasta de sessao'); pastas novas=$($new.Count)" }
    }
    Pop-Location
    $env:PATH = $savedPath; $env:TEMP = "$root\tmp"; $env:TMP = "$root\tmp"
    & icacls.exe $noDelete /remove:d $env:USERNAME | Out-Null
    Check 'S10' 'erro na abertura e limpeza impossivel, 10 vezes: o erro original aparece, a limpeza so avisa, a pasta fica' ($good -eq 10) ("$good de 10 $sample")
    Remove-Item -LiteralPath $noDelete -Recurse -Force

    # the session folder exists but cannot even be looked at when the session ends
    $denied = "$root\tmp-negado"; New-Item -ItemType Directory -Path $denied | Out-Null
    $env:TEMP = $denied; $env:TMP = $denied
    New-Repo; Start-Session
    $env:TEMP = "$root\tmp"; $env:TMP = "$root\tmp"
    & icacls.exe $sess /deny "${env:USERNAME}:(F)" | Out-Null
    & icacls.exe $denied /deny "${env:USERNAME}:(RD)" | Out-Null
    $code = Stop-Session 7
    & icacls.exe $denied /remove:d $env:USERNAME | Out-Null; & icacls.exe $sess /remove:d $env:USERNAME | Out-Null
    Check 'S14' 'pasta da sessao existe mas esta inacessivel: limpeza incompleta avisada, codigo preservado, nada apagado' ($code -eq 7 -and (Launcher-Out) -match 'Aviso: a pasta de sessao' -and (Test-Path "$sess\delegate-claude.ps1")) ("exit=$code; aviso=$((Launcher-Out) -match 'Aviso: a pasta de sessao')")
    Remove-Item -LiteralPath $denied -Recurse -Force

    # a leftover "session folder" that is really a junction to somewhere else
    $target = "$root\destino-da-juncao"; New-Item -ItemType Directory -Path $target | Out-Null
    $jid = New-Id
    foreach ($f in 'AGENTS.md', 'delegate-claude.cmd', 'delegate-claude.ps1') { Set-Content "$target\$f" 'fora da sessao' }
    [IO.File]::WriteAllText("$target\owner", "999996 123 $jid")
    & cmd.exe /c mklink /J "$tmp\$jid" $target | Out-Null
    [IO.File]::WriteAllText("$rulesDir\$jid.rules", "prefix_rule(pattern=[`"$("$tmp\$jid\delegate-claude.ps1".Replace('\', '\\'))`"], decision=`"allow`", justification=`"Delegacao autorizada ao Claude`")`n")
    New-Repo; Start-Session
    Check 'S15' 'pasta de sessao que e uma juncao: nao e seguida, o destino fica intacto e a limpeza incompleta e avisada' (@(Get-ChildItem $target -File).Count -eq 4 -and (Launcher-Out) -match 'juncao') ("arquivos no destino=$(@(Get-ChildItem $target -File).Count); aviso=$((Launcher-Out) -match 'juncao')")
    Stop-Session 0 | Out-Null
    [IO.Directory]::Delete("$tmp\$jid")
    Get-ChildItem $rulesDir | Remove-Item -Force

    # the simulated Codex hangs before the session shows up: the test must end, and end it
    $env:MOCK_CODEX = 'hang'
    New-Repo
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $opened = Try-Start $tool 8
    $env:MOCK_CODEX = ''
    $stuck = @(Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -like "*$root\bin\fake-codex.ps1*" }).Count
    Check 'S11' 'Codex simulado travado, sem marcador: o teste termina no prazo e encerra so o que ele criou' (-not $opened -and $sw.Elapsed.TotalSeconds -lt 40 -and $proc.HasExited -and $stuck -eq 0) ("abriu=$opened; tempo=$([int]$sw.Elapsed.TotalSeconds)s; processos restantes=$stuck")
    $abandoned = @(Leaked)    # the launcher was killed: its folder and rule are a legitimate leftover
    $quick = Start-Process cmd.exe -ArgumentList '/c', 'exit 3' -WindowStyle Hidden -PassThru
    Wait-Mock $quick 30; Stop-Mock $quick
    Check 'S12' 'processo que ja terminou: nada e encerrado e o codigo de saida fica' ($quick.ExitCode -eq 3)
    New-Repo; Start-Session
    Check 'S13' 'depois do prazo estourado a suite segue: a sessao seguinte abre e recolhe a sobra' ((Test-Path $sess) -and $abandoned.Count -eq 1 -and -not (Test-Path "$tmp\$($abandoned[0])")) ("sobras do teste anterior=$($abandoned.Count)")
    Stop-Session 0 | Out-Null

    # --- loop decisions, with the scripted strategist ---
    function Scenario([string[]]$plan, [switch]$StartsPassing) {
        New-Repo
        if ($StartsPassing) { [IO.File]::WriteAllText("$repo\ok.flag", "ok`n"); & $git -C $repo add -A; & $git -C $repo commit -q -m ok }
        Start-Session
        $result = Invoke-Loop $plan
        $result | Add-Member NoteProperty Calls (Calls) -PassThru
        Stop-Session 0 | Out-Null
    }
    $s = Scenario 'FIX'
    Check 'T1' 'objetivo concluido apos 1 delegacao de trabalho' ($s.Goal -eq 'done' -and @($s.Calls -notmatch '^CHECK').Count -eq 1)
    $s = Scenario 'STEP 1', 'STEP 2', 'FIX terceira'
    Check 'T2a' 'objetivo concluido apos 3 delegacoes de trabalho' ($s.Goal -eq 'done' -and @($s.Calls -notmatch '^CHECK').Count -eq 3)
    $s = Scenario 'STEP 1', 'STEP 2', 'STEP 3', 'STEP 4', 'FIX final'
    Check 'T2' 'objetivo concluido apos 5 delegacoes de trabalho' ($s.Goal -eq 'done' -and @($s.Calls -notmatch '^CHECK').Count -eq 5)
    $s = Scenario 'FIX cedo', 'STEP 2', 'STEP 3'
    Check 'T5' 'objetivo atendido antes do previsto: nao delega o resto' ($s.Goal -eq 'done' -and -not @($s.Calls -match '^STEP').Count)
    $s = Scenario 'STEP 1', 'FIX corretiva'
    Check 'T6' 'falha corrigivel gera delegacao corretiva' ($s.Goal -eq 'done' -and ($s.Calls -join '|') -match 'STEP 1\|CHECK.*\|FIX corretiva')
    $s = Scenario 'BADFIX primeira abordagem', 'FIX outra abordagem'
    Check 'T7' 'mesmo erro reaparece: muda de abordagem' ($s.Goal -eq 'done' -and $s.Changes -eq 1 -and @($s.Calls -match '^BADFIX').Count -eq 2)
    $s = Scenario 'BADFIX a', 'BADFIX b'
    Check 'T8' 'falhas repetidas sem alternativa: encerra sozinho com diagnostico' ($s.Goal -eq 'blocked' -and $s.Diagnosis -match 'compilacao falhou' -and @($s.Calls).Count -eq 4) ($s.Diagnosis)
    $s = Scenario 'TOUCH um', 'TOUCH dois'
    Check 'T9' 'arquivos mudam mas o teste segue falhando: nao conclui' ($s.Goal -eq 'blocked' -and $s.Diagnosis -eq 'teste ainda falha')
    $s = Scenario 'DIAG causa', 'FIX pela causa'
    Check 'T10' 'auditoria sem edicao conta e o loop segue ate o fim' ($s.Goal -eq 'done' -and $s.Changes -eq 0)
    $s = Scenario 'LIE'
    Check 'T11' 'Claude declara sucesso falso: a checagem separada impede a conclusao' ($s.Goal -eq 'blocked' -and $s.Diagnosis -eq 'teste ainda falha')
    $s = Scenario 'STEP 1 BREAK' -StartsPassing
    Check 'T12' 'mudanca provoca regressao: detectada' ($s.Goal -eq 'blocked' -and $s.Diagnosis -eq 'teste ainda falha')
    $s = Scenario 'FLAKY passo', 'FIX'
    Check 'T13' 'falha temporaria de ferramenta: uma nova tentativa e segue' ($s.Goal -eq 'done' -and @($s.Calls -match '^FLAKY').Count -eq 2 -and $s.Changes -eq 0)
    $s = Scenario 'DEADTOOL unica via'
    Check 'T14' 'falha permanente de ferramenta: encerra com diagnostico' ($s.Goal -eq 'blocked' -and $s.Diagnosis -match 'ferramenta indisponivel')

    'T20, T21, T22: dependem do Codex e do Claude reais (aprovacao, sandbox, recusa de operacao proibida); nao fazem parte desta suite.'
}
finally {
    if ($proc -and -not $proc.HasExited) { Set-Content "$root\stop" 1; $proc.WaitForExit(15000) | Out-Null }
    & cmd.exe /c "icacls `"$root`" /remove:d $env:USERNAME /T /C /Q >nul 2>&1"   # a test stopped halfway may have left a deny in place
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}
"falhas: $failures"
exit $failures

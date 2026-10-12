# Deterministic checks for strategist.ps1, with a simulated Codex and a simulated Claude in
# throwaway Git repositories. No model is called and nothing outside a temporary folder is
# touched. Run:  powershell -NoProfile -ExecutionPolicy Bypass -File strategist.tests.ps1
# Keep this file ASCII-only (it runs in Windows PowerShell 5.1).
$savedLocalAppData = $env:LOCALAPPDATA
. "$PSScriptRoot\strategist.tests.common.ps1"
New-Item -ItemType Directory -Path "$root\outside" | Out-Null

function Launcher-Out { "$(Get-Content -LiteralPath $launcherOut -Raw -ErrorAction SilentlyContinue)" }

# A field at the start of a line wins; only then one in the middle of a line (unresolved_attempts),
# so explanatory text cannot pass for it.
function Field($r, [string]$name) { if ($r.Text -match "(?m)^$name[=:] ?(\S+)" -or $r.Text -match "(?m) $name[=:] ?(\S+)") { $Matches[1] } }
# The literal "name=" shows up once in the whole output, as the field itself: a reader that
# takes the first occurrence and one that takes the anchored line cannot disagree.
function Sole($r, [string]$name, [string]$value) { @([regex]::Matches($r.Text, "$name=")).Count -eq 1 -and $r.Text -match "(?m)^$name=$value( |$)" }
function Sole-Detail($r) { @($r.Text -split "`n" -match '(validation_status|executor_started)=' | ForEach-Object { $_.Substring(0, [Math]::Min(90, $_.Length)) }) -join ' / ' }

function Calls { @(Get-Content "$root\calls.txt" -ErrorAction SilentlyContinue) }

try {
    "Windows PowerShell $($PSVersionTable.PSVersion); $(& $git --version)"
    Check 'T25' 'roda em Windows PowerShell 5.1 com Git for Windows' ($PSVersionTable.PSVersion.Major -eq 5 -and (& $git --version) -match 'windows')

    # --- numbering, concurrency, normal end ---
    $repo = New-Repo;Start-Session
    $numbers = @(); $ok12 = $false
    foreach ($n in 1..25) {
        $r = Delegate "STEP $n"
        $numbers += [int](Field $r 'delegation')
        if ($n -eq 12) { $ok12 = (Field $r 'validation_status') -eq 'passed' -and $numbers[-1] -eq 12 }
    }
    Check 'T3' '12 delegacoes com progresso, sem parada na oitava' $ok12
    Check 'T4' '25 delegacoes seguidas, contador correto' (($numbers -join ',') -eq ((1..25) -join ',') -and (Calls).Count -eq 25) ("ultima=$($numbers[-1])")
    $jobs = 1..2 | ForEach-Object { Start-Process powershell.exe -WindowStyle Hidden -PassThru -RedirectStandardOutput "$root\par-$_.txt" -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$sess\delegate-claude.ps1", "TOUCH-paralelo-$_" }
    $jobs | ForEach-Object { Wait-Mock $_ }
    $par = 1..2 | ForEach-Object { if ((Get-Content "$root\par-$_.txt" -Raw) -match '(?m)^delegation=(\d+)') { [int]$Matches[1] } }
    Check 'T15' 'duas delegacoes simultaneas, numeros unicos' (($par | Sort-Object) -join ',' -eq '26,27') ("numeros=$($par -join ',')")
    $rule = @(Get-ChildItem "$root\codex-home\rules" -Filter 'strategist-*.rules')
    $ruleText = if ($rule.Count -eq 1) { Get-Content $rule[0].FullName -Raw } else { '' }
    Check 'T21s' 'a regra da sessao nomeia so o launcher desta sessao (checagem estatica)' ($rule.Count -eq 1 -and @([regex]::Matches($ruleText, 'prefix_rule')).Count -eq 1 -and $ruleText.Contains("$sess\delegate-claude.ps1".Replace('\', '\\')))
    Check 'T20s' 'o Codex abre em sandbox somente leitura, sem bypass (checagem estatica)' ((Get-Content "$root\args.txt") -match '--sandbox\|read-only' -and (Get-Content "$root\args.txt") -notmatch 'bypass')
    $code = Stop-Session 0
    Check 'T23' 'encerramento normal remove pasta e regra da sessao' ($code -eq 0 -and -not (Test-Path $sess) -and -not @(Get-ChildItem "$root\codex-home\rules" -Filter 'strategist-*.rules').Count)

    # --- wrapper facts: executor failure, audit, stagnation brake ---
    $repo = New-Repo;Start-Session
    $r = Delegate 'CRASH no meio'
    Check 'T16' 'executor interrompido: falha registrada' ($r.Exit -eq 137 -and (Field $r 'executor_exit') -eq '137' -and (Field $r 'validation_status') -eq 'failed')
    Check 'T16c' 'executor saiu com 137: validation_status e executor_started aparecem uma vez so, no campo real' ((Sole $r 'validation_status' 'failed') -and (Sole $r 'executor_started' 'yes')) (Sole-Detail $r)
    $r = Delegate 'STEP 1'
    Check 'T16b' 'depois da falha a sessao continua, com o numero seguinte' ([int](Field $r 'delegation') -eq 2)
    $a = Delegate 'DEADTOOL x'; $b = Delegate 'DEADTOOL x'; $before = (Calls).Count; $c = Delegate 'DEADTOOL x'
    Check 'T7w' 'freio de estagnacao: 1a e 2a tentativas identicas rodam, a 3a e recusada sem chamar o executor' ((Field $a 'unresolved_attempts') -eq '1' -and (Field $b 'unresolved_attempts') -eq '2' -and $c.Text -match 'Estagnacao' -and (Calls).Count -eq $before)
    $d = Delegate 'DEADTOOL com outra abordagem'
    Check 'T7x' 'tarefa diferente depois da recusa volta a rodar, com numero seguinte' ((Field $d 'unresolved_attempts') -eq '1' -and [int](Field $d 'delegation') -eq 6)
    $r = Delegate 'BREAKGIT'
    Rename-Item "$repo\.git.off" '.git'
    Check 'T17' 'auditoria incompleta nao vira validacao aprovada' ($r.Text -match 'snapshot_status=incomplete' -and (Field $r 'validation_status') -eq 'incomplete' -and $r.Exit -eq 2)
    $code = Stop-Session 9
    Check 'T24' 'encerramento apos falha do Codex: codigo repassado, pasta e regra removidas' ($code -eq 9 -and -not (Test-Path $sess) -and -not @(Get-ChildItem "$root\codex-home\rules" -Filter 'strategist-*.rules').Count)

    # --- a session killed from outside must not leave its rule in force for later sessions ---
    $repo = New-Repo;Start-Session
    $dead = $sess
    Stop-Mock $proc
    $left = (Test-Path $dead) -and @(Get-ChildItem "$root\codex-home\rules" -Filter 'strategist-*.rules').Count -eq 1
    Start-Session
    $rulesNow = @(Get-ChildItem "$root\codex-home\rules" -Filter 'strategist-*.rules' | ForEach-Object BaseName)
    Check 'T24b' 'sessao morta a forca: a proxima abertura remove a regra e a pasta que sobraram' ($left -and $rulesNow.Count -eq 1 -and $rulesNow[0] -eq (Split-Path $sess -Leaf) -and -not (Test-Path $dead)) ("sobrou antes=$left; regras agora=$($rulesNow.Count); pasta morta existe=$(Test-Path $dead)")
    Stop-Session 0 | Out-Null

    # --- how a delegation ends: complete report, wrapper failure, still running ---
    $repo = New-Repo;Start-Session
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
    Check 'E2c' 'recusa do wrapper: validation_status e executor_started aparecem uma vez so, no campo real' ((Sole $r 'validation_status' 'incomplete') -and (Sole $r 'executor_started' 'no')) (Sole-Detail $r)
    Rename-Item "$repo\.git" '.git.off'
    $r = Delegate 'STEP 2'
    Rename-Item "$repo\.git.off" '.git'
    Check 'E5' 'auditoria inicial falha: erro do wrapper, executor nao roda, fim sem relatorio' ($r.Exit -eq 4 -and $r.Text -match 'wrapper_error: Falha de auditoria Git antes' -and $r.Text -match '(?m)^--- FIM SEM RELATORIO ---$' -and (Calls) -notcontains 'STEP 2') ("exit=$($r.Exit)")
    $r = Delegate 'CRASH'
    Check 'E4' 'executor interrompido: falha registrada num relatorio completo' ($r.Exit -eq 137 -and (Field $r 'validation_status') -eq 'failed' -and $r.Text -match '(?m)^--- FIM ---$')
    Check 'E4b' 'executor interrompido: o relatorio diz que o executor iniciou' ((Field $r 'executor_started') -eq 'yes') ("executor_started=$(Field $r 'executor_started')")
    $h0 = & $git -C $repo rev-parse HEAD
    $r = Delegate 'COMMIT vazio no mesmo branch'
    $h = & $git -C $repo rev-parse HEAD
    Check 'H0' 'o commit muda: head_changes no formato hashA -> hashB' ($h -ne $h0 -and $r.Text -match "(?m)^head_changes: $h0 -> $h$" -and (Field $r 'validation_status') -eq 'passed' -and $r.Exit -eq 0) ("$($r.Text -split "`n" -match '^head_changes')")
    $r = Delegate 'DIAG sem tocar em nada'
    Check 'H1' 'commit igual: head_changes e exatamente (nada)' ($r.Text -match '(?m)^head_changes: \(nada\)$') ("$($r.Text -split "`n" -match '^head_changes')")
    Stop-Session 0 | Out-Null

    # --- the initial snapshot fails before the executor: never run, and resumable ---
    # An untracked file held open with no sharing: Git lists it without opening it, and the
    # snapshot cannot hash it. Nothing here depends on timing.
    $repo = New-Repo;Start-Session
    [IO.File]::WriteAllText("$repo\travado.txt", "em uso`n")
    $hold = [IO.File]::Open("$repo\travado.txt", 'Open', 'Read', 'None')
    try { $a = Delegate 'STEP 1' } finally { $hold.Dispose() }
    Check 'P1' 'arquivo travado no retrato inicial: erro do wrapper, fim sem relatorio, executor nao iniciado' ($a.Exit -eq 4 -and $a.Text -match 'wrapper_error: Falha de auditoria Git antes' -and $a.Text -match 'snapshot_status=incomplete' -and (Field $a 'executor_started') -eq 'no' -and $a.Text -match '(?m)^--- FIM SEM RELATORIO ---$' -and $a.Text -notmatch '(?m)^--- FIM ---$') ("exit=$($a.Exit); executor_started=$(Field $a 'executor_started')")
    Check 'P3' 'retrato inicial falhando: o executor nao roda e o repositorio fica como estava' ((Calls).Count -eq 0 -and -not (Test-Path "$repo\progress.txt")) ("chamadas=$((Calls).Count)")
    $r = Delegate 'STEP 1'
    Check 'P5' 'causa removida: a mesma tarefa volta a rodar, com relatorio completo e executor iniciado' ($r.Exit -eq 0 -and (Field $r 'executor_started') -eq 'yes' -and (Field $r 'validation_status') -eq 'passed' -and (Field $r 'unresolved_attempts') -eq '1' -and [int](Field $r 'delegation') -eq 2 -and $r.Text -match '(?m)^--- FIM ---$' -and (Calls) -contains 'STEP 1') ("exit=$($r.Exit); delegacao=$(Field $r 'delegation')")
    Stop-Session 0 | Out-Null

    # --- the brake across other delegations: what did not pass is counted by attempt, in a row or not ---
    # The audit in between is the same task over the same state too, but it passes: it is never
    # counted, and it does not bring the failed task back to 1.
    $repo = New-Repo;Start-Session
    $a1 = Delegate 'DEADTOOL a'; $q1 = Delegate 'DIAG auditoria'; $a2 = Delegate 'DEADTOOL a'; $q2 = Delegate 'DIAG auditoria'
    $before = (Calls).Count; $a3 = Delegate 'DEADTOOL a'
    Check 'R1' 'A falha, auditoria, A falha, auditoria: a terceira A e recusada sem chamar o executor' ((Field $a1 'unresolved_attempts') -eq '1' -and (Field $a2 'unresolved_attempts') -eq '2' -and $a3.Exit -eq 4 -and $a3.Text -match 'wrapper_error: Estagnacao' -and (Field $a3 'unresolved_attempts') -eq '3' -and (Field $a3 'executor_started') -eq 'no' -and $a3.Text -match '(?m)^--- FIM SEM RELATORIO ---$' -and $a3.Text -notmatch '(?m)^--- FIM ---$' -and (Calls).Count -eq $before) ("contagem=$(Field $a1 'unresolved_attempts'),$(Field $a2 'unresolved_attempts'),$(Field $a3 'unresolved_attempts'); exit=$($a3.Exit)")
    Check 'R1b' 'recusa pela contagem de nao aprovadas: validation_status e executor_started aparecem uma vez so, no campo real' ((Field $a3 'validation_status') -eq 'incomplete' -and (Sole $a3 'validation_status' 'incomplete') -and (Sole $a3 'executor_started' 'no')) (Sole-Detail $a3)
    $q3 = Delegate 'DIAG auditoria'; $before = (Calls).Count; $a4 = Delegate 'DEADTOOL a'
    Check 'R2' 'validacao que passa, repetida entre outras tarefas: nunca contada nem recusada' ((Field $q1 'unresolved_attempts') -eq '1' -and (Field $q2 'unresolved_attempts') -eq '1' -and (Field $q3 'unresolved_attempts') -eq '1' -and (Field $q3 'validation_status') -eq 'passed' -and $q3.Exit -eq 0) ("contagem=$(Field $q1 'unresolved_attempts'),$(Field $q2 'unresolved_attempts'),$(Field $q3 'unresolved_attempts')")
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

    $repo = New-Repo;Start-Session
    $r = 1..2 | ForEach-Object { Delegate 'DEADTOOL a'; Delegate 'BADFIX b' }
    $before = (Calls).Count; $a = Delegate 'DEADTOOL a'; $b = Delegate 'BADFIX b'
    Check 'R5' 'A e B alternadas, as duas falhando: duas de cada rodam, a terceira de cada e recusada' ((($r | ForEach-Object { Field $_ 'unresolved_attempts' }) -join ',') -eq '1,1,2,2' -and $a.Exit -eq 4 -and $a.Text -match 'wrapper_error: Estagnacao' -and $b.Exit -eq 4 -and $b.Text -match 'wrapper_error: Estagnacao' -and (Field $b 'executor_started') -eq 'no' -and (Calls).Count -eq $before -and $before -eq 4) ("contagem=$(($r | ForEach-Object { Field $_ 'unresolved_attempts' }) -join ','); chamadas=$((Calls).Count)")
    # A delegation that never got to record how it ended (killed, or still running): the slot
    # holds the attempt alone. Made here by taking the result off two slots that had passed.
    $w1 = Delegate 'DIAG w'; Delegate 'DIAG outra w' | Out-Null; $w2 = Delegate 'DIAG w'; Delegate 'DIAG outra w' | Out-Null
    foreach ($w in $w1, $w2) { $f = "$sess\delegation-$(Field $w 'delegation')"; [IO.File]::WriteAllText($f, @(Get-Content -LiteralPath $f)[0]) }
    $before = (Calls).Count; $w3 = Delegate 'DIAG w'
    Check 'R8' 'tentativa sem resultado registrado conta como nao aprovada: a terceira e recusada' ((Field $w2 'unresolved_attempts') -eq '1' -and $w3.Exit -eq 4 -and (Field $w3 'unresolved_attempts') -eq '3' -and (Field $w3 'executor_started') -eq 'no' -and (Calls).Count -eq $before) ("exit=$($w3.Exit); contagem=$(Field $w3 'unresolved_attempts')")
    Stop-Session 0 | Out-Null

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
    $repo = New-Repo;Start-Session
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
    $repo = New-Repo
    $opened = Try-Start
    if ($opened) { Stop-Session 0 | Out-Null }
    Stop-Mock $locker
    Check 'S2' 'regra antiga ilegivel: a sessao abre, a regra fica, e nada da sessao nova sobra' ($opened -and (Test-Path $lockedRule) -and -not (Leaked).Count) ("abriu=$opened; sobras=$((Leaked).Count)")
    Check 'S3' 'regra antiga com conteudo estranho: regra e pasta preservadas' (Kept $garbage)
    Remove-Item -LiteralPath $garbage -Recurse -Force; Get-ChildItem $rulesDir | Remove-Item -Force

    # the instructions file is missing: AGENTS.md cannot be written
    $broken = "$root\tool-sem-md"; New-Item -ItemType Directory -Path $broken | Out-Null
    Copy-Item "$tool\strategist.cmd", "$tool\strategist.ps1", "$tool\strategist.config.ps1", "$tool\executor.md" $broken
    $opened = Try-Start $broken
    $cause = Launcher-Out; $cause += "$(Get-Content -LiteralPath "$launcherOut.err" -Raw -ErrorAction SilentlyContinue)"   # the failure must be the missing strategist.md, not another file
    Check 'S5' 'falha ao escrever o AGENTS.md: erro, nenhuma pasta e nenhuma regra sobram' (-not $opened -and $proc.ExitCode -ne 0 -and $cause -match 'strategist\.md' -and -not (Leaked).Count -and -not (Rules).Count) ("abriu=$opened; exit=$($proc.ExitCode); sobras=$((Leaked).Count)")

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
    $repo = New-Repo;Start-Session
    $hold = [IO.File]::Open("$sess\AGENTS.md", 'Open', 'Read', 'None')
    $code = Stop-Session 7
    $hold.Dispose()
    Check 'S8' 'erro na limpeza: codigo do Codex preservado, regra removida, so a pasta da propria sessao fica' ($code -eq 7 -and -not (Rules).Count -and ((Leaked) -join ',') -eq (Split-Path $sess -Leaf)) ("exit=$code; sobras=$((Leaked) -join ',')")
    Remove-Item -LiteralPath $sess -Recurse -Force

    # the session folder cannot even be listed when the session ends (listing denied on a
    # throwaway folder of this test, and allowed again right after)
    function Deny-List([string]$dir) { & icacls.exe $dir /deny "${env:USERNAME}:(RD)" | Out-Null }
    function Allow-List([string]$dir) { & icacls.exe $dir /remove:d $env:USERNAME | Out-Null }
    $repo = New-Repo;$base = Sessions; Start-Session
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
    $repo = New-Repo;Start-Session
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
    $repo = New-Repo;Start-Session
    Check 'S15' 'pasta de sessao que e uma juncao: nao e seguida, o destino fica intacto e a limpeza incompleta e avisada' (@(Get-ChildItem $target -File).Count -eq 4 -and (Launcher-Out) -match 'juncao') ("arquivos no destino=$(@(Get-ChildItem $target -File).Count); aviso=$((Launcher-Out) -match 'juncao')")
    Stop-Session 0 | Out-Null
    [IO.Directory]::Delete("$tmp\$jid")
    Get-ChildItem $rulesDir | Remove-Item -Force

    # the simulated Codex hangs before the session shows up: the test must end, and end it
    $env:MOCK_CODEX = 'hang'
    $repo = New-Repo
    $sw =[Diagnostics.Stopwatch]::StartNew()
    $opened = Try-Start $tool 8
    $env:MOCK_CODEX = ''
    $stuck = @(Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object { $_.CommandLine -like "*$root\bin\fake-codex.ps1*" }).Count
    Check 'S11' 'Codex simulado travado, sem marcador: o teste termina no prazo e encerra so o que ele criou' (-not $opened -and $sw.Elapsed.TotalSeconds -lt 40 -and $proc.HasExited -and $stuck -eq 0) ("abriu=$opened; tempo=$([int]$sw.Elapsed.TotalSeconds)s; processos restantes=$stuck")
    $abandoned = @(Leaked)    # the launcher was killed: its folder and rule are a legitimate leftover
    $quick = Start-Process cmd.exe -ArgumentList '/c', 'exit 3' -WindowStyle Hidden -PassThru
    Wait-Mock $quick 30; Stop-Mock $quick
    Check 'S12' 'processo que ja terminou: nada e encerrado e o codigo de saida fica' ($quick.ExitCode -eq 3)
    $repo = New-Repo;Start-Session
    Check 'S13' 'depois do prazo estourado a suite segue: a sessao seguinte abre e recolhe a sobra' ((Test-Path $sess) -and $abandoned.Count -eq 1 -and -not (Test-Path "$tmp\$($abandoned[0])")) ("sobras do teste anterior=$($abandoned.Count)")
    Stop-Session 0 | Out-Null

    'T20, T21, T22: dependem do Codex e do Claude reais (aprovacao, sandbox, recusa de operacao proibida); nao fazem parte desta suite.'
}
finally {
    $env:LOCALAPPDATA = $savedLocalAppData
    if ($proc -and -not $proc.HasExited) { Set-Content "$root\stop" 1; $proc.WaitForExit(15000) | Out-Null }
    & cmd.exe /c "icacls `"$root`" /remove:d $env:USERNAME /T /C /Q >nul 2>&1"   # a test stopped halfway may have left a deny in place
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}
"falhas: $failures"
exit $failures

# Plumbing only. The prompts live beside this file: strategist.md (the orchestrator, Codex) and executor.md (the executor, Claude or Codex).
# Keep this file ASCII-only: strategist.cmd runs it in Windows PowerShell 5.1,
# which reads a .ps1 without BOM as ANSI.
param([string]$Session, [string]$Project)

$ErrorActionPreference = "Stop"

# Config helpers (strategist.config.ps1). In delegate mode only the frozen copy is used.
. "$PSScriptRoot\strategist.config.ps1"

# In delegate mode every failure of this wrapper ends the same way, so whoever reads the output
# can tell "ended without a report" from "still running": the error, the validation state and a
# closing line that is NOT the one of a complete report. Exit code 4. The launcher mode keeps
# PowerShell's own behaviour.
trap {
    if (-not $Session) { break }
    Write-Output "wrapper_error: $_"
    # 'no' is a proof: the line that calls the executor was not reached, so this delegation changed
    # nothing. 'yes' only says the call was reached; what it did is unknown.
    Write-Output "executor_started=$(if ($executorStarted) { 'yes' } else { 'no' }) [no: o executor nao chegou a ser chamado e esta delegacao nao alterou nada; yes: foi chamado e os efeitos sao desconhecidos]"
    Write-Output "validation_status=incomplete [o wrapper parou antes do relatorio completo; a mensagem acima diz por que]"
    Write-Output "--- FIM SEM RELATORIO ---"
    exit 4
}

# Called internally by the temporary delegate-claude.cmd, with the task on stdin.
if ($Session) {
    # Read as UTF-8 bytes: [Console]::In would decode with the console code page and break accents.
    $task = if ([Console]::IsInputRedirected) { (New-Object IO.StreamReader ([Console]::OpenStandardInput()), ([Text.Encoding]::UTF8)).ReadToEnd() }
    if ([string]::IsNullOrWhiteSpace($task)) {
        throw "Task vazia. Passe a tarefa ao launcher (delegate-claude.ps1 'tarefa') ou por stdin ao delegate-claude.cmd."
    }

    # Delegation number: it only grows within a session and is there to be read, not to cap
    # anything. CreateNew is atomic, so concurrent calls cannot take the same number. The
    # sandboxed Codex cannot write in the session folder: it cannot erase the numbers, and a
    # call that did not leave the sandbox stops here.
    if (-not (Test-Path -LiteralPath $Session -PathType Container)) { throw "Falha de inicializacao: a pasta da sessao nao existe: $Session" }
    for ($slot = 1; ; $slot++) {
        try { [IO.File]::Open("$Session\delegation-$slot", 'CreateNew').Close(); break }
        catch [UnauthorizedAccessException] {
            throw "Sem escrita na pasta da sessao: esta chamada esta dentro do sandbox do Codex. So sai dele o launcher da sessao, sozinho na linha e com a tarefa entre aspas simples, como no AGENTS.md."
        }
        catch [IO.IOException] { if (-not (Test-Path -LiteralPath "$Session\delegation-$slot")) { throw } }
    }

    # Frozen copy of the session settings, written by the launcher from a validated object, in
    # a folder the sandboxed Codex cannot write. A session without it stops here.
    $frozen = [IO.File]::ReadAllText("$Session\session-config.json") | ConvertFrom-Json

    Set-Location -LiteralPath $Project

    # Git output split on NUL, so spaces and Unicode in paths survive. Throws when Git fails.
    function Get-GitPaths([string]$arguments) { (Invoke-OrqGit "--no-optional-locks $arguments").Split([char]0) | Where-Object { $_ } }

    # Read-only picture of the working tree: path -> SHA-256 of every tracked file (ignored
    # or not) and of every untracked file that .gitignore does not hide. It writes nothing:
    # no objects, no index, no copies of content.
    # ponytail: hashes every file twice per delegation; cache by size and mtime if slow.
    function Get-Snapshot {
        $map = @{}
        $sha = [Security.Cryptography.SHA256]::Create()
        foreach ($path in @(Get-GitPaths 'ls-files -z --cached') + @(Get-GitPaths 'ls-files -z --others --exclude-standard')) {
            # Only "not found" means gone from disk. Any other error (access denied, file in
            # use) stops the snapshot, so an unreadable file never passes for a removed one.
            try { $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop }
            catch [Management.Automation.ItemNotFoundException] { continue }
            $map[$path] = if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { "link:$($item.Target)" }  # not followed
            elseif ($item.PSIsContainer) { 'dir' }  # submodule or nested repository: one entry
            else {
                # Not Get-FileHash: it vanishes when this 5.1 process is started from PowerShell 7.
                $stream = [IO.File]::OpenRead($item.FullName)
                try { [BitConverter]::ToString($sha.ComputeHash($stream)) } finally { $stream.Dispose() }
                $now = Get-Item -LiteralPath $path -Force -ErrorAction Stop
                if ($now.LastWriteTimeUtc -ne $item.LastWriteTimeUtc -or $now.Length -ne $item.Length) { throw "arquivo mudou durante a leitura: $path" }
            }
        }
        $map
    }

    # Read-only picture of the index: path -> "mode object stage" of each entry (a path in
    # conflict has several). It catches what the hashes cannot: git add, git restore --staged,
    # git rm --cached, a mode change.
    function Get-Index {
        $map = @{}
        foreach ($entry in Get-GitPaths 'ls-files -z --stage') { $meta, $path = $entry -split "`t", 2; $map[$path] += "$meta;" }
        $map
    }

    function Compare-Snapshot($old, $new) {
        foreach ($p in (@($old.Keys) + @($new.Keys) | Sort-Object -Unique)) {
            if (-not $old.ContainsKey($p)) { "A`t$p" }
            elseif (-not $new.ContainsKey($p)) { "D`t$p" }
            elseif ($old[$p] -ne $new[$p]) { "M`t$p" }
        }
    }

    try {
        $beforeHead = & git rev-parse -q --verify HEAD
        $before = Get-Snapshot
        $indexBefore = Get-Index
    }
    # The snapshot is taken on every call and never accepted incomplete: once the cause is gone
    # the delegation runs again.
    catch { throw "Falha de auditoria Git antes da delegacao (snapshot_status=incomplete); o executor NAO foi executado. $_" }

    # Stagnation brake, independent of how many delegations there were: the same task over the
    # same repository state (HEAD, content and index) is the same attempt. The slot keeps the
    # attempt and, on a second line, 'passed' when it ended with validation_status=passed. No
    # second line means it did not pass: failed, incomplete, refused, interrupted or still
    # running, all read the same way. The count is every earlier slot of the session with this
    # same attempt and no second line, in a row or not, plus this one; the third is refused.
    # Rewording the task or any change of state starts at 1: it stops blind repetition, whether
    # a change was progress is for the strategist to say.
    $text = @($task.Trim(), $beforeHead) + @($before.Keys | Sort-Object | ForEach-Object { "$_=$($before[$_])" }) + @($indexBefore.Keys | Sort-Object | ForEach-Object { "$_=$($indexBefore[$_])" })
    $attempt = [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($text -join "`n")))
    $unresolved = 1
    for ($k = 1; $k -lt $slot; $k++) {
        $prior = @(Get-Content -LiteralPath "$Session\delegation-$k" -ErrorAction SilentlyContinue)
        if ($prior.Count -eq 1 -and $prior[0] -eq $attempt) { $unresolved++ }
    }
    [IO.File]::WriteAllText("$Session\delegation-$slot", $attempt)
    if ($unresolved -ge 3) {
        throw "Estagnacao (delegacao $slot): a mesma tarefa sobre o mesmo estado do repositorio ja rodou sem validacao aprovada nesta sessao e nada mudou desde entao, mesmo com outras delegacoes no meio | unresolved_attempts=$unresolved [com esta; a terceira e recusada]. O executor NAO foi executado. Mude a abordagem ou encerre com o diagnostico."
    }

    Write-Output "--- EXECUTOR (delegacao $slot) ---"
    # The prompt goes by stdin: PowerShell 5.1 mangles double quotes inside native arguments.
    $OutputEncoding = [Text.UTF8Encoding]::new($false)
    # Explicit mode, so the user's default (which may be bypassPermissions) does not apply:
    # auto lets a classifier refuse risky actions, and nobody answers prompts, so whatever
    # would ask is denied. The deny rules catch only the obvious spellings: a net, not a wall.
    $deny = 'git push*', 'git reset --hard*', 'git clean*', 'npm publish*', 'pnpm publish*' |
        ForEach-Object { "Bash($_)"; "PowerShell($_)" }
    # Set before the call, never after: from here on a failure of this wrapper cannot prove
    # that the executor did nothing.
    $executorStarted = $true
    $prompt = (Get-Content -Raw -Encoding UTF8 "$PSScriptRoot\executor.md"), $task
    if ($frozen.executor.tool -eq 'codex') {
        # Explicit sandbox, never the user's default (which may be danger-full-access).
        # --ignore-rules: an allow rule, this session's launcher rule included, would run its
        # command outside the sandbox. No prompt argument: codex exec reads it from stdin.
        $prompt | & codex exec --sandbox workspace-write --ignore-rules @(Get-OrqCodexArgs $frozen.executor)
    }
    else {
        $modelArgs = @(Get-OrqClaudeArgs $frozen.executor)
        $prompt | & claude -p --permission-mode auto --permission-prompts none @modelArgs --disallowedTools $deny
    }
    $exitCode = $LASTEXITCODE

    # A Git failure here must not read as "nothing changed": it becomes the audit state.
    $auditError = $null
    try {
        $afterHead = & git rev-parse -q --verify HEAD
        $after = Get-Snapshot
        $indexAfter = Get-Index
        $changed = @(Compare-Snapshot $before $after | ForEach-Object {
            $p = $_.Substring(2)
            if ($_ -like "A`t*" -and -not $indexAfter.ContainsKey($p)) { "??`t$p" }
            # Left the audit's reach (untracked now, and ignored) but still on disk: not a deletion.
            elseif ($_ -like "D`t*" -and (Test-Path -LiteralPath $p)) { "I`t$p" }
            else { $_ }
        })
        $indexChanged = @(Compare-Snapshot $indexBefore $indexAfter)
    }
    catch { $auditError = "$_" }

    # Final state of what this wrapper can check. It never says the task itself is done.
    $validation = if ($exitCode) { 'failed' } elseif ($auditError) { 'incomplete' } else { 'passed' }
    # For the brake's count. Only 'passed' is recorded: anything else, a wrapper that
    # died before this line included, leaves the slot as an attempt that did not pass. A write
    # that fails leaves it the same way and must not cost the report.
    if ($validation -eq 'passed') { try { [IO.File]::AppendAllText("$Session\delegation-$slot", "`npassed") } catch { } }

    Write-Output "--- EVIDENCIA GIT ---"
    Write-Output "delegation=$slot unresolved_attempts=$unresolved [esta, mais as anteriores da sessao com a mesma tarefa sobre o mesmo estado que rodaram sem validacao aprovada, seguidas ou nao; a terceira e recusada]"
    Write-Output "executor_exit: $exitCode$(if ($exitCode) { ' (FALHOU)' })"
    Write-Output "executor_started=yes [o executor foi chamado nesta delegacao]"
    if ($auditError) { Write-Output "auditoria: FALHOU, snapshot_status=incomplete ($auditError): as alteracoes desta delegacao NAO foram verificadas" }
    else {
        Write-Output "auditoria: ok, snapshot_status=complete"
        Write-Output "working_tree_changes (A novo rastreado; ?? novo nao rastreado; M modificado; D removido; I ainda no disco, mas deixou de ser rastreado e e ignorado):"
        if ($changed) { $changed } else { "(nada)" }
        Write-Output "index_changes (A entrou no stage; M entrada mudou: conteudo, modo ou estagio; D saiu do stage):"
        if ($indexChanged) { $indexChanged } else { "(nada)" }
        Write-Output "head_changes: $(if ("$afterHead" -ne "$beforeHead") { "$beforeHead -> $afterHead" } else { '(nada)' })"
    }
    Write-Output "validation_status=$validation [executor e auditoria; nao prova o criterio de pronto da tarefa]"
    Write-Output "limite: compara caminho e SHA-256 antes e depois. Nao prova autoria (outro processo pode ter escrito no intervalo); nao ve nao rastreados ignorados; submodulo ou repositorio aninhado conta como uma entrada; link simbolico e comparado pelo destino, sem ser seguido; pasta que e juncao e lida por dentro, como o Git a lista."
    Write-Output "--- FIM ---"
    # The executor's own code is kept. Only a 0 is replaced: 2 for incomplete.
    exit $(if ($exitCode) { $exitCode } elseif ($auditError) { 2 } else { 0 })
}

# Settings are read once, here. A saved file must be valid (no fallback); with no file the
# defaults apply, over the Git repository of the cwd.
$configPath = Get-OrqConfigPath
$config = Read-OrqConfig $configPath
$project = Get-OrqGitRoot $(if ($config) { $config.project } else { (Get-Location).Path })
if (-not $project) { throw "Abra um terminal dentro do repositorio Git e execute strategist.cmd, ou defina a pasta do projeto em $configPath." }
if (-not $config) { $config = New-OrqConfig $project }
Write-Host "Configuracao: $configPath$(if (-not (Test-Path -LiteralPath $configPath)) { ' (ausente: padroes)' })"
Get-OrqText $config | ForEach-Object { Write-Host $_ }

$temp = Join-Path ([IO.Path]::GetTempPath()) ("strategist-" + [Guid]::NewGuid().ToString("N"))

# The one command Codex may run outside the sandbox without asking. Measured on Codex 0.162:
# an allow rule frees a command only when it is the whole command and is written bare, with
# no "&", pipe or ";", so the task cannot come through a pipe. This launcher takes it as its
# single argument and hands it to the .cmd by stdin. It ends with [Environment]::Exit because
# "pwsh -Command script" turns any failing exit code into 1.
$launcher = "$temp\delegate-claude.ps1"
# The rule names this session's launcher by its full path. Codex only reads rules from its own
# rules folder (a rule inside this temporary folder is ignored, also measured), so the file
# goes there under this session's name and is removed at the end.
$rules = Join-Path $(if ($env:CODEX_HOME) { $env:CODEX_HOME } else { "$HOME\.codex" }) 'rules'
$rule = Join-Path $rules ((Split-Path $temp -Leaf) + '.rules')
function Get-RuleText([string]$path) { "prefix_rule(pattern=[`"$($path.Replace('\', '\\'))`"], decision=`"allow`", justification=`"Delegacao autorizada ao Claude`")`n" }

# Removes one session folder without recursion: only the files this script creates, then the
# folder if it is empty. Anything else in it, a junction included, is left where it is.
# It never throws: it runs inside a finally, where an error of its own would take the place of
# the error that brought the script there, and of Codex's exit code. What it could not remove
# stays, and it says so.
function Remove-Session([string]$dir) {
    try {
        # Only "not found" means there is nothing left to remove. A folder that is there but
        # cannot be looked at, or that is a junction, was not cleaned, and that gets said.
        try { $item = Get-Item -LiteralPath $dir -Force -ErrorAction Stop }
        catch [Management.Automation.ItemNotFoundException] { return }
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'e uma juncao ou um link, que nao e seguido' }
        Get-ChildItem -LiteralPath $dir -Force -File |
            Where-Object { $_.Name -match '^(AGENTS\.md|delegate-claude\.(cmd|ps1)|owner|session-config\.json|delegation-\d+)$' -and -not ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) } |
            Remove-Item -Force -ErrorAction SilentlyContinue
        [IO.Directory]::Delete($dir)
    }
    catch { Write-Host "Aviso: a pasta de sessao $dir nao foi removida por inteiro e ficou no lugar: $($_.Exception.Message)" }
}

# 'alive', 'dead' or 'unknown'. A PID alone proves nothing, Windows reuses them: it is the same
# owner only if the start time matches too. A process that cannot be read is 'unknown'.
function Get-OwnerState([string]$id, [string]$started) {
    try { $p = Get-Process -Id ([int]$id) -ErrorAction Stop }
    catch [Microsoft.PowerShell.Commands.ProcessCommandException] { return 'dead' }
    catch { return 'unknown' }
    try { if ("$($p.StartTime.ToFileTimeUtc())" -eq $started) { 'alive' } else { 'dead' } } catch { 'unknown' }
}

# Everything this session creates is created inside the try, so the finally undoes it whatever
# fails on the way: a write, the old rules, the rule of this session or Codex itself. It does
# not cover a killed process; that is what the recovery of leftovers, in the middle, is for.
try {
    New-Item -ItemType Directory -Path $temp | Out-Null
    # WriteAllText writes UTF-8 without BOM, which a .cmd needs. "%~dp0." is the session folder.
    [IO.File]::WriteAllText("$temp\delegate-claude.cmd", "@powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Session `"%~dp0.`" -Project `"$project`"`r`n")
    [IO.File]::WriteAllText($launcher, "param([Parameter(Mandatory = `$true, Position = 0)][string]`$Task)`r`n`$OutputEncoding = [Text.UTF8Encoding]::new(`$false)`r`n`$Task | & `"`$PSScriptRoot\delegate-claude.cmd`"`r`n[Environment]::Exit(`$LASTEXITCODE)`r`n")
    [IO.File]::WriteAllText("$temp\owner", "$PID $((Get-Process -Id $PID).StartTime.ToFileTimeUtc()) $(Split-Path $temp -Leaf)")
    # Frozen copy of this session's settings: every delegation reads it, never the global file.
    [IO.File]::WriteAllText("$temp\session-config.json", ($config | ConvertTo-Json -Depth 4))
    [IO.File]::WriteAllText("$temp\AGENTS.md", [IO.File]::ReadAllText("$PSScriptRoot\strategist.md").Replace('{{DELEGATE}}', $launcher))

    # A session that was killed leaves its rule in force for as long as its launcher exists.
    # Both are removed here, but only on proof: the rule file has the exact name and text this
    # script writes for that folder, the folder names itself as that session ("pid start
    # session" in owner), and the owner is provably gone. Anything short of that is left
    # untouched, and so is a rule that cannot be read or checked: one bad old rule neither
    # gets deleted nor stops this session from starting.
    Get-ChildItem -LiteralPath $rules -Filter 'strategist-*.rules' -File -ErrorAction SilentlyContinue |
        Where-Object { $_.BaseName -match '^strategist-[0-9a-f]{32}$' } | ForEach-Object {
            $file = $_
            try {
                $old = [IO.Path]::GetTempPath().TrimEnd('\') + '\' + $file.BaseName
                if ([IO.File]::ReadAllText($file.FullName) -ne (Get-RuleText "$old\delegate-claude.ps1")) { return }
                if (-not (Test-Path -LiteralPath $old)) { Remove-Item -LiteralPath $file.FullName -ErrorAction SilentlyContinue; return }
                $owner = "$(Get-Content -LiteralPath "$old\owner" -ErrorAction SilentlyContinue)" -split ' '
                if ($owner.Count -ne 3 -or $owner[0] -notmatch '^\d{1,9}$' -or $owner[1] -notmatch '^\d+$' -or $owner[2] -ne $file.BaseName) { return }
                if ((Get-OwnerState $owner[0] $owner[1]) -ne 'dead') { return }
                Remove-Item -LiteralPath $file.FullName -ErrorAction SilentlyContinue
                Remove-Session $old
            }
            catch { Write-Host "Aviso: a regra antiga $($file.Name) nao pode ser verificada e foi preservada: $($_.Exception.Message)" }
        }

    Write-Host "Codex strategist: o executor ($($config.executor.tool)) opera $project. Codex em sandbox somente leitura; so o launcher de delegacao desta sessao sai dele sem pedir aprovacao."
    Write-Host "Limites: o sandbox nao impede leitura; a regra libera o launcher, nao limita o que o executor faz depois; filtros de comando e prompts orientam, nao sao barreira de seguranca."
    if ($temp -match '\s') { Write-Host "Aviso: a pasta temporaria tem espaco no caminho; a regra nao casa e cada delegacao vai pedir aprovacao." }

    New-Item -ItemType Directory -Force -Path $rules | Out-Null
    [IO.File]::WriteAllText($rule, (Get-RuleText $launcher))
    # Read-only sandbox: Codex cannot write anywhere, this folder and the rules folder included,
    # so it cannot edit the launcher, the rule or the delegation slots. It can still READ the
    # whole disk. Anything else leaves the sandbox only through an approval the user grants.
    & codex -C $temp --sandbox read-only --ask-for-approval on-request @(Get-OrqCodexArgs $config.orchestrator)
    $code = $LASTEXITCODE
}
finally {
    # The original error, if any, goes on after this block; nothing here replaces it.
    Remove-Item -LiteralPath $rule -ErrorAction SilentlyContinue
    Remove-Session $temp
}
exit $code

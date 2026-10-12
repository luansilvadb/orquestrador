# Plumbing only. The prompts live beside this file: strategist.md (Codex) and executor.md (Claude).
# Keep this file ASCII-only: strategist.cmd runs it in Windows PowerShell 5.1,
# which reads a .ps1 without BOM as ANSI.
param([string]$Session, [string]$Project)

$ErrorActionPreference = "Stop"

# In delegate mode every failure of this wrapper ends the same way, so whoever reads the output
# can tell "ended without a report" from "still running": the error, the validation state and a
# closing line that is NOT the one of a complete report. Exit code 4. The launcher mode keeps
# PowerShell's own behaviour.
trap {
    if (-not $Session) { break }
    Write-Output "wrapper_error: $_"
    # 'no' is a proof: the line that calls Claude was not reached, so this delegation changed
    # nothing. 'yes' only says the call was reached; what it did is unknown.
    Write-Output "executor_started=$(if ($executorStarted) { 'yes' } else { 'no' }) [no: o Claude nao chegou a ser chamado e esta delegacao nao alterou nada; yes: foi chamado e os efeitos sao desconhecidos]"
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

    Set-Location -LiteralPath $Project

    # Git output read as UTF-8 and split on NUL, so spaces and Unicode in paths survive
    # (PowerShell 5.1 would decode it with the console code page). Throws when Git fails.
    function Get-GitPaths([string]$arguments) {
        $psi = New-Object Diagnostics.ProcessStartInfo 'git', "--no-optional-locks $arguments"
        $psi.WorkingDirectory = (Get-Location).Path
        $psi.UseShellExecute = $false
        $psi.RedirectStandardOutput = $true
        $psi.StandardOutputEncoding = [Text.Encoding]::UTF8
        $p = [Diagnostics.Process]::Start($psi)
        $out = $p.StandardOutput.ReadToEnd()
        $p.WaitForExit()
        if ($p.ExitCode) { throw "git $arguments saiu com $($p.ExitCode)" }
        $out.Split([char]0) | Where-Object { $_ }
    }

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
        # The branch HEAD points at; empty when HEAD is detached. A checkout to another branch
        # on the same commit changes this and not the hash. Informative only.
        $beforeRef = & git symbolic-ref -q HEAD
        $beforeDirty = @(& git --no-optional-locks status --porcelain).Count
        if ($LASTEXITCODE) { throw "git status saiu com $LASTEXITCODE" }
        $before = Get-Snapshot
        $indexBefore = Get-Index
    }
    catch {
        # This failure comes before the attempt hash below exists, so the stagnation brake never
        # saw it: the slot stayed empty and the same failure could repeat forever. The slot now
        # holds the cause (the error text, not the task: an audit delegated to look into it stops
        # at this same snapshot, under another task). The snapshot is still taken on every call
        # and never accepted incomplete, so once the cause is gone the delegation runs again;
        # another cause, or a delegation that got through, starts the count over.
        $failure = "$_"
        $cause = 'pre-executor ' + [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($failure)))
        $repeats = 1
        for ($k = $slot - 1; $k -ge 1 -and (Get-Content -LiteralPath "$Session\delegation-$k" -ErrorAction SilentlyContinue) -eq $cause; $k--) { $repeats++ }
        [IO.File]::WriteAllText("$Session\delegation-$slot", $cause)
        $stuck = if ($repeats -ge 3) { " Estagnacao (delegacao $slot): a causa esta no ambiente e nenhuma delegacao a remove, nem a de auditoria, que para neste mesmo retrato. Encerre com o diagnostico; removida a causa, a delegacao volta a rodar." }
        throw "Falha de auditoria Git antes da delegacao (snapshot_status=incomplete); o Claude NAO foi executado. $failure | pre_executor_failures=$repeats [mesma falha do retrato inicial, seguidas, qualquer que seja a tarefa]$stuck"
    }

    # One whitespace check ("--check"). Returns, per problem, the line Git printed plus a key
    # ("file: message|offending text") without the line number, so a problem that only moved
    # stays the same problem. The text is compared, never printed. $clean is Git's exit code
    # for "no problem" (0, or 1 with --no-index); problems add 2; anything else is a failure.
    # The working tree goes through diff-index: "git diff HEAD" rewrites a stale index even
    # with --no-optional-locks, and this audit must not write.
    # Git's default rules give two false alarms, and both are adapted here and nowhere else, so
    # every view (working tree, index, commits, untracked) and the "before" they are compared
    # with see the same thing:
    #   - the CR of a CRLF line counts as trailing whitespace wherever Git does not convert the
    #     line ending (core.autocrlf off, or the untracked file read as it is): Git's own
    #     "cr-at-eol" is added to the defaults, and blanks before the CR are still reported;
    #   - a Markdown hard break (a line with content ending in exactly two spaces) is syntax:
    #     that one line is dropped, in a .md/.markdown file. Any other trailing blank in the
    #     same file stays.
    # Only where the repository says nothing. An explicit core.whitespace (any level of config)
    # or a "whitespace" attribute on the path is the project's policy and Git's verdict is kept
    # as it is. Nothing is written: both are only read, on every call, since a delegation may
    # change them. A read that fails throws, and the check ends "unavailable", never passed.
    # ponytail: a lone CR in an LF file is let through too, and two spaces inside a Markdown
    # code block count as a break; tell them apart only if that ever hides a real problem.
    function Get-Whitespace([int]$clean, [string[]]$gitArgs) {
        $null = & git config --get core.whitespace
        if ($LASTEXITCODE -gt 1) { throw "git config core.whitespace saiu com $LASTEXITCODE" }
        $default = $LASTEXITCODE -eq 1   # 1: not set anywhere
        $crAtEol = @(if ($default) { '-c', 'core.whitespace=cr-at-eol' })
        $out = @(& git --no-optional-locks --literal-pathspecs @crAtEol @gitArgs)
        if ($LASTEXITCODE -ne $clean -and $LASTEXITCODE -ne $clean + 2) { throw "git diff --check saiu com $LASTEXITCODE" }
        $noAttribute = @{}
        for ($i = 0; $i -lt $out.Count; $i++) {
            if ($out[$i] -notlike '+*' -and $out[$i] -match '^(.*):\d+: (.*)$') {
                $path = $Matches[1]; $message = $Matches[2]
                $text = if ("$($out[$i + 1])" -like '+*') { $out[$i + 1] }
                if ($default -and $message -eq 'trailing whitespace.' -and $path -match '\.(md|markdown)$' -and $text -match '^\+.*\S  \r?$') {
                    if (-not $noAttribute.ContainsKey($path)) {
                        $attribute = @(& git check-attr whitespace -- $path)
                        if ($LASTEXITCODE) { throw "git check-attr whitespace saiu com $LASTEXITCODE" }
                        $noAttribute[$path] = "$($attribute[-1])" -like '*: whitespace: unspecified'
                    }
                    if ($noAttribute[$path]) { continue }
                }
                $out[$i] + "`t" + $path + ': ' + $message + '|' + $text
            }
        }
    }
    # Problems that were already there, so they are not pinned on this delegation. Three views
    # of the same files: 'working tree' (disk against HEAD), 'index' (staged against HEAD) and
    # 'unstaged' (disk against the index).
    # $null: unknown, and what shows up later has no known origin.
    $wsBefore = $null
    if ($beforeHead) {
        try {
            $wsBefore = @{
                'working tree' = @(Get-Whitespace 0 'diff-index', '--check', 'HEAD')
                'index'        = @(Get-Whitespace 0 'diff', '--cached', '--check', 'HEAD')
                'unstaged'     = @(Get-Whitespace 0 'diff-files', '--check')
            }
        }
        catch { $wsBeforeError = "$_" }
    }
    # Credits, one per occurrence. A line that is staged and also on disk is one line, even when
    # a move makes 'unstaged' report it a second time. So the stage and the commits get:
    #   sure  = the larger of "staged" and "on disk", occurrence by occurrence;
    #   maybe = what adding 'index' and 'unstaged' would give beyond that: either a second line
    #           or the same one moved, and nothing here can tell which.
    # $wsBefore is read only when it exists: an empty view means "no problem", $null "no data".
    $keys = { param($view) if ($null -ne $wsBefore) { @($wsBefore[$view] -replace "^[^`t]*`t") } }
    $minus = { param($x, $y) $r = [Collections.ArrayList]@($x); foreach ($k in $y) { $r.Remove($k) }; $r }
    $onDisk = @(& $keys 'working tree')
    $stagedBefore = @(& $keys 'index')
    $sure = [Collections.ArrayList]@($stagedBefore + @(& $minus $onDisk $stagedBefore))
    $maybe = [Collections.ArrayList]@(& $minus ($stagedBefore + @(& $keys 'unstaged')) $sure)
    $pools = @{ 'working tree' = [Collections.ArrayList]@($onDisk); 'index' = $sure }
    $diskProof = @{}
    $classify = {
        param($area, $found, $from)
        # Each earlier problem excuses one later problem with the same text, not all of them.
        # The stage and the commits share one pool: an occurrence spent on a commit is not
        # available again for the stage.
        $pool = $pools[$from]
        foreach ($f in $found) {
            $shown, $key = $f -split "`t", 2
            # A file that was untracked has no "before" above. If its SHA-256 did not change it
            # still holds the text it held, so a problem found in it now, staged or committed,
            # was already on disk. Only for files the index did not know: for the others the
            # pool already counts every occurrence, and this would count them again.
            $path = $shown -replace ':\d+: .*$'
            # It is a count, not a set: one occurrence on disk excuses one occurrence found.
            if (-not $diskProof.ContainsKey($path)) {
                $diskProof[$path] = [Collections.ArrayList]@(if (-not $indexBefore.ContainsKey($path) -and $before.ContainsKey($path) -and $before[$path] -eq $after[$path] -and $after[$path] -notmatch '^(dir$|link:)') {
                    @(Get-Whitespace 1 'diff', '--no-index', '--check', '--', '/dev/null', $path) -replace "^[^`t]*`t"
                })
            }
            $label = if ($pool.Contains($key)) { $pool.Remove($key); 'preexistente' }
            elseif ($diskProof[$path].Contains($key)) { $diskProof[$path].Remove($key); 'preexistente' }
            elseif ($from -eq 'index' -and $maybe.Contains($key)) { $maybe.Remove($key); 'origem indeterminada' }
            elseif ($null -eq $wsBefore) { 'origem indeterminada' }
            else { 'novo' }
            "$label [$area] $shown"
        }
    }

    # Stagnation brake, independent of how many delegations there were: the same task over the
    # same repository state is the same attempt. One repeat is let through (a tool may have
    # failed for a moment); the third identical attempt in a row is refused. Rewording the
    # task gets around it: it stops blind repetition, it does not judge progress.
    # "In a row" alone was undone by any delegation in between: fail, audit, fail, audit and
    # the count never left 1. So the slot also keeps how the attempt ended, on a second line:
    # 'passed' (validation_status=passed, written at the end) or 'refused' (stopped here, the
    # executor not called). No second line means the executor was called and the delegation did
    # not pass: failed, incomplete, interrupted or still running, all read the same way. The
    # second count is every earlier slot of the session with this same attempt and no second
    # line, in a row or not, plus this one; the third is refused. A change of HEAD, content or
    # index makes another attempt and starts at 1: whether that change was progress is for the
    # strategist to say, nothing here reads what the agents wrote. Attempts that passed are not
    # counted, so a validation that keeps passing is only ever held by the count in a row.
    $text =@($task.Trim(), $beforeHead) + @($before.Keys | Sort-Object | ForEach-Object { "$_=$($before[$_])" }) + @($indexBefore.Keys | Sort-Object | ForEach-Object { "$_=$($indexBefore[$_])" })
    $attempt = [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes($text -join "`n")))
    $identical = 1
    for ($k = $slot - 1; $k -ge 1 -and @(Get-Content -LiteralPath "$Session\delegation-$k" -ErrorAction SilentlyContinue)[0] -eq $attempt; $k--) { $identical++ }
    $unresolved = 1
    for ($k = 1; $k -lt $slot; $k++) {
        $prior = @(Get-Content -LiteralPath "$Session\delegation-$k" -ErrorAction SilentlyContinue)
        if ($prior.Count -eq 1 -and $prior[0] -eq $attempt) { $unresolved++ }
    }
    $refusal = if ($identical -ge 3) {
        "Estagnacao (delegacao $slot): a mesma tarefa sobre o mesmo estado do repositorio foi tentada $identical vezes seguidas. O Claude NAO foi executado. Mude a abordagem ou encerre com o diagnostico."
    }
    elseif ($unresolved -ge 3) {
        "Estagnacao (delegacao $slot): a mesma tarefa sobre o mesmo estado do repositorio ja rodou sem validacao aprovada nesta sessao e nada mudou desde entao, mesmo com outras delegacoes no meio | unresolved_attempts=$unresolved [com esta; a terceira e recusada]. O Claude NAO foi executado. Mude a abordagem ou encerre com o diagnostico."
    }
    [IO.File]::WriteAllText("$Session\delegation-$slot", $attempt + $(if ($refusal) { "`nrefused" }))
    if ($refusal) { throw $refusal }

    Write-Output "--- CLAUDE EXECUTOR (delegacao $slot) ---"
    # Explicit mode, so the user's default (which may be bypassPermissions) does not apply:
    # auto lets a classifier refuse risky actions, and nobody answers prompts, so whatever
    # would ask is denied. The deny rules catch only the obvious spellings: a net, not a wall.
    $deny = 'git push*', 'git reset --hard*', 'git clean*', 'npm publish*', 'pnpm publish*' |
        ForEach-Object { "Bash($_)"; "PowerShell($_)" }
    # The prompt goes by stdin: PowerShell 5.1 mangles double quotes inside native arguments.
    $OutputEncoding = [Text.UTF8Encoding]::new($false)
    # Set before the call, never after: from here on a failure of this wrapper cannot prove
    # that Claude did nothing.
    $executorStarted = $true
    (Get-Content -Raw -Encoding UTF8 "$PSScriptRoot\executor.md"), $task |
        & claude -p --permission-mode auto --permission-prompts none --disallowedTools $deny
    $exitCode = $LASTEXITCODE

    # A Git failure here must not read as "nothing changed": it becomes the audit state.
    $auditError = $null
    try {
        $afterHead = & git rev-parse -q --verify HEAD
        $afterRef = & git symbolic-ref -q HEAD
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

    # Whitespace check: a state of its own, never mixed with the audit or with Claude's exit
    # code. Only what this delegation touched is checked, each area with the form that sees it:
    # tracked files on disk against HEAD, staged entries (--cached) against HEAD, since a
    # git add changes the index and not the file, and untracked files against nothing.
    # ponytail: paths go on the command line; batch them if a delegation changes thousands.
    $whitespace = 'unavailable (auditoria incompleta)'
    $wsLines = @()
    if (-not $auditError) {
        try {
            $files = @($changed | Where-Object { $_ -match "^(A|M|\?\?)`t" } | ForEach-Object { ($_ -split "`t", 2)[1] } | Where-Object { $after[$_] -notmatch '^(dir$|link:)' })
            $tracked = @($files | Where-Object { $indexAfter.ContainsKey($_) })
            $staged = @($indexChanged | Where-Object { $_ -match "^[AM]`t" } | ForEach-Object { $_.Substring(2) })
            if (-not $afterHead -and ($tracked -or $staged)) { throw 'repositorio sem commit' }
            $committed = $afterHead -ne $beforeHead
            if ($committed -and -not ($beforeHead -and $afterHead)) { throw 'HEAD surgiu ou sumiu durante a delegacao' }
            $wsLines = @(
                if ($tracked) { & $classify 'working tree' (Get-Whitespace 0 (@('diff-index', '--check', 'HEAD', '--') + $tracked)) 'working tree' }
                if ($staged) { & $classify 'index' (Get-Whitespace 0 (@('diff', '--cached', '--check', 'HEAD', '--') + $staged)) 'index' }
                # After a commit the two checks above compare against the new HEAD and see nothing
                # of what was committed. This is the net difference between the two HEADs, not
                # an audit of each commit in between.
                if ($committed) { & $classify 'commit' (Get-Whitespace 0 'diff', '--check', $beforeHead, $afterHead) 'index' }
                foreach ($p in $files | Where-Object { -not $indexAfter.ContainsKey($_) }) {
                    # A file that did not exist is new by definition; an older untracked one has no "before".
                    $label = if ($before.ContainsKey($p)) { 'origem indeterminada' } else { 'novo' }
                    Get-Whitespace 1 'diff', '--no-index', '--check', '--', '/dev/null', $p | ForEach-Object { "$label [nao rastreado] " + ($_ -split "`t", 2)[0] }
                }
            )
            $whitespace = if ($wsLines -like 'novo *') { 'failed' }
            elseif ($wsLines -like 'origem *') { "unavailable (problema de origem indeterminada$(if ($wsBeforeError) { "; checagem inicial falhou: $wsBeforeError" }))" }
            # The initial check failed and nothing above pinned a problem on it: "nothing found"
            # is not "checked", with or without files touched. A new problem stays 'failed'.
            elseif ($wsBeforeError) { "unavailable (checagem inicial falhou: $wsBeforeError)" }
            elseif ($files -or $staged -or $committed) { 'passed' }
            else { 'passed (nenhum arquivo alterado para verificar)' }
        }
        catch { $whitespace = "unavailable ($_)" }
    }

    # Final state of what this wrapper can check. It never says the task itself is done.
    $validation = if ($exitCode -or $whitespace -eq 'failed') { 'failed' }
    elseif ($auditError -or $whitespace -like 'unavailable*') { 'incomplete' }
    else { 'passed' }
    # For the brake's second count. Only 'passed' is recorded: anything else, a wrapper that
    # died before this line included, leaves the slot as an attempt that did not pass. A write
    # that fails leaves it the same way and must not cost the report.
    if ($validation -eq 'passed') { try { [IO.File]::AppendAllText("$Session\delegation-$slot", "`npassed") } catch { } }

    Write-Output "--- EVIDENCIA GIT ---"
    Write-Output "delegation=$slot identical_attempts=$identical [mesma tarefa sobre o mesmo estado; a terceira seguida e recusada]"
    Write-Output "unresolved_attempts=$unresolved [esta, mais as anteriores da sessao com a mesma tarefa sobre o mesmo estado que rodaram sem validacao aprovada, seguidas ou nao; a terceira e recusada]"
    Write-Output "executor_exit: $exitCode$(if ($exitCode) { ' (FALHOU)' })"
    Write-Output "executor_started=yes [o Claude foi chamado nesta delegacao]"
    if ($auditError) { Write-Output "auditoria: FALHOU, snapshot_status=incomplete ($auditError): as alteracoes desta delegacao NAO foram verificadas" }
    else {
        Write-Output "auditoria: ok, snapshot_status=complete"
        Write-Output "working_tree_changes (A novo rastreado; ?? novo nao rastreado; M modificado; D removido; I ainda no disco, mas deixou de ser rastreado e e ignorado):"
        if ($changed) { $changed } else { "(nada)" }
        Write-Output "index_changes (A entrou no stage; M entrada mudou: conteudo, modo ou estagio; D saiu do stage):"
        if ($indexChanged) { $indexChanged } else { "(nada)" }
        # The reference is shown only when it changed: a commit on the same branch keeps the
        # old "hashA -> hashB" line, which is what readers of this field already parse.
        $headWas = "$beforeHead"; $headIs = "$afterHead"
        if ("$afterRef" -ne "$beforeRef") {
            $headWas += " ($(if ($beforeRef) { $beforeRef } else { 'destacado' }))"
            $headIs += " ($(if ($afterRef) { $afterRef } else { 'destacado' }))"
        }
        Write-Output "head_changes: $(if ($headIs -ne $headWas) { "$headWas -> $headIs" } else { '(nada)' })"
        Write-Output "ja pendentes antes: $beforeDirty caminho(s); os que nao aparecem acima ficaram intactos."
    }
    Write-Output "whitespace_check=$whitespace [so o que a delegacao tocou: working tree e index contra HEAD, commits pela diferenca entre o HEAD anterior e o final, nao rastreados contra vazio; sem politica explicita do repositorio (core.whitespace ou atributo whitespace), o CR de fim de linha CRLF e a quebra dura de Markdown (dois espacos apos conteudo, em .md/.markdown) nao contam]"
    $wsLines | Where-Object { $_ -notlike 'preexistente*' } | Select-Object -First 20
    if ($wsLines) { Write-Output "problemas de whitespace: $(@($wsLines -like 'novo *').Count) novo(s), $(@($wsLines -like 'origem *').Count) de origem indeterminada, $(@($wsLines -like 'preexistente*').Count) preexistente(s) nao atribuidos a esta delegacao; ate 20 listados." }
    Write-Output "validation_status=$validation [executor, auditoria e whitespace; nao prova o criterio de pronto da tarefa]"
    Write-Output "limite: compara caminho e SHA-256 antes e depois. Nao prova autoria (outro processo pode ter escrito no intervalo); nao ve nao rastreados ignorados; submodulo ou repositorio aninhado conta como uma entrada; link simbolico e comparado pelo destino, sem ser seguido; pasta que e juncao e lida por dentro, como o Git a lista."
    Write-Output "--- FIM ---"
    # Claude's own code is kept. Only a 0 is replaced: 3 for failed, 2 for incomplete.
    if ($exitCode) { exit $exitCode }
    exit @{ failed = 3; incomplete = 2; passed = 0 }[$validation]
}

$project = & git rev-parse --show-toplevel
if (-not $project) { throw "Abra um terminal dentro do repositorio Git e execute strategist.cmd." }

# Checked once per session, before Codex opens: the delegation depends on these options and
# there is no fallback to a looser mode.
if (-not (Get-Command claude -ErrorAction SilentlyContinue)) { throw "Falha de inicializacao: 'claude' nao esta no PATH." }
$help = (& claude --help) -join "`n"
$missing = '--print', '--permission-mode', '"auto"', '--permission-prompts', '--disallowedTools' | Where-Object { $help -notlike "*$_*" }
if ($missing) {
    throw "Falha de inicializacao: o Claude CLI $(& claude --version) nao oferece $($missing -join ', '). Atualize o Claude Code; nao ha troca automatica por um modo menos restrito."
}

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
            Where-Object { $_.Name -match '^(AGENTS\.md|delegate-claude\.(cmd|ps1)|owner|delegation-\d+)$' -and -not ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) } |
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

    Write-Host "Codex strategist: o Claude opera $project. Codex em sandbox somente leitura; so o launcher de delegacao desta sessao sai dele sem pedir aprovacao."
    Write-Host "Limites: o sandbox nao impede leitura; a regra libera o launcher, nao limita o que o Claude faz depois; filtros de comando e prompts orientam, nao sao barreira de seguranca."
    if ($temp -match '\s') { Write-Host "Aviso: a pasta temporaria tem espaco no caminho; a regra nao casa e cada delegacao vai pedir aprovacao." }

    New-Item -ItemType Directory -Force -Path $rules | Out-Null
    [IO.File]::WriteAllText($rule, (Get-RuleText $launcher))
    # Read-only sandbox: Codex cannot write anywhere, this folder and the rules folder included,
    # so it cannot edit the launcher, the rule or the delegation slots. It can still READ the
    # whole disk. Anything else leaves the sandbox only through an approval the user grants.
    & codex -C $temp --sandbox read-only --ask-for-approval on-request
    $code = $LASTEXITCODE
}
finally {
    # The original error, if any, goes on after this block; nothing here replaces it.
    Remove-Item -LiteralPath $rule -ErrorAction SilentlyContinue
    Remove-Session $temp
}
exit $code

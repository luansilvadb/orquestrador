# Plumbing only. The prompts live beside this file: strategist.md (the orchestrator, Codex or Claude) and executor.md (the executor, Claude or Codex).
# Keep this file ASCII-only: strategist.cmd runs it in Windows PowerShell 5.1,
# which reads a .ps1 without BOM as ANSI.
param([string]$Session, [string]$Project)

$ErrorActionPreference = "Stop"

# Config helpers (strategist.config.ps1). In delegate mode only the frozen copy is used.
. "$PSScriptRoot\strategist.config.ps1"

# In delegate mode a failure of this wrapper still ends with the closing line, so whoever reads
# the output can tell "ended" from "still running". Exit code 4. The launcher mode keeps
# PowerShell's own behaviour.
trap {
    if (-not $Session) { break }
    Write-Output "wrapper_error: $_"
    Write-Output "--- FIM ---"
    exit 4
}

# Called internally by the temporary delegate.cmd, with the task on stdin.
if ($Session) {
    # Read as UTF-8 bytes: [Console]::In would decode with the console code page and break accents.
    $task = if ([Console]::IsInputRedirected) { (New-Object IO.StreamReader ([Console]::OpenStandardInput()), ([Text.Encoding]::UTF8)).ReadToEnd() }
    if ([string]::IsNullOrWhiteSpace($task)) {
        throw "Task vazia. Passe a tarefa ao launcher (delegate.ps1 'tarefa') ou por stdin ao delegate.cmd."
    }

    # Frozen copy of the session settings, written by the launcher from a validated object, in
    # the session folder. A session without it stops here.
    $frozen = [IO.File]::ReadAllText("$Session\session-config.json") | ConvertFrom-Json

    Set-Location -LiteralPath $Project

    Write-Output "--- EXECUTOR ---"
    # The prompt goes by stdin: PowerShell 5.1 mangles double quotes inside native arguments.
    $OutputEncoding = [Text.UTF8Encoding]::new($false)
    # Explicit mode, so the user's default (which may be bypassPermissions) does not apply:
    # auto lets a classifier refuse risky actions, and nobody answers prompts, so whatever
    # would ask is denied. The deny rules catch only the obvious spellings: a net, not a wall.
    $deny = 'git push*', 'git reset --hard*', 'git clean*', 'npm publish*', 'pnpm publish*' |
        ForEach-Object { "Bash($_)"; "PowerShell($_)" }
    $modelArgs = @(Get-OrqArgs $frozen.executor)
    $prompt = (Get-Content -Raw -Encoding UTF8 "$PSScriptRoot\executor.md"), $task
    if ($frozen.executor.tool -eq 'codex') {
        # Explicit sandbox, never the user's default (which may be danger-full-access).
        # --ignore-rules: an allow rule, this session's launcher rule included, would run its
        # command outside the sandbox. No prompt argument: codex exec reads it from stdin.
        $prompt | & codex exec --sandbox workspace-write --ignore-rules @modelArgs
    }
    else { $prompt | & claude -p --permission-mode auto --permission-prompts none @modelArgs --disallowedTools $deny }
    $exitCode = $LASTEXITCODE

    # What Git says is changed now. Not a diff of this delegation: whatever was already pending
    # before it shows here too.
    Write-Output "--- GIT ---"
    Write-Output "executor_exit: $exitCode$(if ($exitCode) { ' (FALHOU)' })"
    Write-Output "git status (tudo o que esta alterado agora, inclusive o que ja estava pendente antes desta delegacao):"
    $status = @(& git status --porcelain --untracked-files=all)
    if ($LASTEXITCODE) { "(git status falhou com codigo $LASTEXITCODE)" } elseif ($status) { $status } else { "(nada)" }
    Write-Output "--- FIM ---"
    exit $exitCode
}

$configPath = Get-OrqConfigPath
# The project is always the Git repository of the folder this is started from; it is never saved.
$project = Get-OrqGitRoot (Get-Location).Path
if (-not $project) { throw "Abra um terminal dentro do repositorio Git do projeto e execute strategist.cmd." }

# Settings are read once, here: the answers of the last start, which must be valid (no
# fallback). They open already chosen in the onboarding page, which asks tool, model and effort
# for each role on every start. What the page sends is saved as the default of the next start,
# and only then does the session begin, in this terminal.
$config = Invoke-OrqOnboarding (Read-OrqConfig $configPath) $project
Save-OrqConfig $configPath $config
Write-Host "Projeto     : $project"
Get-OrqText $config | ForEach-Object { Write-Host $_ }
$orchestrator = $config.orchestrator.tool

$temp = Join-Path ([IO.Path]::GetTempPath()) ("strategist-" + [Guid]::NewGuid().ToString("N"))

# The one command the orchestrator may run; whatever else it tries is denied, without asking.
# Measured on Codex 0.162: an allow rule frees a command only when it is the whole command and
# is written bare, with no "&", pipe or ";", so the task cannot come through a pipe; with
# --ask-for-approval never (measured through codex exec) the rule still takes the launcher out
# of the sandbox and anything else fails inside it. Measured on Claude Code 2.1.296: in dontAsk
# mode the allow rule below passes the launcher with a multi-line quoted task and denies it
# when another command follows a ";". This launcher takes the task as its single argument and
# hands it to the .cmd by stdin. It ends with [Environment]::Exit because "pwsh -Command
# script" turns any failing exit code into 1.
$launcher = "$temp\delegate.ps1"
# Codex: the rule names this session's launcher by its full path. Codex only reads rules from
# its own rules folder (a rule inside this temporary folder is ignored, also measured), so the
# file goes there under this session's name and is removed at the end. Claude takes its allow
# rule from a settings file of this session, named on the command line: nothing is left outside.
$rules = Join-Path $(if ($env:CODEX_HOME) { $env:CODEX_HOME } else { "$HOME\.codex" }) 'rules'
$rule = Join-Path $rules ((Split-Path $temp -Leaf) + '.rules')
function Get-RuleText([string]$path) { "prefix_rule(pattern=[`"$($path.Replace('\', '\\'))`"], decision=`"allow`", justification=`"Delegacao autorizada ao executor`")`n" }

# The other thing the orchestrator may run: the programs of the user's global skills (the
# grilling canvas, for one), each by its interpreter. They are named one by one, as found now,
# in both spellings of the path, since the orchestrator may write either slash. Measured on
# Claude Code 2.1.296: a rule with a wildcard inside the path does not pass in dontAsk mode, a
# rule naming the file does. Whatever those programs can do, the orchestrator can do through
# them: the user owns these folders and what goes into them.
$globalDirs = @('.agents', '.claude' | ForEach-Object { "$env:USERPROFILE\$_" } | Where-Object { Test-Path -LiteralPath "$_\skills" -PathType Container })
$runners = @{ node = '\.(mjs|cjs|js)$'; python = '\.py$' }
function Get-SkillPrograms([string]$runner) {
    $globalDirs | ForEach-Object { [IO.Directory]::EnumerateFiles("$_\skills", '*', 'AllDirectories') } |
        Where-Object { $_ -match $runners[$runner] -and $_ -notmatch '\\node_modules\\' } | ForEach-Object { $_; $_.Replace('\', '/') }
}

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
            Where-Object { $_.Name -match '^(AGENTS\.md|delegate\.(cmd|ps1)|owner|session-config\.json|claude-settings\.json)$' -and -not ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) } |
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
# fails on the way: a write, the old rules, the rule of this session or the orchestrator. It does
# not cover a killed process; that is what the recovery of leftovers, in the middle, is for.
try {
    New-Item -ItemType Directory -Path $temp | Out-Null
    # WriteAllText writes UTF-8 without BOM, which a .cmd needs. "%~dp0." is the session folder.
    [IO.File]::WriteAllText("$temp\delegate.cmd", "@powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Session `"%~dp0.`" -Project `"$project`"`r`n")
    [IO.File]::WriteAllText($launcher, "param([Parameter(Mandatory = `$true, Position = 0)][string]`$Task)`r`n`$OutputEncoding = [Text.UTF8Encoding]::new(`$false)`r`n`$Task | & `"`$PSScriptRoot\delegate.cmd`"`r`n[Environment]::Exit(`$LASTEXITCODE)`r`n")
    [IO.File]::WriteAllText("$temp\owner", "$PID $((Get-Process -Id $PID).StartTime.ToFileTimeUtc()) $(Split-Path $temp -Leaf)")
    # Frozen copy of this session's settings: every delegation reads it, never the global file.
    [IO.File]::WriteAllText("$temp\session-config.json", ($config | ConvertTo-Json -Depth 4))
    [IO.File]::WriteAllText("$temp\AGENTS.md", [IO.File]::ReadAllText("$PSScriptRoot\strategist.md").Replace('{{DELEGATE}}', $launcher).Replace('{{PROJECT}}', $project))

    # A Codex session that was killed leaves its rule in force for as long as its launcher exists.
    # Both are removed here, but only on proof: the rule file has the exact name and first line this
    # script writes for that folder, the folder names itself as that session ("pid start
    # session" in owner), and the owner is provably gone. Anything short of that is left
    # untouched, and so is a rule that cannot be read or checked: one bad old rule neither
    # gets deleted nor stops this session from starting.
    Get-ChildItem -LiteralPath $rules -Filter 'strategist-*.rules' -File -ErrorAction SilentlyContinue |
        Where-Object { $_.BaseName -match '^strategist-[0-9a-f]{32}$' } | ForEach-Object {
            $file = $_
            try {
                $old = [IO.Path]::GetTempPath().TrimEnd('\') + '\' + $file.BaseName
                if (@([IO.File]::ReadAllLines($file.FullName))[0] -ne (Get-RuleText "$old\delegate.ps1").Trim()) { return }
                if (-not (Test-Path -LiteralPath $old)) { Remove-Item -LiteralPath $file.FullName -ErrorAction SilentlyContinue; return }
                $owner = "$(Get-Content -LiteralPath "$old\owner" -ErrorAction SilentlyContinue)" -split ' '
                if ($owner.Count -ne 3 -or $owner[0] -notmatch '^\d{1,9}$' -or $owner[1] -notmatch '^\d+$' -or $owner[2] -ne $file.BaseName) { return }
                if ((Get-OwnerState $owner[0] $owner[1]) -ne 'dead') { return }
                Remove-Item -LiteralPath $file.FullName -ErrorAction SilentlyContinue
                Remove-Session $old
            }
            catch { Write-Host "Aviso: a regra antiga $($file.Name) nao pode ser verificada e foi preservada: $($_.Exception.Message)" }
        }

    Write-Host "Strategist ($orchestrator): le o projeto e delega; o executor ($($config.executor.tool)) opera $project. Do orquestrador so passam o launcher de delegacao desta sessao e os programas das skills globais; o resto e negado, sem pedir aprovacao."
    Write-Host "Limites: a regra libera o launcher, nao limita o que o executor faz depois; filtros de comando e prompts orientam, nao sao barreira de seguranca."
    if ($temp -match '\s') { Write-Host "Aviso: a pasta temporaria tem espaco no caminho; a regra pode nao casar, e ai toda delegacao e negada." }

    if ($orchestrator -eq 'codex') {
        New-Item -ItemType Directory -Force -Path $rules | Out-Null
        # After the launcher's line, one rule per interpreter naming every skill program found now.
        $skillRules = foreach ($runner in $runners.Keys | Sort-Object) {
            $files = @(Get-SkillPrograms $runner | ForEach-Object { '"' + $_.Replace('\', '\\') + '"' }) -join ', '
            if ($files) { "prefix_rule(pattern=[`"$runner`", [$files]], decision=`"allow`", justification=`"Programa de skill global do usuario`")`n" }
        }
        [IO.File]::WriteAllText($rule, (Get-RuleText $launcher) + (-join $skillRules))
        # Read-only sandbox: Codex cannot write anywhere, this folder and the rules folder included,
        # so it cannot edit the launcher, the rule or the delegation slots. It can still READ the
        # whole disk. Nobody approves anything: what the rule does not free fails in the sandbox.
        & codex -C $temp --sandbox read-only --ask-for-approval never @(Get-OrqArgs $config.orchestrator)
    }
    else {
        # No sandbox here, so the tools are the wall: --restricted leaves only the ones named
        # (no edit tool exists), ignores the user's settings and confines Read to the project
        # and the user's global folders; dontAsk denies every PowerShell command but the
        # launcher, the skill programs and what Claude Code itself takes as read-only; no MCP
        # server is loaded. Restricted mode drops the user's skills with the user's settings, so
        # .agents comes back as a plugin (measured: its skills load as ".agents:name").
        # ponytail: a killed Claude session leaves its temporary folder behind (no rule points
        # at it, so the recovery above never sees it); sweep by owner if they pile up.
        $skillAllow = @(foreach ($runner in $runners.Keys | Sort-Object) { Get-SkillPrograms $runner | ForEach-Object { "PowerShell($runner $_ *)" } })
        [IO.File]::WriteAllText("$temp\claude-settings.json", (@{ permissions = @{ allow = @('Read', 'Glob', 'Grep', 'Skill', "PowerShell($launcher *)") + $skillAllow } } | ConvertTo-Json -Depth 4))
        $skillArgs = @(if ($globalDirs) { '--plugin-dir'; $globalDirs[0]; '--add-dir'; $globalDirs })
        Set-Location -LiteralPath $project
        & claude --restricted --strict-mcp-config --tools 'Read,Glob,Grep,PowerShell,Skill' --settings "$temp\claude-settings.json" --permission-mode dontAsk --append-system-prompt-file "$temp\AGENTS.md" @skillArgs @(Get-OrqArgs $config.orchestrator)
    }
    $code = $LASTEXITCODE
}
finally {
    # The original error, if any, goes on after this block; nothing here replaces it.
    Remove-Item -LiteralPath $rule -ErrorAction SilentlyContinue
    Remove-Session $temp
}
exit $code

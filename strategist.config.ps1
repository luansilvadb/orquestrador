# Pre-session configuration and menu, dot-sourced by strategist.ps1 (and by its tests).
# Keep this file ASCII-only: Windows PowerShell 5.1 reads a .ps1 without BOM as ANSI.
# Nothing here calls a model, the network or Invoke-Expression; model and effort travel as
# literal arguments.

# Per role: the tools that exist and, for the ones that do not work, the reason. A tool
# with a reason is rejected with that reason, never replaced by another one.
$script:OrqTools = @{
    orchestrator = [ordered]@{
        codex  = $null
        claude = 'Claude como orquestrador ainda nao foi implementado (adiado por decisao do usuario).'
    }
    executor = [ordered]@{
        claude = $null
        codex  = 'no Codex 0.162.1, commit requer Git fora do sandbox.'
    }
}
# Values each CLI accepts for effort (empty keeps the CLI default). Confirmed on the installed
# CLIs: `claude --help` (--effort) and `codex debug models` (union of supported_reasoning_levels;
# the effective set depends on the chosen model, e.g. gpt-5.5 stops at xhigh, ultra only on some).
$script:OrqEfforts = @{
    claude = 'low', 'medium', 'high', 'xhigh', 'max'
    codex  = 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'
}
$script:OrqRoles = 'orchestrator', 'executor'

function Get-OrqConfigPath {
    if (-not $env:LOCALAPPDATA) { throw 'LOCALAPPDATA nao esta definido; nao ha onde guardar a configuracao.' }
    Join-Path $env:LOCALAPPDATA 'orquestrador\config.json'
}

# Folder -> Git top level (backslashes), or $null when it is not inside a Git work tree.
# Never initializes anything.
function Get-OrqGitRoot([string]$folder) {
    if (-not $folder -or $folder.Contains('"') -or -not (Test-Path -LiteralPath $folder -PathType Container)) { return $null }
    $psi = New-Object Diagnostics.ProcessStartInfo 'git', "-C `"$($folder.TrimEnd('\'))`" rev-parse --show-toplevel"
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.StandardOutputEncoding = [Text.Encoding]::UTF8
    $p = [Diagnostics.Process]::Start($psi)
    $out = $p.StandardOutput.ReadToEnd()
    $null = $p.StandardError.ReadToEnd()
    $p.WaitForExit()
    if ($p.ExitCode -or -not $out.Trim()) { return $null }
    $out.Trim().Replace('/', '\')
}

# Error strings for one role (empty array = valid).
function Test-OrqRole([string]$role, $r) {
    $errors = @()
    if (-not $r) { return @("${role}: bloco ausente") }
    $tool = "$($r.tool)"
    if (-not $script:OrqTools[$role].Contains($tool)) {
        return @("${role}: ferramenta '$tool' desconhecida (validas: $($script:OrqTools[$role].Keys -join ', '))")
    }
    $why = $script:OrqTools[$role][$tool]
    if ($why) { return @("${role}: ferramenta '$tool' indisponivel: $why") }
    $model = "$($r.model)"
    # A leading dash would be read as another option; quotes are mangled by PowerShell 5.1.
    if ($model -and $model -notmatch '^[A-Za-z0-9][A-Za-z0-9._:/@+\[\]-]*$') {
        $errors += "${role}: modelo '$model' invalido (use letras, digitos e . _ : / @ + [ ] -, sem espaco, aspas nem iniciar com '-')"
    }
    $effort = "$($r.effort)"
    if ($effort -and $script:OrqEfforts[$tool] -cnotcontains $effort) {
        $errors += "${role}: esforco '$effort' invalido para $tool (validos: $($script:OrqEfforts[$tool] -join ', '); alguns modelos aceitam so parte; vazio mantem o padrao do CLI)"
    }
    $errors
}

# Error strings for a whole config object.
function Test-OrqConfig($cfg) {
    $errors = @()
    if (-not $cfg) { return @('configuracao vazia') }
    if (-not "$($cfg.project)") { $errors += 'project: pasta nao definida' }
    elseif (-not (Test-Path -LiteralPath "$($cfg.project)" -PathType Container)) { $errors += "project: a pasta nao existe: $($cfg.project)" }
    elseif (-not (Get-OrqGitRoot "$($cfg.project)")) { $errors += "project: a pasta nao esta em um repositorio Git valido (o Git nao e inicializado automaticamente): $($cfg.project)" }
    foreach ($role in $script:OrqRoles) { $errors += @(Test-OrqRole $role $cfg.$role) }
    $errors
}

function New-OrqConfig([string]$project = '') {
    [pscustomobject]@{
        version      = 1
        project      = $project
        orchestrator = [pscustomobject]@{ tool = 'codex'; model = ''; effort = '' }
        executor     = [pscustomobject]@{ tool = 'claude'; model = ''; effort = '' }
    }
}

# Reads and validates a config file. $null when the file does not exist (legacy behaviour);
# throws, with the reason, when it exists and is invalid. No fallback.
function Read-OrqConfig([string]$path) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return $null }
    try { $cfg = [IO.File]::ReadAllText($path, [Text.Encoding]::UTF8) | ConvertFrom-Json }
    catch { throw "Configuracao invalida (JSON ilegivel) em ${path}: $($_.Exception.Message)" }
    $errors = @(Test-OrqConfig $cfg)
    if ($errors) { throw "Configuracao invalida em ${path}: $($errors -join '; ')" }
    $cfg
}

function Save-OrqConfig($cfg, [string]$path) {
    $errors = @(Test-OrqConfig $cfg)
    if ($errors) { throw "Nao salvo, configuracao invalida: $($errors -join '; ')" }
    $dir = Split-Path -Parent $path
    if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
    $tmp = "$path.tmp"
    [IO.File]::WriteAllText($tmp, ($cfg | ConvertTo-Json -Depth 4), (New-Object Text.UTF8Encoding $false))
    Move-Item -LiteralPath $tmp -Destination $path -Force
}

# Extra arguments for each CLI, only for what is set. Literal array elements: no shell parse.
function Get-OrqClaudeArgs($role) {
    @(if ("$($role.model)") { '--model'; "$($role.model)" }; if ("$($role.effort)") { '--effort'; "$($role.effort)" })
}
function Get-OrqCodexArgs($role) {
    # The TOML value is left unquoted: Codex falls back to a literal string, and PowerShell
    # 5.1 would mangle embedded double quotes.
    @(if ("$($role.model)") { '-m'; "$($role.model)" }; if ("$($role.effort)") { '-c'; "model_reasoning_effort=$($role.effort)" })
}

function Get-OrqText($cfg) {
    $project = if ($cfg.project) { $cfg.project } else { '(nao definida)' }
    $lines = @("Pasta do projeto: $project")
    foreach ($role in $script:OrqRoles) {
        $r = $cfg.$role
        $lines += "$(if ($role -eq 'orchestrator') { 'Orquestrador' } else { 'Executor    ' }): $($r.tool) | modelo: $(if ($r.model) { $r.model } else { '(padrao do CLI)' }) | esforco: $(if ($r.effort) { $r.effort } else { '(padrao do CLI)' })"
    }
    $lines
}

# --- Wizard (TUI): Projeto -> Orquestrador (ferramenta, modelo, esforco) -> Executor -> Revisao.
# One decision per screen. Arrows choose, Enter confirms, Esc goes back (first step: leaves
# without saving). Nothing is written until the review action "Salvar". Input comes from three
# hooks only: $Read (lines), $Write (text, optional color) and $ReadKey (one key token per call:
# Up, Down, Enter, Esc, Backspace, Eof, Ctrl+C, or a single character).

$script:OrqStages = 'Projeto', 'Orquestrador', 'Executor', 'Revisao'
$script:OrqRoleNames = @{ orchestrator = 'Orquestrador'; executor = 'Executor' }
# Short UI notes for tools that are listed but cannot be chosen (the long reason stays in OrqTools).
$script:OrqToolNotes = @{
    'orchestrator.claude' = 'adiado nesta versao'
}

# Optional, offline suggestion list from Codex's local models cache. Any problem -> empty.
function Get-OrqCodexCatalog {
    $out = @()
    try {
        $dir = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex' }
        $file = Join-Path $dir 'models_cache.json'
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { return @() }
        $j = [IO.File]::ReadAllText($file, [Text.Encoding]::UTF8) | ConvertFrom-Json
        foreach ($m in @($j.models)) {
            try {
                $slug = "$($m.slug)"
                if ($m.slug -isnot [string] -or $slug -notmatch '^[A-Za-z0-9][A-Za-z0-9._:/@+\[\]-]*$') { continue }
                $levels = @(foreach ($x in @($m.supported_reasoning_levels)) {
                        $e = if ($x -is [string]) { $x } else { "$($x.effort)" }
                        if ($e -and ($script:OrqEfforts.codex -ccontains $e)) { $e }
                    })
                $out += [pscustomobject]@{ Slug = $slug; Listed = ("$($m.visibility)" -ne 'hide'); Levels = $levels }
            }
            catch { $null = $_ }
        }
    }
    catch { return @() }
    $out
}

# Aliases confirmed by the local `claude --help` text (e.g. 'opus'); none when not stated.
function Get-OrqClaudeAliasesFromHelp([string]$help) {
    $m = [regex]::Match($help, '--model\s+<model>(.{0,400}?)(\r?\n\s*-|\z)', 'Singleline')
    if (-not $m.Success) { return @() }
    $e = [regex]::Match($m.Groups[1].Value, 'e\.g\.\s*(.*?)\)', 'Singleline')
    if (-not $e.Success) { return @() }
    @([regex]::Matches($e.Groups[1].Value, "'([A-Za-z0-9][A-Za-z0-9._-]*)'") | ForEach-Object { $_.Groups[1].Value })
}
function Get-OrqClaudeAliases {
    try {
        if (-not (Get-Command claude -ErrorAction SilentlyContinue)) { return @() }
        $help = (& claude --help 2>&1 | ForEach-Object { "$_" }) -join "`n"
        @(Get-OrqClaudeAliasesFromHelp $help)
    }
    catch { @() }
}

# Efforts to offer for tool+model. Known = the list comes from the local catalog for that model.
function Get-OrqEffortInfo([string]$tool, [string]$model, $catalog) {
    $all = @($script:OrqEfforts[$tool])
    if ($tool -eq 'codex' -and $model) {
        $hit = @($catalog | Where-Object { $_.Slug -ceq $model } | Select-Object -First 1)
        if ($hit.Count -and @($hit[0].Levels).Count) { return [pscustomobject]@{ Levels = @($hit[0].Levels); Known = $true } }
    }
    [pscustomobject]@{ Levels = $all; Known = $false }
}

# Re-fits only the fields that depend on what just changed ('tool' or 'model'). Returns notes.
function Update-OrqRoleDependencies($r, [string]$changed, $catalog) {
    $notes = @()
    if ($changed -eq 'tool' -and $r.model) { $notes += "Modelo '$($r.model)' voltou ao padrao (era de outra ferramenta)."; $r.model = '' }
    if ($r.effort) {
        $info = Get-OrqEffortInfo "$($r.tool)" "$($r.model)" $catalog
        if ($info.Levels -cnotcontains "$($r.effort)") {
            $notes += "Esforco '$($r.effort)' nao vale para $(if ($r.model) { $r.model } else { $r.tool }); voltou ao padrao."
            $r.effort = ''
        }
    }
    $notes
}

function Get-OrqProjectError([string]$p) {
    $e = @(Test-OrqConfig ([pscustomobject]@{ project = $p; orchestrator = $null; executor = $null }) | Where-Object { $_ -like 'project:*' })
    if ($e.Count) { $e[0] -replace '^project:\s*', '' }
}
function Get-OrqModelError([string]$role, [string]$tool, [string]$model) {
    $e = @(Test-OrqRole $role ([pscustomobject]@{ tool = $tool; model = $model; effort = '' }) | Where-Object { $_ -match "^${role}: modelo '" })
    if ($e.Count) { $e[0] -replace "^${role}:\s*", '' }
}

function Test-OrqKeyInput {
    try { if ([Console]::IsInputRedirected) { return $false }; [void][Console]::KeyAvailable; $true }
    catch { $false }
}
function Read-OrqRealKey {
    $k = [Console]::ReadKey($true)
    if (($k.Modifiers -band [ConsoleModifiers]::Control) -and "$($k.Key)" -eq 'C') { return 'Ctrl+C' }
    switch ("$($k.Key)") {
        'UpArrow' { return 'Up' }
        'DownArrow' { return 'Down' }
        'Enter' { return 'Enter' }
        'Escape' { return 'Esc' }
        'Backspace' { return 'Backspace' }
    }
    if ($k.KeyChar -ge ' ') { return [string]$k.KeyChar }
    'Other'
}

# One input token. Key mode: from $ReadKey. Line mode: a typed line mapped to the same tokens
# (number = pick, empty = Enter, up/down/esc/b, q = quit); a closed input is 'Eof'.
function Read-OrqToken($ui, [string]$prompt) {
    if ($ui.KeyMode) {
        $t = & $ui.ReadKey
        if ($null -eq $t -or "$t" -eq 'Ctrl+C') { return 'Eof' }
        return "$t"
    }
    $line = & $ui.Read $prompt
    if ($null -eq $line) { return 'Eof' }
    $x = "$line".Trim().ToLowerInvariant()
    if ($x -eq '' -or $x -eq 'enter') { return 'Enter' }
    if ($x -match '^\d+$') { return "Pick:$x" }
    if ($x -in 'up', 'u') { return 'Up' }
    if ($x -in 'down', 'd') { return 'Down' }
    if ($x -in 'esc', 'b', 'voltar', 'back') { return 'Esc' }
    if ($x -in 'q', 'sair', 'exit', 'quit') { return 'Eof' }
    "Bad:$line"
}

function Show-OrqScreen($ui, $s, [int]$idx, [string]$notice) {
    if ($ui.Clear) { try { & $ui.Clear } catch { $null = $_ } } else { & $ui.Write '' }
    $head = "$($s.Title) - etapa $($s.Stage + 1)/4"
    if ($s.Context) { $head += " | $($s.Context)" }
    & $ui.Write $head
    $trail = for ($k = 0; $k -lt $script:OrqStages.Count; $k++) { if ($k -eq $s.Stage) { "[$($script:OrqStages[$k])]" } else { $script:OrqStages[$k] } }
    & $ui.Write ($trail -join ' > ') 'DarkGray'
    foreach ($n in @($ui.Notes)) { & $ui.Write $n 'Yellow' }
    $ui.Notes = @()
    if ($notice) { & $ui.Write $notice 'Yellow' }
    foreach ($ln in @($s.Lines)) { if ($null -ne $ln) { & $ui.Write $ln } }
    if ($s.Hint) { & $ui.Write $s.Hint 'DarkGray' }
    $opts = @($s.Options)
    for ($k = 0; $k -lt $opts.Count; $k++) {
        $o = $opts[$k]
        $num = if ($ui.KeyMode) { '' } else { "$($k + 1)) " }
        if (-not $o.Enabled) { & $ui.Write "  - $num$($o.Label) ($($o.Note))" 'DarkGray' }
        elseif ($k -eq $idx) { & $ui.Write "> $num$($o.Label)" 'Cyan' }
        else { & $ui.Write "  $num$($o.Label)" }
    }
    $back = if ($s.First) { 'Esc: sair' } else { 'Esc: voltar' }
    if ($s.Text) { & $ui.Write 'Digite e Enter: confirmar | Esc: cancelar' 'DarkGray' }
    elseif ($ui.KeyMode) { & $ui.Write "Setas: escolher | Enter: confirmar | $back" 'DarkGray' }
    else { & $ui.Write "Entrada por linhas: numero escolhe; Enter vazio confirma (>); b volta; q sai." 'DarkGray' }
}

function Step-OrqIndex($opts, [int]$idx, [int]$d) {
    $n = @($opts).Count
    for ($k = 1; $k -le $n; $k++) {
        $j = (($idx + $d * $k) % $n + $n) % $n
        if ($opts[$j].Enabled) { return $j }
    }
    $idx
}

# Returns @{ Action = ok|back|exit; Index }.
function Select-OrqOption($ui, $s) {
    $opts = @($s.Options)
    $idx = [int]$s.Index
    if ($idx -lt 0 -or $idx -ge $opts.Count -or ($opts.Count -and -not $opts[$idx].Enabled)) {
        $idx = 0
        for ($k = 0; $k -lt $opts.Count; $k++) { if ($opts[$k].Enabled) { $idx = $k; break } }
    }
    $notice = ''
    while ($true) {
        Show-OrqScreen $ui $s $idx $notice
        $notice = ''
        $t = Read-OrqToken $ui 'Opcao'
        if ($t -eq 'Eof') { return @{ Action = 'exit'; Index = $idx } }
        if ($t -eq 'Esc') { return @{ Action = 'back'; Index = $idx } }
        if ($t -eq 'Up') { $idx = Step-OrqIndex $opts $idx -1; continue }
        if ($t -eq 'Down') { $idx = Step-OrqIndex $opts $idx 1; continue }
        if ($t -eq 'Enter') {
            if ($opts.Count -and $opts[$idx].Enabled) { return @{ Action = 'ok'; Index = $idx } }
            $notice = 'Nada selecionavel aqui; Esc volta.'
            continue
        }
        if (-not $ui.KeyMode -and $t -match '^Pick:(\d+)$') {
            $n = [int]$Matches[1]
            if ($n -lt 1 -or $n -gt $opts.Count) { $notice = "Opcao fora da lista: $n."; continue }
            $o = $opts[$n - 1]
            if (-not $o.Enabled) { $notice = "$($o.Label): $($o.Note)."; continue }
            return @{ Action = 'ok'; Index = ($n - 1) }
        }
        $notice = if ($ui.KeyMode) { 'Tecla sem efeito.' } else { 'Entrada nao reconhecida.' }
    }
}

# Free text. Returns @{ Action = ok|back|exit; Text }. Empty text = cancel (back).
function Read-OrqText($ui, $s, [string]$prompt) {
    $s.Options = @(); $s.Text = $true
    Show-OrqScreen $ui $s 0 ''
    & $ui.Write $prompt
    if (-not $ui.KeyMode) {
        $v = & $ui.Read 'Texto'
        if ($null -eq $v) { return @{ Action = 'exit'; Text = '' } }
        $v = "$v".Trim().Trim('"')
        if (-not $v) { return @{ Action = 'back'; Text = '' } }
        return @{ Action = 'ok'; Text = $v }
    }
    $buf = ''
    while ($true) {
        $t = & $ui.ReadKey
        if ($null -eq $t -or "$t" -eq 'Eof' -or "$t" -eq 'Ctrl+C') { return @{ Action = 'exit'; Text = '' } }
        $t = "$t"
        if ($t -eq 'Esc') { return @{ Action = 'back'; Text = '' } }
        if ($t -eq 'Enter') {
            if ($ui.Echo) { & $ui.Echo "`n" }
            $v = $buf.Trim().Trim('"')
            if (-not $v) { return @{ Action = 'back'; Text = '' } }
            return @{ Action = 'ok'; Text = $v }
        }
        if ($t -eq 'Backspace') {
            if ($buf.Length) { $buf = $buf.Substring(0, $buf.Length - 1); if ($ui.Echo) { & $ui.Echo "`b `b" } }
            continue
        }
        if ($t.Length -eq 1) { $buf += $t; if ($ui.Echo) { & $ui.Echo $t } }
    }
}

function Invoke-OrqProjectStep($ui, $draft, [string]$default) {
    while ($true) {
        $cur = "$($draft.project)"
        $opts = @()
        if ($default) { $opts += @{ Label = "Pasta atual: $default"; Value = $default; Enabled = $true } }
        if ($cur -and $cur -ne $default) { $opts += @{ Label = "Pasta do rascunho: $cur"; Value = $cur; Enabled = $true } }
        $opts += @{ Label = 'Digitar outro caminho...'; Value = $null; Enabled = $true }
        $idx = 0
        for ($k = 0; $k -lt $opts.Count; $k++) { if ($cur -and $opts[$k].Value -eq $cur) { $idx = $k; break } }
        $s = @{ Stage = 0; Title = 'Projeto'; Context = ''; First = $true; Options = $opts; Index = $idx
            Hint = 'Pasta de um repositorio Git onde o trabalho acontece.' }
        $r = Select-OrqOption $ui $s
        if ($r.Action -ne 'ok') { return $r.Action }
        $o = $opts[$r.Index]
        $p = $o.Value
        if ($null -eq $p) {
            $t = Read-OrqText $ui @{ Stage = 0; Title = 'Projeto'; Context = 'outro caminho'; First = $true } 'Caminho da pasta (pode ter espacos):'
            if ($t.Action -eq 'exit') { return 'exit' }
            if ($t.Action -eq 'back') { continue }
            $p = $t.Text
        }
        $err = Get-OrqProjectError $p
        if ($err) { $ui.Notes += "ERRO: $err"; continue }
        $draft.project = $p
        return 'next'
    }
}

function Invoke-OrqRoleStep($ui, $draft, [string]$role, [string]$part, [int]$dir) {
    $r = $draft.$role
    $name = $script:OrqRoleNames[$role]
    $stage = if ($role -eq 'orchestrator') { 1 } else { 2 }
    $skip = if ($dir -lt 0) { 'back' } else { 'next' }

    if ($part -eq 'tool') {
        $opts = @(foreach ($k in $script:OrqTools[$role].Keys) {
                $why = $script:OrqTools[$role][$k]
                $note = if ($why) { if ($script:OrqToolNotes.ContainsKey("$role.$k")) { $script:OrqToolNotes["$role.$k"] } else { 'indisponivel' } }
                @{ Label = $k; Value = $k; Enabled = (-not $why); Note = $note }
            })
        $valid = @($opts | Where-Object { $_.Enabled })
        if ($valid.Count -eq 1) {
            # Only one real choice: advance, the tool shows in the next headers.
            if ("$($r.tool)" -ne $valid[0].Value) {
                $r.tool = $valid[0].Value
                $ui.Notes += @(Update-OrqRoleDependencies $r 'tool' $ui.Catalog)
            }
            return $skip
        }
        $idx = 0
        for ($k = 0; $k -lt $opts.Count; $k++) { if ($opts[$k].Value -eq "$($r.tool)") { $idx = $k } }
        $res = Select-OrqOption $ui @{ Stage = $stage; Title = "${name}: ferramenta"; Context = ''; Options = $opts; Index = $idx }
        if ($res.Action -ne 'ok') { return $res.Action }
        $new = $opts[$res.Index].Value
        if ($new -ne "$($r.tool)") {
            $r.tool = $new
            $ui.Notes += @(Update-OrqRoleDependencies $r 'tool' $ui.Catalog)
        }
        return 'next'
    }

    $tool = "$($r.tool)"
    $ctx = "ferramenta: $tool"
    if ($part -eq 'model') {
        while ($true) {
            $opts = @(@{ Label = 'Padrao atual do CLI'; Value = ''; Enabled = $true })
            $sug = @()
            if ($tool -eq 'codex') { $sug = @($ui.Catalog | Where-Object { $_.Listed } | ForEach-Object { $_.Slug }) }
            elseif ($tool -eq 'claude') {
                if ($null -eq $ui.Aliases) { $ui.Aliases = @(Get-OrqClaudeAliases) }
                $sug = @($ui.Aliases)
            }
            $cur = "$($r.model)"
            if ($cur -and $sug -cnotcontains $cur) { $opts += @{ Label = "Atual: $cur"; Value = $cur; Enabled = $true } }
            foreach ($m in $sug) { $opts += @{ Label = $m; Value = $m; Enabled = $true } }
            $opts += @{ Label = 'Modelo personalizado...'; Value = $null; Enabled = $true; Custom = $true }
            $idx = 0
            for ($k = 0; $k -lt $opts.Count; $k++) { if (-not $opts[$k].Custom -and $opts[$k].Value -ceq $cur) { $idx = $k; break } }
            $res = Select-OrqOption $ui @{ Stage = $stage; Title = "${name}: modelo"; Context = $ctx; Options = $opts; Index = $idx }
            if ($res.Action -ne 'ok') { return $res.Action }
            $o = $opts[$res.Index]
            $new = $o.Value
            if ($o.Custom) {
                $t = Read-OrqText $ui @{ Stage = $stage; Title = "${name}: modelo"; Context = $ctx } 'Nome do modelo (sem espacos):'
                if ($t.Action -eq 'exit') { return 'exit' }
                if ($t.Action -eq 'back') { continue }
                $err = Get-OrqModelError $role $tool $t.Text
                if ($err) { $ui.Notes += "ERRO: $err"; continue }
                $new = $t.Text
            }
            if ($new -cne $cur) {
                $r.model = $new
                $ui.Notes += @(Update-OrqRoleDependencies $r 'model' $ui.Catalog)
            }
            return 'next'
        }
    }

    # effort
    $info = Get-OrqEffortInfo $tool "$($r.model)" $ui.Catalog
    $opts = @(@{ Label = 'Padrao atual do CLI'; Value = ''; Enabled = $true })
    foreach ($e in $info.Levels) { $opts += @{ Label = $e; Value = $e; Enabled = $true } }
    $idx = 0
    for ($k = 0; $k -lt $opts.Count; $k++) { if ($opts[$k].Value -ceq "$($r.effort)") { $idx = $k; break } }
    $mdl = if ($r.model) { $r.model } else { 'padrao' }
    $hint = if ($info.Known) { "Niveis aceitos por $($r.model) (cache local do Codex)." }
    else { 'Nao conferido com o modelo; a ferramenta confirma ao executar.' }
    $res = Select-OrqOption $ui @{ Stage = $stage; Title = "${name}: esforco"; Context = "$ctx | modelo: $mdl"; Options = $opts; Index = $idx; Hint = $hint }
    if ($res.Action -ne 'ok') { return $res.Action }
    $r.effort = $opts[$res.Index].Value
    'next'
}

function Invoke-OrqReviewStep($ui, $draft, [string]$ConfigPath) {
    while ($true) {
        $lines = @("Projeto: $($draft.project)")
        $hints = @()
        foreach ($role in $script:OrqRoles) {
            $r = $draft.$role
            $lines += "$($script:OrqRoleNames[$role]): $($r.tool) | modelo: $(if ($r.model) { $r.model } else { 'padrao' }) | esforco: $(if ($r.effort) { $r.effort } else { 'padrao' })"
            if ($r.effort -and -not (Get-OrqEffortInfo "$($r.tool)" "$($r.model)" $ui.Catalog).Known) {
                $hints += "$($script:OrqRoleNames[$role]): esforco nao conferido com o modelo; a ferramenta confirma."
            }
        }
        $opts = @(
            @{ Label = 'Salvar e iniciar'; Value = 'start'; Enabled = $true }
            @{ Label = 'Salvar e sair'; Value = 'saveexit'; Enabled = $true }
            @{ Label = 'Voltar'; Value = 'back'; Enabled = $true }
            @{ Label = 'Sair sem salvar'; Value = 'exit'; Enabled = $true }
        )
        $res = Select-OrqOption $ui @{ Stage = 3; Title = 'Revisao'; Context = ''; Options = $opts; Index = 0; Lines = $lines; Hint = ($hints -join ' ') }
        if ($res.Action -ne 'ok') { return $res.Action }
        $v = $opts[$res.Index].Value
        if ($v -eq 'back') { return 'back' }
        if ($v -eq 'exit') { return 'exit' }
        try { Save-OrqConfig $draft $ConfigPath }
        catch {
            $msg = $_.Exception.Message -replace '^Nao salvo, configuracao invalida:\s*', ''
            $ui.Notes += "ERRO: nao salvo. $msg"
            continue
        }
        & $ui.Write "Salvo em $ConfigPath" 'Green'
        return $(if ($v -eq 'start') { 'start' } else { 'exit' })
    }
}

# Returns 'start' (the draft was validated and saved, then "Salvar e iniciar" chosen) or 'exit'.
# Leaving, Esc on the first step and a closed input never save. Given -Read without -ReadKey,
# input is by lines; with neither, a real console uses Console.ReadKey and a redirected or
# console-less host falls back to lines.
function Invoke-OrqMenu([string]$ConfigPath,
    [scriptblock]$Read = { param($p) Write-Host "${p}: " -NoNewline; [Console]::In.ReadLine() },
    [scriptblock]$Write = { param($t, $c) if ($c) { Write-Host $t -ForegroundColor $c } else { Write-Host $t } },
    [string]$DefaultProject = '', [scriptblock]$ReadKey = $null) {
    $ui = @{ Read = $Read; Write = $Write; ReadKey = $ReadKey; KeyMode = $false; Clear = $null; Echo = $null
        Notes = @(); Catalog = @(Get-OrqCodexCatalog); Aliases = $null }
    if ($ReadKey) { $ui.KeyMode = $true }
    elseif (-not $PSBoundParameters.ContainsKey('Read') -and (Test-OrqKeyInput)) {
        $ui.KeyMode = $true
        $ui.ReadKey = { Read-OrqRealKey }
        $ui.Echo = { param($x) [Console]::Write($x) }
        if (-not [Console]::IsOutputRedirected) { $ui.Clear = { Clear-Host } }
    }
    $draft = $null
    try { $draft = Read-OrqConfig $ConfigPath }
    catch { $ui.Notes += 'AVISO: configuracao salva invalida; o arquivo so e substituido se voce salvar.' }
    if (-not $draft) { $draft = New-OrqConfig $DefaultProject }

    $steps = 'project', 'orchestrator:tool', 'orchestrator:model', 'orchestrator:effort', 'executor:tool', 'executor:model', 'executor:effort', 'review'
    $i = 0; $dir = 1
    while ($true) {
        $name = $steps[$i]
        if ($name -eq 'project') { $a = Invoke-OrqProjectStep $ui $draft $DefaultProject }
        elseif ($name -eq 'review') { $a = Invoke-OrqReviewStep $ui $draft $ConfigPath }
        else { $role, $part = $name -split ':'; $a = Invoke-OrqRoleStep $ui $draft $role $part $dir }
        if ($a -eq 'start') { return 'start' }
        if ($a -eq 'exit') { return 'exit' }
        if ($a -eq 'next') { $i++; $dir = 1 }
        else {
            if ($i -eq 0) { return 'exit' }
            $i--; $dir = -1
        }
    }
}

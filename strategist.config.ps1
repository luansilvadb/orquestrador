# Session configuration, dot-sourced by strategist.ps1 (and by its tests).
# Keep this file ASCII-only: Windows PowerShell 5.1 reads a .ps1 without BOM as ANSI.
# Nothing here calls a model, the network or Invoke-Expression; model and effort travel as
# literal arguments.

# The tools that exist, per role.
$script:OrqTools = @{
    orchestrator = @('codex')
    executor     = 'claude', 'codex'
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

# Git's stdout read as UTF-8 (PowerShell 5.1 would decode it with the console code page), run
# in the current location. Throws when Git fails.
function Invoke-OrqGit([string]$arguments) {
    $psi = New-Object Diagnostics.ProcessStartInfo 'git', $arguments
    $psi.WorkingDirectory = (Get-Location).Path
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.StandardOutputEncoding = [Text.Encoding]::UTF8
    $p = [Diagnostics.Process]::Start($psi)
    $out = $p.StandardOutput.ReadToEnd()
    $p.WaitForExit()
    if ($p.ExitCode) { throw "git $arguments saiu com $($p.ExitCode)" }
    $out
}

# Folder -> Git top level (backslashes), or $null when it is not inside a Git work tree.
# Never initializes anything.
function Get-OrqGitRoot([string]$folder) {
    if (-not $folder -or $folder.Contains('"') -or -not (Test-Path -LiteralPath $folder -PathType Container)) { return $null }
    try { (Invoke-OrqGit "-C `"$($folder.TrimEnd('\'))`" rev-parse --show-toplevel").Trim().Replace('/', '\') } catch { $null }
}

# Error strings for one role (empty array = valid).
function Test-OrqRole([string]$role, $r) {
    $errors = @()
    if (-not $r) { return @("${role}: bloco ausente") }
    $tool = "$($r.tool)"
    if ($script:OrqTools[$role] -notcontains $tool) {
        return @("${role}: ferramenta '$tool' desconhecida (validas: $($script:OrqTools[$role] -join ', '))")
    }
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

# The defaults, and the shape of config.json (edited by hand; empty model or effort keeps the
# CLI default).
function New-OrqConfig([string]$project) {
    [pscustomobject]@{
        project      = $project
        orchestrator = [pscustomobject]@{ tool = 'codex'; model = ''; effort = '' }
        executor     = [pscustomobject]@{ tool = 'claude'; model = ''; effort = '' }
    }
}

# Reads and validates a config file. $null when the file does not exist (the caller applies the
# defaults); throws, with the reason, when it exists and is invalid. No fallback.
function Read-OrqConfig([string]$path) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return $null }
    try { $cfg = [IO.File]::ReadAllText($path, [Text.Encoding]::UTF8) | ConvertFrom-Json }
    catch { throw "Configuracao invalida (JSON ilegivel) em ${path}: $($_.Exception.Message)" }
    $errors = @(Test-OrqConfig $cfg)
    if ($errors) { throw "Configuracao invalida em ${path}: $($errors -join '; ')" }
    $cfg
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
    $lines = @("Pasta do projeto: $($cfg.project)")
    foreach ($role in $script:OrqRoles) {
        $r = $cfg.$role
        $lines += "$(if ($role -eq 'orchestrator') { 'Orquestrador' } else { 'Executor    ' }): $($r.tool) | modelo: $(if ($r.model) { $r.model } else { '(padrao do CLI)' }) | esforco: $(if ($r.effort) { $r.effort } else { '(padrao do CLI)' })"
    }
    $lines
}

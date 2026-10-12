# Session configuration, dot-sourced by strategist.ps1 (and by its tests).
# Keep this file ASCII-only: Windows PowerShell 5.1 reads a .ps1 without BOM as ANSI.
# Nothing here calls a model, the network or Invoke-Expression; model and effort travel as
# literal arguments.

# Everything that differs per tool: the roles it can take, how its CLI receives model and
# effort ({0} is the value), and the values it accepts for effort (empty keeps the CLI default).
# Efforts confirmed on the installed CLIs: `claude --help` (--effort) and `codex debug models`
# (union of supported_reasoning_levels; the effective set depends on the chosen model, e.g.
# gpt-5.5 stops at xhigh, ultra only on some).
# The Codex effort is a TOML value left unquoted: Codex falls back to a literal string, and
# PowerShell 5.1 would mangle embedded double quotes.
$script:OrqTools = @{
    claude = @{ roles = 'orchestrator', 'executor'; model = '--model', '{0}'; effort = '--effort', '{0}'
        efforts = 'low', 'medium', 'high', 'xhigh', 'max' }
    codex  = @{ roles = 'orchestrator', 'executor'; model = '-m', '{0}'; effort = '-c', 'model_reasoning_effort={0}'
        efforts = 'low', 'medium', 'high', 'xhigh', 'max', 'ultra' }
}
$script:OrqRoles = 'orchestrator', 'executor'
# A leading dash would be read as another option; quotes are mangled by PowerShell 5.1.
$script:OrqModelPattern = '^[A-Za-z0-9][A-Za-z0-9._:/@+\[\]-]*$'

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
    $t = $script:OrqTools[$tool]
    if (-not $t -or $t.roles -notcontains $role) {
        $valid = $script:OrqTools.Keys | Where-Object { $script:OrqTools[$_].roles -contains $role } | Sort-Object
        return @("${role}: ferramenta '$tool' desconhecida (validas: $($valid -join ', '))")
    }
    $model = "$($r.model)"
    if ($model -and $model -notmatch $script:OrqModelPattern) {
        $errors += "${role}: modelo '$model' invalido (use letras, digitos e . _ : / @ + [ ] -, sem espaco, aspas nem iniciar com '-')"
    }
    $effort = "$($r.effort)"
    if ($effort -and $t.efforts -cnotcontains $effort) {
        $errors += "${role}: esforco '$effort' invalido para $tool (validos: $($t.efforts -join ', '); alguns modelos aceitam so parte; vazio mantem o padrao do CLI)"
    }
    $errors
}

# Error strings for a whole config object.
function Test-OrqConfig($cfg) {
    $errors = @()
    if (-not $cfg) { return @('configuracao vazia') }
    foreach ($role in $script:OrqRoles) { $errors += @(Test-OrqRole $role $cfg.$role) }
    $errors
}

# The defaults, and the shape of config.json (empty model or effort keeps the CLI default).
# The project is never stored: it is the Git repository of the folder the launcher starts in.
function New-OrqConfig {
    [pscustomobject]@{
        orchestrator = [pscustomobject]@{ tool = 'codex'; model = ''; effort = '' }
        executor     = [pscustomobject]@{ tool = 'claude'; model = ''; effort = '' }
    }
}

# The defaults with whatever the file sets laid over them, validated: the file holds the
# answers of the last start, and no file at all means the defaults. Throws, with the reason, when the file
# is unreadable or the result is invalid. No fallback.
function Read-OrqConfig([string]$path) {
    $cfg = New-OrqConfig
    if (Test-Path -LiteralPath $path -PathType Leaf) {
        try { $file = [IO.File]::ReadAllText($path, [Text.Encoding]::UTF8) | ConvertFrom-Json }
        catch { throw "Configuracao invalida (JSON ilegivel) em ${path}: $($_.Exception.Message)" }
        foreach ($role in $script:OrqRoles) { foreach ($key in 'tool', 'model', 'effort') { if ($file.$role.$key) { $cfg.$role.$key = "$($file.$role.$key)" } } }
    }
    $errors = @(Test-OrqConfig $cfg)
    if ($errors) { throw "Configuracao invalida em ${path}: $($errors -join '; ')" }
    $cfg
}

# Extra arguments for the role's CLI, only for what is set. Literal array elements: no shell parse.
function Get-OrqArgs($role) {
    $t = $script:OrqTools["$($role.tool)"]
    @(foreach ($key in 'model', 'effort') { if ("$($role.$key)") { $t[$key] | ForEach-Object { $_ -f "$($role.$key)" } } })
}

function Get-OrqText($cfg) {
    foreach ($role in $script:OrqRoles) {
        $r = $cfg.$role
        "$(if ($role -eq 'orchestrator') { 'Orquestrador' } else { 'Executor    ' }): $($r.tool) | modelo: $(if ($r.model) { $r.model } else { '(padrao do CLI)' }) | esforco: $(if ($r.effort) { $r.effort } else { '(padrao do CLI)' })"
    }
}

function Save-OrqConfig([string]$path, $cfg) {
    New-Item -ItemType Directory -Force -Path (Split-Path $path) | Out-Null
    [IO.File]::WriteAllText($path, ($cfg | ConvertTo-Json -Depth 4))
}

# The models offered for a tool, each with the efforts it accepts. Claude has no command that
# lists models: its aliases resolve to the latest of each family when the CLI is called. Codex
# is asked on every start; when it cannot answer, the list is empty and a name can still be typed.
function Get-OrqModels([string]$tool) {
    if ($tool -eq 'claude') { return @('fable', 'opus', 'sonnet', 'haiku' | ForEach-Object { [pscustomobject]@{ name = $_; efforts = $script:OrqTools.claude.efforts } }) }
    if ($null -eq $script:OrqCodexModels) {
        try {
            $script:OrqCodexModels = @(((& codex debug models) -join '' | ConvertFrom-Json).models | Where-Object { $_.visibility -eq 'list' } | ForEach-Object {
                    [pscustomobject]@{ name = "$($_.slug)"; efforts = @($_.supported_reasoning_levels | ForEach-Object { "$($_.effort)" }) } })
        }
        catch { $script:OrqCodexModels = @(); Write-Host "Aviso: 'codex debug models' nao devolveu a lista de modelos ($($_.Exception.Message)); digite o nome do modelo ou mantenha o padrao do CLI." }
    }
    $script:OrqCodexModels
}

# The start: a page served on the loopback asks tool, model and effort for each role, with the
# saved answers already chosen. Returns the answers once the page sends a valid set; an invalid
# one goes back to the page as a list of errors and the wait goes on. The address carries a
# random token, so another page open in the browser cannot post here. Ctrl+C gives up: nothing
# is returned and nothing gets saved. ORQUESTRADOR_SEM_NAVEGADOR, when set, only prints the address.
function Invoke-OrqOnboarding($cfg, [string]$project) {
    $data = [pscustomobject]@{
        project = $project
        config  = $cfg
        tools   = @($script:OrqTools.Keys | Sort-Object | ForEach-Object {
                [pscustomobject]@{ name = $_; roles = @($script:OrqTools[$_].roles); efforts = @($script:OrqTools[$_].efforts); models = @(Get-OrqModels $_) } })
    }
    # ConvertTo-Json escapes the angle brackets, so the data cannot close the script element it sits in.
    $page = [IO.File]::ReadAllText("$PSScriptRoot\strategist.onboarding.html", [Text.Encoding]::UTF8).Replace('{{DATA}}', ($data | ConvertTo-Json -Depth 6 -Compress))
    # HttpListener takes no port 0: a free port is asked from the system first.
    $probe = New-Object Net.Sockets.TcpListener ([Net.IPAddress]::Loopback), 0
    $probe.Start(); $port = $probe.LocalEndpoint.Port; $probe.Stop()
    $url = "http://127.0.0.1:$port/$([Guid]::NewGuid().ToString('N'))/"
    $listener = New-Object Net.HttpListener
    $listener.Prefixes.Add($url)
    $listener.Start()
    try {
        Write-Host "Onboarding: $url"
        Write-Host 'Escolha no navegador e clique em "Salvar e iniciar". Ctrl+C cancela.'
        if (-not $env:ORQUESTRADOR_SEM_NAVEGADOR) { Start-Process $url }
        while ($true) {
            # Waited in slices: a blocking GetContext would not let Ctrl+C through.
            $pending = $listener.GetContextAsync()
            while (-not $pending.Wait(200)) { }
            $ctx = $pending.Result
            $status = 200; $type = 'text/html'; $body = $page; $new = $null
            if ($ctx.Request.HttpMethod -eq 'POST') {
                try {
                    $sent = (New-Object IO.StreamReader $ctx.Request.InputStream, ([Text.Encoding]::UTF8)).ReadToEnd() | ConvertFrom-Json
                    $new = New-OrqConfig
                    foreach ($role in $script:OrqRoles) { foreach ($key in 'tool', 'model', 'effort') { $new.$role.$key = "$($sent.$role.$key)" } }
                    $errors = @(Test-OrqConfig $new)
                }
                catch { $errors = @("pedido ilegivel: $($_.Exception.Message)") }
                if ($errors) { $status = 400; $new = $null }
                $type = 'application/json'; $body = [pscustomobject]@{ errors = $errors } | ConvertTo-Json -Compress
            }
            $bytes = [Text.Encoding]::UTF8.GetBytes($body)
            $ctx.Response.StatusCode = $status
            $ctx.Response.ContentType = "$type; charset=utf-8"
            $ctx.Response.Headers['Cache-Control'] = 'no-store'
            $ctx.Response.ContentLength64 = $bytes.Length
            $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
            $ctx.Response.Close()
            if ($new) { return $new }
        }
    }
    finally { $listener.Close() }
}

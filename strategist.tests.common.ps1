# Setup of strategist.tests.ps1: a throwaway root folder, the
# environment pointed at it, a simulated Claude and a simulated Codex first in the PATH, the
# session helpers (New-Repo, Start-Session, Stop-Session, Delegate) and Check.
# Keep this file ASCII-only (it runs in Windows PowerShell 5.1).
$ErrorActionPreference = 'Stop'
$tool = $PSScriptRoot
$root = Join-Path ([IO.Path]::GetTempPath()) ("strategist-tests-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path "$root\bin", "$root\codex-home\rules", "$root\tmp" | Out-Null
$env:TEMP = "$root\tmp"; $env:TMP = "$root\tmp"   # sessions are created here, not in the real temporary folder
$env:LOCALAPPDATA = "$root\appdata"               # the user's real configuration is never read
$env:MOCK_DIR = $root
$env:CODEX_HOME = "$root\codex-home"       # the session rule never reaches the real Codex folder
$env:USERPROFILE = "$root\home"            # the global skills are these, not the user's
$skillProgram = "$root\home\.agents\skills\demo\scripts\tool.mjs"
New-Item -ItemType Directory -Path (Split-Path $skillProgram) | Out-Null
Set-Content $skillProgram 'console.log("skill")'
$env:GIT_CEILING_DIRECTORIES = $root       # Git never climbs out of the test folder
$env:PATH = "$root\bin;$env:PATH"
$env:ORQUESTRADOR_SEM_NAVEGADOR = '1'      # the onboarding page is answered by the tests, no browser opens
$git = (Get-Command git.exe).Source

# Simulated Claude: records its arguments and its task, then acts on the current folder
# according to keywords in the task.
Set-Content "$root\bin\fake-claude.ps1" -Encoding ascii @'
if ($args -notcontains '-p') {
    # As orchestrator: holds the session open like the simulated Codex below.
    Set-Content "$env:MOCK_DIR\args.txt" ($args -join '|')
    Set-Content "$env:MOCK_DIR\cwd.txt" (Get-Location).Path
    Set-Content "$env:MOCK_DIR\session.txt" (Split-Path $args[[Array]::IndexOf($args, '--append-system-prompt-file') + 1])
    while (-not (Test-Path "$env:MOCK_DIR\stop")) { Start-Sleep -Milliseconds 150 }
    exit [int](Get-Content "$env:MOCK_DIR\stop")
}
$ms = New-Object IO.MemoryStream; [Console]::OpenStandardInput().CopyTo($ms)
$task = ([Text.Encoding]::UTF8.GetString($ms.ToArray()) -split 'TAREFA:')[1].Trim()
function Log($file, $text) { foreach ($try in 1..20) { try { Add-Content -LiteralPath "$env:MOCK_DIR\$file" -Value $text -ErrorAction Stop; break } catch { Start-Sleep -Milliseconds 50 } } }
Log 'claude-args.txt' ($args -join '|')
Log 'calls.txt' $task
$here = (Get-Location).Path
function Append($file, $text) { [IO.File]::AppendAllText("$here\$file", $text) }
switch -Regex ($task) {
    '^STEP (\d+)'  { Append 'progress.txt' "step $($Matches[1])`n" }
    '^BADFIX'      { 'ERRO=compilacao falhou em modulo X'; exit 5 }
}
'fake claude: feito'
exit 0
'@
Set-Content "$root\bin\claude.cmd" -Encoding ascii '@powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0fake-claude.ps1" %*'

# Simulated Codex. As executor ("exec") it records its arguments and its stdin. As orchestrator
# it holds the session open until the test asks it to leave with a given code.
Set-Content "$root\bin\fake-codex.ps1" -Encoding ascii @'
if ($args[0] -eq 'exec') {
    $ms = New-Object IO.MemoryStream; [Console]::OpenStandardInput().CopyTo($ms)
    Add-Content -LiteralPath "$env:MOCK_DIR\codex-exec-args.txt" -Value ($args -join '|')
    Add-Content -LiteralPath "$env:MOCK_DIR\codex-exec-stdin.txt" -Value ([Text.Encoding]::UTF8.GetString($ms.ToArray()))
    [IO.File]::AppendAllText("$((Get-Location).Path)\progress.txt", "codex`n")
    'fake codex exec: feito'
    exit 0
}
if ($args[0] -eq 'debug') {
    '{"models":[{"slug":"gpt-a","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"high"},{"effort":"ultra"}]},'
    '{"slug":"gpt-luna","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"}]},'
    '{"slug":"gpt-hidden","visibility":"hide","supported_reasoning_levels":[{"effort":"low"}]}]}'
    exit 0
}
if ($env:MOCK_CODEX -eq 'hang') { Start-Sleep -Seconds 600 }   # never shows the session
Set-Content "$env:MOCK_DIR\args.txt" ($args -join '|')
Set-Content "$env:MOCK_DIR\session.txt" $args[[Array]::IndexOf($args, '-C') + 1]
while (-not (Test-Path "$env:MOCK_DIR\stop")) { Start-Sleep -Milliseconds 150 }
exit [int](Get-Content "$env:MOCK_DIR\stop")
'@
Set-Content "$root\bin\codex.cmd" -Encoding ascii '@powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0fake-codex.ps1" %*'

function New-Repo([string]$name = ('repo-' + [Guid]::NewGuid().ToString('N').Substring(0, 8))) {
    $dir = "$root\$name"
    New-Item -ItemType Directory -Path $dir | Out-Null
    & $git -C $dir init -q .; & $git -C $dir config user.email t@t; & $git -C $dir config user.name t; & $git -C $dir config core.autocrlf false
    [IO.File]::WriteAllText("$dir\tracked.txt", "tracked`n")
    & $git -C $dir add -A; & $git -C $dir commit -q -m init
    $dir
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

# One request to the onboarding page: the status and the body, also when the status is an error.
function Send-Onboarding([string]$url, [string]$body) {
    $req = [Net.WebRequest]::Create($url); $req.Proxy = $null
    if ($body) { $req.Method = 'POST'; $req.ContentType = 'application/json'; $bytes = [Text.Encoding]::UTF8.GetBytes($body); $s = $req.GetRequestStream(); $s.Write($bytes, 0, $bytes.Length); $s.Close() }
    try { $res = $req.GetResponse() }
    catch { $e = $_.Exception; while ($e -and $e -isnot [Net.WebException]) { $e = $e.InnerException }; if (-not $e -or -not $e.Response) { throw }; $res = $e.Response }
    $text = (New-Object IO.StreamReader $res.GetResponseStream(), ([Text.Encoding]::UTF8)).ReadToEnd()
    $res.Close()
    [pscustomobject]@{ Status = [int]$res.StatusCode; Text = $text }
}

# Opens a session through strategist.cmd and leaves it in $proc, $sess and $launcherOut.
# The onboarding page is fetched ($page: its text; $pageData: the data it carries) and answered
# with each of $post in turn, the replies kept in $posted; with no $post, what the page shows is
# sent back unchanged, as a click on its button would.
function Start-Session([string]$from = $tool, [int]$seconds = 60, [string]$cwd = $repo, $post = @()) {
    Remove-Item "$root\session.txt", "$root\stop", "$root\calls.txt", "$root\args.txt", "$root\claude-args.txt" -ErrorAction SilentlyContinue
    $script:launcherOut = "$root\launcher-" + [Guid]::NewGuid().ToString('N').Substring(0, 8) + '.txt'   # what the launcher prints, warnings included
    $script:proc = Start-Process cmd.exe -ArgumentList '/c', "`"$from\strategist.cmd`"" -WorkingDirectory $cwd -WindowStyle Hidden -PassThru -RedirectStandardOutput $launcherOut -RedirectStandardError "$launcherOut.err"
    $null = $proc.Handle   # without this, a redirected Start-Process loses the exit code in 5.1
    $url = $null
    for ($i = 0; $i -lt ($seconds * 5) -and -not $url -and -not $proc.HasExited; $i++) {
        Start-Sleep -Milliseconds 200
        if ("$(Get-Content -LiteralPath $launcherOut -Raw -ErrorAction SilentlyContinue)" -match '(?m)^Onboarding: (\S+)') { $url = $Matches[1] }
    }
    if (-not $url) { throw 'a pagina de onboarding nao abriu' }
    $script:page = (Send-Onboarding $url).Text
    $script:pageData = [regex]::Match($page, '(?s)<script id="data"[^>]*>(.*?)</script>').Groups[1].Value | ConvertFrom-Json
    if (-not @($post).Count) { $post = @($pageData.config) }
    $script:posted = @($post | ForEach-Object { Send-Onboarding $url ($_ | ConvertTo-Json -Depth 4) })
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
    $out = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$sess\delegate.ps1" $task 2>&1 | ForEach-Object { "$_" }
    [pscustomobject]@{ Exit = $LASTEXITCODE; Text = ($out -join "`n") }
}

$script:failures = 0
function Check([string]$id, [string]$name, [bool]$ok, [string]$detail = '') {
    if (-not $ok) { $script:failures++ }
    "{0} {1,-4} {2}{3}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $id, $name, $(if ($detail) { "  [$detail]" })
}

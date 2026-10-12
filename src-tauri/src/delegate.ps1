# Launcher de delegacao de uma sessao do app Orquestrador. O unico comando que o orquestrador roda:
#   delegate.ps1 'tarefa A' 'tarefa B'    um lote; fica esperando ate todas voltarem
#   delegate.ps1 '--aguardar'             volta a esperar o lote mais recente ainda nao entregue
# Somente ASCII: o Windows PowerShell 5.1 le um .ps1 sem BOM como ANSI.
# O pedido vai para o app como uma pasta em requests\ (um arquivo por tarefa, publicada por
# renomeacao); o resultado volta em results\<lote>.txt (primeira linha: codigo de saida).
$ErrorActionPreference = 'Stop'
$utf8 = New-Object Text.UTF8Encoding $false
# Bytes UTF-8 direto no stdout, como um programa nativo: nao depende da pagina de codigo do
# console nem do $OutputEncoding de quem chamou.
$stdout = [Console]::OpenStandardOutput()
function Write-Raw([string]$text) { $b = $utf8.GetBytes($text); $stdout.Write($b, 0, $b.Length); $stdout.Flush() }
$code = 4
try {
    $session = $PSScriptRoot
    if ($args.Count -eq 0) { throw "Nenhuma tarefa. Passe cada tarefa como um argumento entre aspas simples: delegate.ps1 'tarefa A' 'tarefa B'." }
    if ($args.Count -eq 1 -and "$($args[0])" -eq '--aguardar') {
        $pending = @(Get-ChildItem -LiteralPath "$session\requests" -Directory |
                Where-Object { $_.Name -notlike '*.tmp' -and -not (Test-Path -LiteralPath "$session\results\$($_.Name).delivered") } | Sort-Object Name)
        if (-not $pending) { throw 'Nenhum lote aguardando entrega.' }
        $id = $pending[-1].Name
    }
    else {
        foreach ($task in $args) { if ([string]::IsNullOrWhiteSpace("$task")) { throw 'Tarefa vazia.' } }
        $id = [DateTime]::UtcNow.ToString('yyyyMMddHHmmssfff') + '-' + [Guid]::NewGuid().ToString('N').Substring(0, 8)
        $tmp = "$session\requests\$id.tmp"
        [IO.Directory]::CreateDirectory($tmp) | Out-Null
        for ($i = 0; $i -lt $args.Count; $i++) { [IO.File]::WriteAllText("$tmp\$($i + 1).txt", "$($args[$i])", $utf8) }
        [IO.Directory]::Move($tmp, "$session\requests\$id")
    }
    $result = "$session\results\$id.txt"
    $owner = [int](([IO.File]::ReadAllText("$session\owner") -split ' ')[0])
    while (-not [IO.File]::Exists($result)) {
        if (-not [IO.File]::Exists("$session\owner") -or -not (Get-Process -Id $owner -ErrorAction SilentlyContinue)) { throw 'A sessao do orquestrador terminou antes do fim do lote.' }
        Start-Sleep -Milliseconds 250
    }
    $text = [IO.File]::ReadAllText($result, $utf8)
    $cut = $text.IndexOf("`n")
    $exit = [int]$text.Substring(0, $cut)
    Write-Raw $text.Substring($cut + 1)
    [IO.File]::WriteAllText("$session\results\$id.delivered", '')
    $code = $exit
}
catch { Write-Raw "wrapper_error: $($_.Exception.Message)`n--- FIM ---`n" }
# "pwsh -Command script" transforma qualquer codigo de falha em 1; Exit entrega o codigo certo.
[Environment]::Exit($code)

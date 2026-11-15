# Ponta a ponta local, sem tocar na instalação real: chave temporária, produto `OrquestradorTeste`, exe `orquestrador-teste`.
# 1. O updater do 0.1.48 (compat-tauri, tauri-plugin-updater 2.14.1) instala a 0.2.0-teste.1 pelo instalador novo.
# 2. A .1 abre sozinha (/R), acha a .2 no latest.json local, baixa, valida, instala e reabre.
# 3. Confere registro e exe, desinstala e confere a limpeza. Exige o FFmpeg estático, o cargo-packager e Python (servidor HTTP).
param(
    [string]$Raiz = (Join-Path $env:TEMP 'orq-e2e'),
    [int]$Porta = 8765,
    [switch]$SemBuild
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$aqui = $PSScriptRoot
$raizRepo = (Resolve-Path (Join-Path $aqui '..\..\..')).Path
$alvo = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $raizRepo 'target' }
$produto = 'OrquestradorTeste'
$binario = 'orquestrador-teste'
$chaveReg = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$produto"
$base = "http://127.0.0.1:$Porta"
$versoes = '0.2.0-teste.1', '0.2.0-teste.2'
if (Test-Path $chaveReg) { throw "$produto já instalado; desinstale antes." }

New-Item -ItemType Directory -Force $Raiz, "$Raiz\srv", "$Raiz\local" | Out-Null
cargo build --release --locked --manifest-path (Join-Path $aqui 'assinador\Cargo.toml')
cargo build --release --locked --manifest-path (Join-Path $aqui 'compat-tauri\Cargo.toml')
$assinador = Join-Path $alvo 'release\orquestrador-assinador.exe'
$compat = Join-Path $alvo 'release\orquestrador-compat-tauri.exe'
$chave = Join-Path $Raiz 'teste.key'
if (-not (Test-Path $chave)) { & $assinador chaves $chave | Out-Null }
$pub = (Get-Content -Raw (Join-Path $Raiz 'teste.pub')).Trim()

if (-not $SemBuild) {
    $env:ORQ_ATUALIZACAO_URL = "$base/latest.json"
    $env:ORQ_ATUALIZACAO_CHAVE = $pub
    foreach ($v in $versoes) {
        $nome = "${produto}_${v}_x64-setup.exe"
        & pwsh (Join-Path $aqui 'empacotar.ps1') -Versao $v -Chave $chave -Produto $produto -Binario $binario `
            -Url "$base/$v/$nome" -Saida (Join-Path $Raiz $v)
    }
    Remove-Item env:ORQ_ATUALIZACAO_URL, env:ORQ_ATUALIZACAO_CHAVE
}
foreach ($v in $versoes) {
    New-Item -ItemType Directory -Force "$Raiz\srv\$v" | Out-Null
    Copy-Item "$Raiz\$v\*-setup.exe" "$Raiz\srv\$v\"
    & $assinador verificar $pub $v "$Raiz\$v\latest.json" (Get-Item "$Raiz\$v\*-setup.exe").FullName
    & $compat verificar $pub "$Raiz\$v\latest.json" (Get-Item "$Raiz\$v\*-setup.exe").FullName
}
# O 0.1.48 consulta tauri.json (a .1); o app novo consulta latest.json (a .2).
Copy-Item "$Raiz\$($versoes[0])\latest.json" "$Raiz\srv\tauri.json"
Copy-Item "$Raiz\$($versoes[1])\latest.json" "$Raiz\srv\latest.json"

$servidor = Start-Process python -ArgumentList '-m', 'http.server', $Porta, '--bind', '127.0.0.1', '--directory', "$Raiz\srv" -PassThru -WindowStyle Hidden
$antigoLocal = $env:LOCALAPPDATA
function versao { (Get-ItemProperty $chaveReg -ErrorAction SilentlyContinue).DisplayVersion }
function esperar([string]$v, [int]$s) {
    $fim = (Get-Date).AddSeconds($s)
    while ((Get-Date) -lt $fim) { if ((versao) -eq $v) { return } ; Start-Sleep -Milliseconds 500 }
    throw "Tempo esgotado esperando $v (registro: $(versao))"
}
try {
    Start-Sleep 1
    # Dados do app (config, interface, migração) numa pasta isolada; o NSIS usa a pasta real só para o padrão, trocado por /D.
    $env:LOCALAPPDATA = "$Raiz\local"
    $destino = "$Raiz\instalado"
    & $compat instalar $pub "$base/tauri.json" "/D=$destino"
    esperar $versoes[0] 120
    Write-Host "0.1.48 (Tauri) → $($versoes[0]): ok"
    esperar $versoes[1] 300
    Write-Host "$($versoes[0]) → $($versoes[1]) pelo app: ok"
    # A .2 reabre (/R); espera o processo e encerra antes de ele ir longe na etapa dos CLIs.
    $fim = (Get-Date).AddSeconds(60)
    while (-not (Get-Process $binario -ErrorAction SilentlyContinue) -and (Get-Date) -lt $fim) { Start-Sleep -Milliseconds 300 }
    $reaberto = [bool](Get-Process $binario -ErrorAction SilentlyContinue)
    Get-Process $binario -ErrorAction SilentlyContinue | Stop-Process -Force
    if (-not $reaberto) { throw "A $($versoes[1]) não reabriu" }
    Write-Host 'reabriu sozinho: ok'
    $instalado = (Get-FileHash "$destino\$binario.exe").Hash
    $esperado = (Get-FileHash "$Raiz\$($versoes[1])\palco\$binario.exe").Hash
    if ($instalado -ne $esperado) { throw 'O exe instalado não é o da .2' }
    $r = Get-ItemProperty $chaveReg
    if ($r.Publisher -ne 'luansilvadb' -or $r.UninstallString -ne "`"$destino\uninstall.exe`"") { throw "Registro inesperado: $($r | Out-String)" }
    if (-not (Test-Path "HKCU:\Software\Classes\$produto.File\shell\open\command")) { throw 'Hook de pós-instalação não rodou' }
    Write-Host 'exe, registro e hooks: ok'
} finally {
    $env:LOCALAPPDATA = $antigoLocal
    Get-Process $binario -ErrorAction SilentlyContinue | Stop-Process -Force
    if (Test-Path "$Raiz\instalado\uninstall.exe") {
        Start-Process "$Raiz\instalado\uninstall.exe" -ArgumentList '/S' -Wait
        $fim = (Get-Date).AddSeconds(30)
        while ((Test-Path $chaveReg) -and (Get-Date) -lt $fim) { Start-Sleep -Milliseconds 300 }
    }
    Stop-Process -Id $servidor.Id -Force -ErrorAction SilentlyContinue
}
if (Test-Path $chaveReg) { throw 'A desinstalação deixou a chave de registro' }
if (Test-Path "HKCU:\Software\Classes\$produto.File") { throw 'A desinstalação deixou as associações' }
Write-Host 'desinstalação limpa: ok'

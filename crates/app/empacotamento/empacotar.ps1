# Gera o instalador NSIS por usuário (cargo-packager), o .sig e o latest.json no formato do updater do Tauri 2.
# Uso: pwsh crates/app/empacotamento/empacotar.ps1 -Versao 0.2.0 -Chave <caminho ou conteúdo da chave privada>
# Sem -Url, o instalador aponta para a release vX.Y.Z do GitHub. -Produto/-Binario diferentes só para teste.
param(
    [Parameter(Mandatory)][string]$Versao,
    [string]$Chave = $env:ORQ_CHAVE_ASSINATURA,
    [string]$Url,
    [string]$Notas = '-',
    [string]$Produto = 'Orquestrador',
    [string]$Identificador = 'dev.luansilvadb.orquestrador',
    # Nome do exe instalado; o 0.1.48 instalou `orquestrador.exe`.
    [string]$Binario = 'orquestrador',
    # Bin do crate orquestrador-app a empacotar.
    [string]$Origem = 'orquestrador',
    [string]$Saida,
    # Só monta o pacote com um exe já compilado.
    [switch]$SemBuild
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$aqui = $PSScriptRoot
$raiz = (Resolve-Path (Join-Path $aqui '..\..\..')).Path
if (-not $Chave) { throw 'Falta a chave privada (-Chave ou ORQ_CHAVE_ASSINATURA).' }
$alvo = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $raiz 'target' }
if (-not $Saida) { $Saida = Join-Path $alvo 'pacote' }
$palco = Join-Path $Saida 'palco'
Remove-Item -Recurse -Force $Saida -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $palco | Out-Null

if (-not $SemBuild) {
    $env:ORQ_VERSAO = $Versao
    cargo build --release --locked -p orquestrador-app --bin $Origem
    Remove-Item env:ORQ_VERSAO
}
Copy-Item (Join-Path $alvo "release\$Origem.exe") (Join-Path $palco "$Binario.exe")

# DLLs do FFmpeg compartilhado (LGPL) ao lado do exe e a licença, exigida pela LGPL. O avdevice não é usado.
$ffmpeg = Join-Path $raiz 'ferramentas\ffmpeg'
$recursos = @()
foreach ($dll in Get-ChildItem (Join-Path $ffmpeg 'bin') -Filter '*.dll' | Where-Object Name -NotLike 'avdevice-*') {
    Copy-Item $dll.FullName $palco
    $recursos += @{ src = $dll.Name; target = $dll.Name }
}
if ($recursos.Count -lt 6) { throw "DLLs do FFmpeg ausentes em $ffmpeg (crates/midia/preparar-ffmpeg.ps1)" }
Copy-Item (Join-Path $ffmpeg 'LICENSE.txt') (Join-Path $palco 'FFmpeg-LICENSE.txt')
$recursos += @{ src = 'FFmpeg-LICENSE.txt'; target = 'FFmpeg-LICENSE.txt' }

# O modelo inclui os hooks por caminho absoluto.
$hooks = Join-Path $aqui 'hooks.nsh'
$modelo = Join-Path $palco 'installer.nsi'
(Get-Content -Raw (Join-Path $aqui 'installer.nsi')).Replace('@@HOOKS@@', $hooks) | Set-Content -NoNewline -Encoding utf8 $modelo

$icone = Join-Path $aqui 'icon.ico'
$config = @{
    name = 'orquestrador'
    productName = $Produto
    version = $Versao
    identifier = $Identificador
    publisher = 'luansilvadb'
    formats = @('nsis')
    outDir = $Saida
    binariesDir = $palco
    binaries = @(@{ path = $Binario; main = $true })
    icons = @($icone)
    resources = $recursos
    nsis = @{ installMode = 'currentUser'; template = $modelo; installerIcon = $icone }
} | ConvertTo-Json -Depth 5
$arquivoConfig = Join-Path $palco 'Packager.json'
Set-Content -Encoding utf8 $arquivoConfig $config

$packager = (Get-Command cargo-packager -ErrorAction SilentlyContinue).Source
if (-not $packager) { throw 'cargo-packager ausente: cargo install cargo-packager --version 0.11.8 --locked' }
Push-Location $palco
try { & $packager --config $arquivoConfig -f nsis } finally { Pop-Location }

$exe = Get-ChildItem $Saida -Filter '*-setup.exe' | Select-Object -First 1
if (-not $exe) { throw "cargo-packager não gerou o instalador em $Saida" }
# Mesmo nome do tauri-bundler, que o fluxo de release e as URLs já usam.
$nome = "${Produto}_${Versao}_x64-setup.exe"
if ($exe.Name -cne $nome) { $exe = Rename-Item $exe.FullName $nome -PassThru }
if (-not $Url) { $Url = "https://github.com/luansilvadb/orquestrador/releases/download/v$Versao/$nome" }

$assinador = Join-Path $alvo 'release\orquestrador-assinador.exe'
cargo build --release --locked --manifest-path (Join-Path $aqui 'assinador\Cargo.toml')
& $assinador assinar $Chave $exe.FullName $Versao
& $assinador manifesto $Versao $Url "$($exe.FullName).sig" $Notas (Join-Path $Saida 'latest.json')
Write-Host "Pronto: $($exe.FullName), .sig e latest.json em $Saida"

# Baixa o FFmpeg compartilhado (só LGPL, pré-compilado) e o libclang que o `ffmpeg-sys-next` precisa em `<raiz>\ferramentas`
# (ignorada pelo git); o `.cargo/config.toml` aponta as variáveis para lá. Idempotente: rodar de novo só confere.
# Uso: pwsh crates/midia/preparar-ffmpeg.ps1
param(
    # Build mensal do BtbN/FFmpeg-Builds (as do fim do mês ficam guardadas) com FFmpeg 9.0, import libs .lib do MSVC e DLLs.
    [string]$Url = 'https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-10-31-12-59/ffmpeg-n9.0.2-39-g7adfbe40dc-win64-lgpl-shared-9.0.zip',
    [string]$Sha256 = 'e907b01fa4783913151237e37df2a10fa97d99a1598a021bf59d4ae62ffed4c0',
    [string]$LibclangVersao = '18.1.1'
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Ferramentas = Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..')) 'ferramentas'
$destino = Join-Path $Ferramentas 'ffmpeg'
$marca = Join-Path $destino '.sha256'
New-Item -ItemType Directory -Force $Ferramentas | Out-Null

if (-not ((Test-Path $marca) -and (Get-Content $marca -Raw).Trim() -eq $Sha256)) {
    $zip = Join-Path $Ferramentas 'ffmpeg.zip'
    if (-not ((Test-Path $zip) -and (Get-FileHash $zip -Algorithm SHA256).Hash -eq $Sha256)) {
        Invoke-WebRequest $Url -OutFile $zip
    }
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash
    if ($hash -ne $Sha256) { throw "SHA-256 do FFmpeg não confere: $hash" }
    $temp = Join-Path $Ferramentas 'ffmpeg-extraindo'
    Remove-Item -Recurse -Force $temp, $destino -ErrorAction SilentlyContinue
    Expand-Archive $zip $temp
    Move-Item (Get-ChildItem $temp -Directory | Select-Object -First 1).FullName $destino
    Remove-Item -Recurse -Force $temp

    # Confere a licença: sem GPL nem nonfree.
    $conf = & (Join-Path $destino 'bin\ffmpeg.exe') -hide_banner -buildconf | Out-String
    if ($LASTEXITCODE -ne 0) { throw 'ffmpeg -buildconf falhou' }
    if ($conf -match '--enable-(gpl|nonfree)') { throw 'FFmpeg baixado não é só LGPL' }
    if ((Get-Content (Join-Path $destino 'LICENSE.txt') -Raw) -notmatch 'LESSER GENERAL PUBLIC') { throw 'LICENSE.txt sem a LGPL' }
    foreach ($lib in 'avcodec', 'avformat', 'avfilter', 'avutil', 'swscale', 'swresample') {
        if (-not (Test-Path (Join-Path $destino "lib\$lib.lib"))) { throw "Falta lib\$lib.lib" }
    }
    Set-Content $marca $Sha256
}

$libclang = Join-Path $Ferramentas 'libclang'
if (-not (Test-Path (Join-Path $libclang 'clang\native\libclang.dll'))) {
    python -m pip install "libclang==$LibclangVersao" --target $libclang --quiet
    if ($LASTEXITCODE -ne 0) { throw "pip falhou ($LASTEXITCODE)" }
}

'Pronto: cargo test -p orquestrador-midia'

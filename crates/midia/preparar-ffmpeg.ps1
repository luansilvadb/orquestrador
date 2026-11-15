# Prepara o FFmpeg estático (só LGPL) e o libclang que o `ffmpeg-sys-next` precisa, fora do repositório, e grava as
# variáveis de ambiente do usuário. Idempotente: rodar de novo só confere. Uso: pwsh crates/midia/preparar-ffmpeg.ps1 [-Ferramentas D:\ferramentas]
param(
    [string]$Ferramentas = 'D:\ferramentas',
    # Commit do vcpkg com o port ffmpeg 9.0.2#7; fixar o commit torna o build reproduzível.
    [string]$VcpkgCommit = '8f363bdc11359ede4bc51e5eb32a2617538049c8',
    [string]$LibclangVersao = '18.1.1'
)
$ErrorActionPreference = 'Stop'
$triplet = 'x64-windows-static-md'
# Sem `gpl`, `version3` nem `nonfree`: o port só liga GPL com a feature `gpl`. Decodificadores e demuxers nativos vêm todos.
$pacote = "ffmpeg[core,avcodec,avformat,avfilter,swscale,swresample]:$triplet"

New-Item -ItemType Directory -Force $Ferramentas | Out-Null
$vcpkg = Join-Path $Ferramentas 'vcpkg'
if (-not (Test-Path (Join-Path $vcpkg '.git'))) {
    git clone https://github.com/microsoft/vcpkg.git $vcpkg
}
git -C $vcpkg fetch --depth 1 origin $VcpkgCommit
git -C $vcpkg checkout --quiet $VcpkgCommit
if (-not (Test-Path (Join-Path $vcpkg 'vcpkg.exe'))) {
    & (Join-Path $vcpkg 'bootstrap-vcpkg.bat') -disableMetrics
}
& (Join-Path $vcpkg 'vcpkg.exe') install $pacote --clean-after-build
if ($LASTEXITCODE -ne 0) { throw "vcpkg install falhou ($LASTEXITCODE)" }

# Confere que a build não ligou GPL nem nonfree.
$licenca = Get-Content (Join-Path $vcpkg "installed\$triplet\share\ffmpeg\copyright") -Raw
if ($licenca -match 'GNU GENERAL PUBLIC LICENSE' -and $licenca -notmatch 'LESSER') { throw 'FFmpeg instalado com licença GPL' }

$libclang = Join-Path $Ferramentas 'libclang'
if (-not (Test-Path (Join-Path $libclang 'clang\native\libclang.dll'))) {
    python -m pip install "libclang==$LibclangVersao" --target $libclang --quiet
}

$variaveis = @{
    VCPKG_ROOT      = $vcpkg
    VCPKGRS_TRIPLET = $triplet
    LIBCLANG_PATH   = (Join-Path $libclang 'clang\native')
}
foreach ($v in $variaveis.GetEnumerator()) {
    [Environment]::SetEnvironmentVariable($v.Key, $v.Value, 'User')
    Set-Item "env:$($v.Key)" $v.Value
    "$($v.Key)=$($v.Value)"
}
'Pronto. Abra um terminal novo (ou use este) e rode: cargo test -p orquestrador-midia'

# Script de exemplo
param(
    [string]$Nome = "mundo",
    [int]$Limite = 10
)

$ErrorActionPreference = 'Stop'

function Get-Saudacao {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Quem)
    return "Ola, $Quem! $(Get-Date -Format yyyy)"
}

foreach ($i in 1..$Limite) {
    if ($i -gt 5 -and (Test-Path "C:\temp")) {
        Get-Saudacao -Quem $Nome | Out-Null
    }
}

$lista = @(1, 2, 3)
$mapa = @{ chave = $true; outra = $null }
Write-Host $lista.Count $env:PATH

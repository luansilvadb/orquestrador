# Desenvolvimento

## Preparar a máquina

1. Visual Studio Build Tools com a carga "Desenvolvimento para desktop com C++" (MSVC e Windows SDK).
2. Rust pelo [rustup](https://rustup.rs), toolchain `stable-x86_64-pc-windows-msvc`, com `clippy`.
3. Git.

## Testes com CLIs reais

Ficam ignorados porque chamam o Claude Code e o Codex instalados.

- Ciclo de lote, num repositório Git descartável: defina `ORQ_REAL_REPO` e `ORQ_REAL=codex,claude` (orquestrador,executor) e rode
  `cargo test -p orquestrador-app --test real -- --ignored --nocapture`.
- Colagem de imagem: ponha uma imagem no clipboard, defina `ORQ_PASTE=claude` ou `codex` e rode
  `cargo test -p orquestrador-core --test colagem -- --ignored --nocapture --test-threads=1`.

## Empacotar

```powershell
cargo install cargo-packager --version 0.11.8 --locked
pwsh crates/app/empacotamento/empacotar.ps1 -Versao X.Y.Z -Chave <caminho ou conteúdo da chave privada>
```

`pwsh crates/app/empacotamento/e2e.ps1` testa a atualização ponta a ponta sem tocar na instalação real.

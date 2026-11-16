# Desenvolvimento

## Preparar a máquina

1. Visual Studio Build Tools com a carga "Desenvolvimento para desktop com C++" (MSVC e Windows SDK).
2. Rust pelo [rustup](https://rustup.rs), toolchain `stable-x86_64-pc-windows-msvc`, com `clippy`.
3. Git.
4. FFmpeg compartilhado (LGPL) e libclang, usados pelo `orquestrador-midia`:

   ```powershell
   pwsh crates/midia/preparar-ffmpeg.ps1
   ```

   O script baixa o FFmpeg LGPL pré-compilado (URL e SHA-256 fixos) e o libclang, tudo em `ferramentas/`, ignorada pelo git.
   O `[env]` do `.cargo/config.toml` define `FFMPEG_DIR` e `LIBCLANG_PATH`; o `build.rs` do `orquestrador-midia` copia as DLLs
   para `target/`. Rodar de novo só confere.

## Rodar e testar

```powershell
cargo run -p orquestrador-app                               # abre o app (orquestrador.exe), sem atualização nem instância única
cargo test --workspace                                      # testes; os CLIs são simulados por crates/app/examples/fake_cli.rs
cargo clippy --workspace --all-targets -- -D warnings       # lint
cargo build --workspace --release
```

No build de debug, o portão não instala nem atualiza os CLIs: só localiza `claude.exe` e `codex.exe` (`ORQ_CLAUDE_EXE` e
`ORQ_CODEX_EXE` à frente; detalhes em [atualização](atualizacao.md)). A instância única vale só no release, então o build de
desenvolvimento coexiste com o app instalado.

Testes que chamam CLIs reais ficam ignorados:

- `crates/app/tests/lote.rs` fala com o servidor MCP real (`orquestrador.exe --mcp`) por JSON-RPC, com CLIs falsos.
- `crates/app/tests/real.rs` faz o ciclo com os CLIs reais: defina `ORQ_REAL_REPO` e `ORQ_REAL=codex,claude` e rode
  `cargo test -p orquestrador-app --test real -- --ignored --nocapture`.
- `crates/core/tests/colagem.rs` sobe o Claude Code ou o Codex reais (`ORQ_PASTE=claude` ou `codex`):
  `cargo test -p orquestrador-core --test colagem -- --ignored --nocapture --test-threads=1`. `colar_imagem` lê a imagem do
  clipboard.

Os crates de tela têm vitrines (`cargo run -p <crate> --example ...`, ver o README de cada um).

## Tarefas

`cargo xtask ajuda` lista as tarefas:

- `release <subcomando>`: regras do fluxo de release ([atualização e release](atualizacao.md)).
- `limpar-target`: `cargo clean` quando `target` passa de 30 GB (`ORQUESTRADOR_TARGET_MAX_GB`).
- `medir`: medição de desempenho ([desempenho do GPUI](desempenho-gpui.md)).
- `despejar`: gerador de saída para as medições.

## Empacotar

```powershell
cargo install cargo-packager --version 0.11.8 --locked
pwsh crates/app/empacotamento/empacotar.ps1 -Versao X.Y.Z -Chave <caminho ou conteúdo da chave privada>
```

Saída em `target\pacote\` (ver [atualização](atualizacao.md#empacotamento)). `pwsh crates/app/empacotamento/e2e.ps1`
testa ponta a ponta, sem tocar na instalação real, a atualização a partir do updater da 0.1.48.

## CI

`.github/workflows/ci.yml` roda a cada push em `main` e em pull request: no Windows, `cargo test --workspace` e o clippy; em
Ubuntu e macOS, só `cargo check -p orquestrador-core --all-targets`. O workflow de release roda `cargo test --workspace` antes
de publicar.

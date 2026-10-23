# Desenvolvimento

## Pré-requisitos

- Windows com WebView2.
- Node.js.
- Rust com a toolchain MSVC (exige o Microsoft C++ Build Tools).
- Git (recomendado).

## Rodar e compilar

```powershell
npm install
npm run tauri dev      # desenvolvimento: uso liberado direto, sem consultar atualizações
npm run build          # tsc && vite build: checa os tipos e gera o front-end
npm run tauri build    # gera o instalador assinado em src-tauri\target\release\bundle\nsis (exige a chave, ver atualizacao.md)
```

`npm run dev` sobe só o front-end (Vite), com um backend simulado em `src/backend/mock.ts`, fora do Tauri. `?project=none` abre sem projeto mesmo se houver um último salvo; `?projects=2` abre dois projetos simulados. Os modos sem Git ficam nas URLs `?git=no-repo`, `no-git`, `repo-above` e `?subfolder=1`.

Para acrescentar um comando: escreva a função `#[tauri::command]` em `lib.rs`, registre-a no `generate_handler!`, adicione a entrada em `Commands` (`types.ts`), o método em `backend.ts` e o handler no mock; `npm run check` aponta a divergência entre o Rust e `Commands`, e o `tsc` aponta o handler que falta no mock.

## Testes

```powershell
npm run check       # 9 checagens puras (Node, sem dependências)
npm run check:ui    # 11 checks de interface, 4 em paralelo (Chrome headless)
```

`npm run check` roda os arquivos listados em `package.json`; `npm run check:ui` sobe um Vite só na porta 1421 (`SMOKE_PORT`) pelo setup global (`tests/ui-setup.mjs`) e roda 4 arquivos de cada vez, cada um com o próprio Chromium. Um arquivo rodado sozinho (`node --test tests/check-git-ui.mjs`) sobe o próprio Vite se a porta estiver livre. Os dois saem com código diferente de 0 se algum teste falhou, e mostram o resultado de todos os arquivos.

O plugin de instância única roda só em builds release. Builds debug usam o mesmo identificador do app instalado, mas não encaminham caminhos nem ativam outra instância; assim `npm run tauri dev` pode coexistir com o app instalado.

Os checks de interface são em Chrome headless, contra o app simulado.

Capturas e logs vão para `.playwright-mcp/`, que não vai para o Git.

O workflow `.github/workflows/ci.yml` roda a cada push em `main` e em pull request. No Windows, faz o build do front-end, `npm run check` e `cargo test`. Em Ubuntu e macOS, só compila o backend (`cargo check --all-targets`), sem testes, porque os recursos específicos do sistema ainda não foram portados. Os checks de interface (`npm run check:ui`) ficam de fora do CI: sobem o Vite e um Chromium local.

Os testes do backend:

```powershell
cd src-tauri
cargo test
```

Os testes usam CLIs simulados e não chamam nenhum modelo. `tests\lote.rs` fala com o servidor MCP real (`orquestrador.exe --mcp`) por JSON-RPC, sem PowerShell no caminho. `tests\real.rs` faz o ciclo com os CLIs reais e fica ignorado por padrão: defina `ORQ_REAL_REPO` e `ORQ_REAL=codex,claude` e rode `cargo test --test real -- --ignored --nocapture`. `tests\colagem.rs` sobe o Claude Code ou o Codex reais (`ORQ_PASTE=claude` ou `codex`) e mede a colagem sem enviar mensagem: `colar_caminho` (caminhos de imagem e de outro arquivo viram anexo e texto) e `cli_real` (o shell reconhece o CLI aberto nele e o esquece quando ele sai) não mexem no clipboard; `colar_imagem` lê a imagem que estiver no clipboard. Rode com `cargo test --test colagem -- --ignored --nocapture --test-threads=1`.

O workflow de release (`.github/workflows/release.yml`) roda antes de publicar: `npm run build`, `node tests/check-file-tabs.mjs`, `node tests/check-release.mjs` e `cargo test --manifest-path src-tauri/Cargo.toml`. As regras do fluxo de release estão em `scripts/release.mjs`; o processo de publicação, em [atualização e release](atualizacao.md).

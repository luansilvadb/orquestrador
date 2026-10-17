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

`npm run dev` sobe só o front-end (Vite), com um backend simulado em `src/mock.ts`, fora do Tauri. Os modos sem Git ficam nas URLs `?git=no-repo`, `no-git`, `repo-above` e `?subfolder=1`.

## Checagens sem dependências

Rodam com Node, sem dependências:

```powershell
node scripts/check-workspace.mjs
node scripts/check-editor-store.mjs
node scripts/check-file-tabs.mjs
node scripts/check-explorer.mjs
node scripts/check-git-graph.mjs
node scripts/check-paste.mjs
node scripts/check-release.mjs
```

## Testes

Os checks de interface são em Chrome headless, contra o app simulado, e saem com 0 se todos os cenários passam. Todos sobem o Vite na mesma porta (1421, `SMOKE_PORT`), então rode um de cada vez:

```powershell
node scripts/check-editor-ui.mjs
node scripts/check-files-search-ui.mjs
node scripts/check-git-ui.mjs
node scripts/check-media-ui.mjs
```

Capturas e logs vão para `.playwright-mcp/`, que não vai para o Git.

Os testes do backend:

```powershell
cd src-tauri
cargo test
```

Os testes usam CLIs simulados e não chamam nenhum modelo. `tests\lote.rs` fala com o servidor MCP real (`orquestrador.exe --mcp`) por JSON-RPC, sem PowerShell no caminho. `tests\real.rs` faz o ciclo com os CLIs reais e fica ignorado por padrão: defina `ORQ_REAL_REPO` e `ORQ_REAL=codex,claude` e rode `cargo test --test real -- --ignored --nocapture`. `tests\colagem.rs` sobe o Claude Code ou o Codex reais (`ORQ_PASTE=claude` ou `codex`) e mede a colagem sem enviar mensagem: `colar_caminho` (caminhos de imagem e de outro arquivo viram anexo e texto) e `cli_real` (o shell reconhece o CLI aberto nele e o esquece quando ele sai) não mexem no clipboard; `colar_imagem` lê a imagem que estiver no clipboard. Rode com `cargo test --test colagem -- --ignored --nocapture --test-threads=1`.

O workflow de release (`.github/workflows/release.yml`) roda antes de publicar: `npm run build`, `node scripts/check-file-tabs.mjs`, `node scripts/check-release.mjs` e `cargo test --manifest-path src-tauri/Cargo.toml`. As regras do fluxo de release estão em `scripts/release.mjs`; o processo de publicação, em [atualização e release](atualizacao.md).

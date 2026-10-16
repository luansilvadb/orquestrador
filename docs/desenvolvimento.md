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

As Issues simuladas ficam em memória, com o repositório `luansilvadb/orquestrador` pronto e os comandos `github_*` demorando cerca de 300 ms. Os outros modos ficam em `?gh=no-gh`, `no-login`, `no-remote`, `issues-disabled` e `offline`, em que `github_status` volta indisponível com a mensagem do backend, e `?gh=create-fails`, em que tudo funciona menos a criação, que falha com “Sem conexão com o GitHub.”. No console, `__mock.ghMode = 'offline'` (ou outro modo) troca o modo em execução. No orquestrador simulado, um Enter sozinho (não uma colagem) começa uma rodada, que termina sozinha em 8 s, com o evento `orchestrator-turn` nas duas pontas; `__mock.orchestratorTurn(true)` (ou `false`) força o estado na hora, sem o fim automático.

## Checagens sem dependências

Rodam com Node, sem dependências:

```powershell
node scripts/check-workspace.mjs
node scripts/check-editor-store.mjs
node scripts/check-file-tabs.mjs
node scripts/check-explorer.mjs
node scripts/check-git-graph.mjs
node scripts/check-issues.mjs
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

Os testes usam CLIs simulados e não chamam nenhum modelo. `tests\lote.rs` fala com o servidor MCP real (`orquestrador.exe --mcp`) por JSON-RPC, sem PowerShell no caminho. `tests\real.rs` faz o ciclo com os CLIs reais e fica ignorado por padrão: defina `ORQ_REAL_REPO` e `ORQ_REAL=codex,claude` e rode `cargo test --test real -- --ignored --nocapture`.

O workflow de release (`.github/workflows/release.yml`) roda antes de publicar: `npm run build`, `node scripts/check-file-tabs.mjs`, `node scripts/check-issues.mjs`, `node scripts/check-release.mjs` e `cargo test --manifest-path src-tauri/Cargo.toml`. As regras do fluxo de release estão em `scripts/release.mjs`; o processo de publicação, em [atualização e release](atualizacao.md).

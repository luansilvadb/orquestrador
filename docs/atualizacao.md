# Atualização e release

## Atualização automática

Em toda abertura do app instalado (build de release), antes de liberar a escolha da pasta ou qualquer terminal, ele consulta a última release **estável** de `luansilvadb/orquestrador` no GitHub (`releases/latest/download/latest.json`). Rascunhos e pré-releases não chegam ao app. Se houver versão superior, o app mostra o andamento, baixa, valida a assinatura, instala em modo silencioso (NSIS por usuário, sem administrador) e reabre sozinho na versão nova. Se já estiver em dia, passa à etapa dos CLIs.

Isso fica em `crates/app/src/atualizacao/` (consulta com `ureq`, assinatura com `minisign-verify`, o mesmo verificador do `tauri-plugin-updater` da 0.1.48) e a tela em `crates/app/src/terminal/portao.rs`: `Verificando se há uma versão nova…`, `Baixando a versão X… N%` com barra, `Instalando a versão X. O app reabre sozinho.` O instalador roda com `/S /UPDATE /R`, como o updater da 0.1.48: silencioso, sem recriar atalhos e reabrindo o app.

### CLIs

Com o app em dia, a tela mostra `Preparando os CLIs…` e uma linha por CLI com versão e passo (conferindo, instalando, atualizando, pronto). `claude` e `codex` são exigidos qualquer que seja a escolha dos papéis e correm em paralelo. Para cada um:

1. Localiza o binário nativo `.exe` nas pastas do `PATH` lido do registro (máquina, depois usuário); senão, no local do instalador oficial: `%USERPROFILE%\.local\bin\claude.exe` e `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin\codex.exe`.
2. Se faltar, instala pelo script oficial no Windows PowerShell do sistema, sem janela e sem administrador: `irm https://claude.ai/install.ps1 | iex` e `irm https://chatgpt.com/codex/install.ps1 | iex` (este com `CODEX_NON_INTERACTIVE=1`).
3. Confere com `--version` (prazo de 30 s). Sem resposta, roda o instalador por cima uma vez e confere de novo.
4. Roda `claude update` ou `codex update` (prazo de 15 min cada).

O caminho absoluto encontrado é o usado para abrir orquestrador e executores e para `codex debug models`; o app não depende do `PATH` do processo. O login não é conferido: na primeira vez, o CLI pede no terminal dele.

Falha em qualquer passo mantém o uso bloqueado, com o CLI, o passo e o fim da saída do comando, e `Tentar novamente`, que refaz a rodada inteira. Não há opção de continuar.

Medido com Claude Code 2.1.325 e Codex 0.169.0, ambos em dia: `--version` em 72 ms e 126 ms; `claude update` em 2,0 s; `codex update` em 1,5 a 2,1 s (reexecuta o script oficial). Os dois `update` saíram com código 0 com outro processo do mesmo CLI aberto; com versão nova disponível nessa situação, não foi medido. Sob a política de execução `Restricted`, os cmdlets que os scripts usam carregaram; a instalação pelos scripts não foi rodada nesta medição.

Não há downgrade: só instala versão maior que a instalada, e a versão anunciada precisa ser a versão assinada (`version:` no comentário confiável do `.sig`). A atualização troca só os arquivos do programa; `%LOCALAPPDATA%\orquestrador\config.json` e os arquivos dos projetos ficam como estão.

Falha na verificação, no download ou na instalação (inclusive sem internet) mostra o motivo e **mantém o uso bloqueado**, com `Tentar novamente`. Não há opção de continuar na versão antiga. No build de debug (`cargo run -p orquestrador-app`) o uso fica liberado direto e nada é consultado; dos CLIs, só localiza, na mesma ordem, com `ORQ_CLAUDE_EXE` e `ORQ_CODEX_EXE` à frente, e sem achar usa o nome `claude.exe` ou `codex.exe`. Não instala nem roda `--version` ou `update`. Os modos `--hook`, `--allow` e `--mcp` do executável não abrem janela nem passam pelo portão.

O repositório é público, então o app baixa a release sem token. Se o repositório virar privado, a atualização automática deixa de funcionar e a distribuição precisa mudar.

## Antes da primeira release

Enquanto não houver release publicada, `releases/latest/download/latest.json` responde 404. Um build instalado fica então bloqueado com `nenhuma versão estável publicada foi encontrada` e `Tentar novamente`. Para usar o app antes disso, rode em desenvolvimento (build de debug) ou publique a primeira release.

## Chave de assinatura

- Chave privada: `%USERPROFILE%\.tauri\orquestrador-updater.key`, fora do repositório, com acesso só para o usuário. Não tem senha. A chave pública está em `PUBKEY_OFICIAL` de `crates/app/src/atualizacao/mod.rs`, igual à do updater da 0.1.48, guardada em `crates/app/empacotamento/compat-tauri/updater-0.1.48.json` (um teste confere).
- Guarde um backup da chave privada. Sem ela não há como publicar atualizações que os apps instalados aceitem; trocar a chave exige nova instalação manual de todos.
- Secret do workflow, uma vez (o conteúdo da chave vai pela entrada padrão e não aparece no terminal):

```powershell
Get-Content -Raw "$env:USERPROFILE\.tauri\orquestrador-updater.key" | gh secret set TAURI_SIGNING_PRIVATE_KEY -R luansilvadb/orquestrador
```

## Empacotamento

`crates/app/empacotamento/`: `empacotar.ps1` compila o app, monta o instalador com o `cargo-packager` 0.11.8 (NSIS por usuário, modelo `installer.nsi` com os hooks de `hooks.nsh` e `/UPDATE`), assina com `assinador/` e gera o `latest.json`. Instala as DLLs do FFmpeg e `FFmpeg-LICENSE.txt` ao lado do exe. Exige o FFmpeg (`crates/midia/preparar-ffmpeg.ps1`) e `cargo install cargo-packager --version 0.11.8 --locked`.

```powershell
pwsh crates/app/empacotamento/empacotar.ps1 -Versao 0.2.0 -Chave "$env:USERPROFILE\.tauri\orquestrador-updater.key" -Notas docs/releases/v0.2.0.md
```

Saída em `target\pacote\`: `Orquestrador_<versão>_x64-setup.exe`, o `.sig` e o `latest.json`. `-Binario` é o nome do exe instalado (`orquestrador`, o do 0.1.48) e `-Origem` o bin compilado (`orquestrador`).

### Compatibilidade com o 0.1.48 (Tauri)

O instalador novo usa o mesmo produto (`Orquestrador`), identificador, publicador (`luansilvadb`), pasta (`%LOCALAPPDATA%\Orquestrador`), chave de desinstalação (`HKCU\...\Uninstall\Orquestrador`), ícone e exe (`orquestrador.exe`); os hooks geram as mesmas chaves do instalador Tauri da 0.1.48. O `.sig` leva `version:` no comentário confiável, que o 0.1.48 exige, e o `latest.json` traz `windows-x86_64` e `windows-x86_64-nsis`. `compat-tauri/` roda o `tauri-plugin-updater` 2.14.1 do 0.1.48 contra os artefatos:

```powershell
cargo run --release --manifest-path crates/app/empacotamento/compat-tauri/Cargo.toml -- verificar <pubkey> target\pacote\latest.json target\pacote\Orquestrador_<versão>_x64-setup.exe
```

`compat instalar <pubkey> <url do latest.json>` faz a consulta, o download e a instalação de verdade pelo plugin, como o 0.1.48 faria.

### Teste de ponta a ponta local

Com chave temporária e nomes de teste, para não tocar na instalação real: `ORQ_ATUALIZACAO_URL` e `ORQ_ATUALIZACAO_CHAVE` na compilação trocam canal e chave, e `ORQ_INSTANCIA` troca o mutex e o pipe da instância única (sem isso, com o app real aberto, o de teste cede a vez a ele e sai); `-Produto OrquestradorTeste -Binario orquestrador-teste` trocam pasta, registro e associações. O roteiro está em `crates/app/empacotamento/e2e.ps1`.

## Publicar uma versão

O processo completo, com o modelo das notas e a checklist, está em [notas de release](releases/README.md). Resumo:

1. Suba a versão em `Cargo.toml` (`[workspace.package]`) e no `Cargo.lock` (`orquestrador-core` e `orquestrador-app`); `cargo xtask release check-versions X.Y.Z` confere. Ela precisa ser maior que a última publicada.
2. Escreva as notas em `docs/releases/vX.Y.Z.md`. O workflow recusa notas ausentes, vazias ou genéricas.
3. Faça o commit e crie a tag: `git tag vX.Y.Z` e depois `git push origin vX.Y.Z`.
4. O workflow `.github/workflows/release.yml` confere as versões e as notas, prepara o cache de compilação, gera pelo `empacotar.ps1` o instalador NSIS assinado e o `latest.json` com as notas completas, sobe tudo num rascunho e confere com o assinador e com o `compat-tauri`. Só então, com a CI do mesmo SHA verde, publica a release como a `latest` que os apps consultam.
5. Se o job falhar antes da publicação, a release fica em rascunho e não chega aos apps. Se falhar durante a publicação, ela pode já estar publicada. Uma release publicada não é alterada por nenhum passo: para corrigir, publique uma versão nova. O detalhe de cada caso está em [notas de release](releases/README.md), na seção "Se algo der errado".

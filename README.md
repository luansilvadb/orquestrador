# Orquestrador

## 1. O que é

Um app para Windows (Tauri) que põe CLIs de IA para trabalhar em papéis separados sobre um repositório Git. Cada CLI tem uma aba nos terminais da tela principal, com o terminal dele ao vivo. O orquestrador alinha o objetivo com o usuário, lê o projeto, delega e, quando o usuário pede, faz stage, commit e push; os executores recebem as tarefas, cada um na sua aba, e operam o repositório em paralelo. As ferramentas `claude` e `codex` podem ocupar qualquer um dos dois papéis.

## 2. Requisitos

- Windows com WebView2.
- Git.
- Os CLIs `claude` e `codex` no `PATH`, somente os que forem usados.
- Para compilar: Node.js e Rust (toolchain MSVC).

## 3. Como rodar

```powershell
npm install
npm run tauri dev      # desenvolvimento: uso liberado direto, sem consultar atualizações
npm run tauri build    # gera o instalador assinado em src-tauri\target\release\bundle\nsis (exige a chave, seção 8)
```

## 4. Como se usa

1. Abra o app. Antes da escolha da pasta, ele confere a versão e atualiza sozinho se houver uma nova (seção 8). Depois, escolha a pasta do projeto. Uma pasta fora de um repositório Git é recusada.
2. Na tela principal, sem orquestrador aberto, clique em `Abrir terminal` e escolha ferramenta, modelo e esforço do orquestrador; a última escolha já vem marcada. Há um só orquestrador, e o terminal dele é o único em que se digita.
3. Converse com o orquestrador e feche o objetivo.
4. Na primeira delegação, uma aba de executor aparece e espera a escolha de ferramenta, modelo e esforço; a aba ativa não muda. Os executores seguintes abrem sozinhos com essa escolha. O botão `executores: ...` na barra do orquestrador mostra a escolha e deixa trocá-la; a troca vale para os próximos.
5. O orquestrador delega um lote e fica ocupado até todos voltarem. Cada executor tem uma aba com a tela completa do CLI, somente leitura, ligada ao orquestrador. Acima do teto de simultâneos (ajustável na barra superior, padrão 4), os excedentes ficam na fila.
6. Quando um executor termina a rodada, a resposta dele volta sozinha ao orquestrador e a aba dele fecha sozinha. O botão `parar` interrompe só aquele executor; o orquestrador recebe `interrompido pelo usuário` no lugar da resposta.
7. Se o CLI de um executor parar num pedido de confirmação, a aba e a barra dela passam a `aguardando orquestrador`, e o executor continua ocupando o seu lugar no teto de simultâneos. Quem responde é o orquestrador, não você: ele recebe a pergunta com a ferramenta e o comando e aceita, ou recusa com um motivo que vai ao executor (seção 5). Você não precisa agir na aba. O botão `parar` continua valendo ali e encerra o executor como interrompido.
8. Fechar o app com terminais vivos pede confirmação e encerra todos. Nada é restaurado na próxima abertura; o que já mudou nos arquivos fica no projeto.

A tela principal tem, à esquerda, a árvore de pastas do projeto numa faixa fixa de 280 px, com rolagem própria. À direita, os terminais ocupam o resto da janela: uma faixa de abas em cima e, embaixo, o terminal da aba ativa. A primeira aba é o Orquestrador, fixa; cada executor tem uma aba só com o ícone do estado e o número `#N`. A barra acima do terminal mostra o estado, o número e o título da tarefa, a escolha e o botão `parar`. Trocar de aba não desmonta nenhum terminal: os outros só ficam escondidos. Dentro de um terminal, a roda do mouse rola o terminal.

À esquerda da área central fica uma coluna com dois ícones: Arquivos e Git. Arquivos abre a tela principal, com a árvore de pastas e os terminais lado a lado; Git troca a área central inteira pela tela do Git. O ícone da tela aberta fica marcado. O ícone do Git mostra um contador com os arquivos alterados (some quando é zero). A árvore mostra a pasta do projeto: pastas abrem e fecham com um clique, e clicar em arquivo ainda não faz nada. Ela se atualiza sozinha a cada 1,5 s, com arquivos novos e apagados. Quando a tela Git está aberta, a árvore e os terminais continuam montados, só saem da vista.

A tela Git tem o painel à esquerda e, à direita, as alterações de um arquivo por vez: o da linha clicada, que fica marcada. O painel, de cima para baixo: a branch atual (abre a lista das branches locais e um campo para criar uma nova, já trocando para ela); os botões Buscar, Receber e Enviar, com o número de commits que faltam receber ou enviar; o campo da mensagem, com o botão Commit (Ctrl+Enter também commita); e as seções Preparadas e Alterações. O botão de cada linha prepara ou tira o arquivo. O painel se atualiza sozinho e a cada ação. Com executor trabalhando, Receber, trocar de branch e criar branch ficam travados até o lote terminar; o resto segue normal. O commit pelo painel é seu; o orquestrador continua podendo commitar quando você pede.

A borda direita do painel arrasta para mudar a largura, que volta ao padrão ao reabrir o app. Embaixo, a barra de status mostra a branch atual do projeto.

No terminal do orquestrador, `Ctrl+V` cola texto normalmente. Se o clipboard tiver uma imagem (uma captura de tela, por exemplo), o app envia ao orquestrador o gesto de colar imagem dele (`Ctrl+V` no Codex, `Alt+V` no Claude Code), e o CLI lê a imagem do clipboard do Windows e a anexa à mensagem em edição, sem enviá-la. Se o clipboard tiver arquivos copiados no Explorer, o app cola o caminho completo de cada um (entre aspas quando tem espaço); o CLI anexa o que for imagem e recebe o resto como caminho. Vários arquivos vão do jeito que cada CLI aceita (medido no Claude Code 2.1.296 e no Codex 0.162.1): no Claude, num paste só, um caminho por linha; no Codex, um paste por caminho, com um intervalo curto entre eles.

## 5. Como funciona uma delegação

Além do `git` (seção 7), o orquestrador tem um único comando de delegação por sessão: o nome do adaptador da sessão, `orq-<id>.ps1`, seguido de uma ou mais tarefas, cada uma como um argumento entre aspas simples. O adaptador fica na pasta da sessão, e essa pasta entra na frente do `PATH` do orquestrador: o comando não leva caminho nem aspas em volta. A linha inteira deve ser esse comando: sem `&` antes, sem pipe, sem `;` e sem outro comando junto. Um apóstrofo dentro de uma tarefa deve ser duplicado, como em `d''água`. O resto vai como está: aspas duplas, barras e acentos, inclusive uma barra no fim da tarefa.

```powershell
orq-<id>.ps1 'Tarefa A. Escopo: ... Pronto quando: ...' 'Tarefa B. Escopo: ... Pronto quando: ...'
```

O mesmo comando aceita `'--aguardar'` (volta a esperar o lote) e `'--responder' '<executor>' 'aceitar'` ou `'--responder' '<executor>' 'recusar' 'motivo'` (responde a uma pergunta de executor, abaixo).

O programa `orquestrador-delegar.exe` vai junto com o app: no instalador ele é recurso ao lado de `orquestrador.exe`, e em `npm run tauri dev` e no build de release os comandos `dev:tauri` e `build:tauri` (em `package.json`) o compilam antes. No empacotamento, o hook `beforeBundleCommand` (`npm run copy:delegar`) copia o programa de release para `src-tauri\target\delegar`, a pasta que vai para o instalador; se ele não existir, o build para em vez de empacotar uma cópia antiga. Nos outros builds essa pasta sai sem o programa (`build.rs`), porque o `tauri-build` copia o que houver nela para a pasta do perfil, onde o Cargo põe o próprio programa. Ele é de console, de propósito: o PowerShell espera o término de um programa de console e recebe o código de saída dele; um programa de janela, não (medido: `$LASTEXITCODE` fica vazio e a saída chega depois).

O adaptador `orq-<id>.ps1` é escrito pelo app na pasta da sessão (`src-tauri/src/delegate.rs`, função `adapter`) e só transporta: cada texto vai ao programa como base64 do UTF-8, numa linha pela entrada padrão (stdin), e o processo sai com o código que o programa devolve. Quem publica, espera e imprime o lote é o programa em Rust, que lê a entrada até o fim. Nenhum argumento leva o lote: a linha de comando do Windows tem limite de 32767 caracteres, e o base64 cresce um terço (medido: três tarefas de 9000 caracteres falhavam antes de o programa rodar). Entrada vazia, cortada (sem o fim de linha que o PowerShell acrescenta) ou ilegível fecha com `wrapper_error:`, código 4 e `--- FIM ---`, e não publica nada. O Windows PowerShell 5.1 põe um BOM no começo da entrada; o programa o tira. A regra que libera o comando cobre só o adaptador daquela sessão: ele não tem outros modos, e `orquestrador.exe --hook`, `--guard` e `--ask` ficam fora da permissão do orquestrador.

Por que há um adaptador: medido no Windows PowerShell 5.1 com um programa de teste, o PowerShell altera o texto que passa a um programa nativo. A aspa dupla pura some, `\"` vira `"`, `a\\"b` vira `a\b`, e a barra no fim de uma tarefa com espaço vira aspa. O pwsh entrega o texto cru, mas o comando precisa valer nos dois shells, e nenhuma grafia que o orquestrador escreve chega igual nos dois. Argumento de script chega exato nos dois, por isso o adaptador é um `.ps1`.

Por que o código de saída sai por `[Environment]::Exit`: com `-Command`, o `exit` do script não vira o código do processo. Medido nos dois shells, o código 4 do programa saía como 1, e o orquestrador veria uma falha sem o motivo. O adaptador encerra o processo com o código do programa, e o harness de teste não acrescenta `exit` nenhum, para testar o caminho real. A preferência `$PSNativeCommandUseErrorActionPreference` fica desligada no adaptador, para o perfil do pwsh 7.4 em diante não mudar o código.

Cada argumento abre um executor novo, sem memória das tarefas anteriores. Todos trabalham na mesma pasta; quem garante que as tarefas de um lote não se cruzam é o orquestrador, pelas instruções de `src-tauri/src/roles.rs`.

Quando o lote termina, o comando imprime, em UTF-8:

- Para cada tarefa, na ordem: `--- EXECUTOR i/N ---`, o título, a última resposta do executor e `executor_exit` (`0`, ou o motivo seguido de `(FALHOU)`).
- `--- GIT ---` com a saída de `git status --porcelain --untracked-files=all`: tudo o que está alterado no repositório naquele momento, de todos os executores e também o que já estava pendente antes.
- `--- FIM ---`: a linha que fecha toda saída completa.

O código de saída é `0` se todas as tarefas concluíram, `1` se alguma falhou, e `3` quando a espera para por causa de uma pergunta de executor (abaixo).

Se a ferramenta do orquestrador desistir de esperar antes do fim, os executores continuam. `orq-<id>.ps1 '--aguardar'` volta a esperar o lote mais recente ainda não entregue, sem publicar nada. Uma falha do próprio comando imprime `wrapper_error:`, fecha com `--- FIM ---` e sai com `4`; isso vale para tarefa vazia, falta de tarefa, sessão encerrada e uma resposta inválida a `--responder`.

Perguntas de executor. Quando um executor do lote para num pedido de confirmação, a espera volta na hora, com código `3`, sem esperar o resto do lote. A saída tem uma seção por pergunta sem resposta, na ordem das tarefas, e depois a linha de resposta:

```
--- PERGUNTA exec-2 (1/3) ---
<título da tarefa>
O executor parou num pedido de confirmação e espera a sua resposta.
Bash: <o comando que o executor quer rodar>
Para responder: orq-<id>.ps1 '--responder' '<executor>' 'aceitar', ou orq-<id>.ps1 '--responder' '<executor>' 'recusar' 'motivo'. <executor> é o de cada pergunta acima.
--- AGUARDANDO RESPOSTA ---
```

`--- AGUARDANDO RESPOSTA ---` é a última linha. A saída não traz `--- FIM ---`, porque o lote não acabou. O número `(i/n)` é a posição da tarefa no lote. `--responder` grava a decisão da pergunta mais antiga desse executor e volta a esperar o lote; a pergunta respondida não volta. Uma pergunta que fica sem resposta por cerca de 100 s (seção 7) sai da espera: o executor recebe a negativa, repete o comando, e a pergunta volta como pergunta nova. Sem pergunta pendente desse executor, com resposta desconhecida ou com `recusar` sem motivo, nada é gravado e o comando sai com `4`. `--aguardar` devolve de novo a pergunta enquanto ela não tiver resposta nem tiver expirado. Os outros executores do lote seguem trabalhando nesse meio-tempo.

Limites, testados no Windows PowerShell 5.1 e no pwsh 7.6 (`src-tauri\tests\lote.rs`):

- O texto de cada tarefa chega igual ao pedido publicado nos dois shells: aspas duplas puras, barra antes da aspa, JSON com escapes, barra final, acentos, apóstrofos e várias linhas. Tarefa vazia ou só de espaços é recusada e não publica nada.
- O caminho da pasta da sessão e o do programa podem ter espaço, acento e apóstrofo. Não há exigência de nome curto 8.3 nem de ativar nada no volume.
- O `.ps1` precisa de uma política de execução que deixe rodar scripts locais no shell do orquestrador. Nesta máquina o PowerShell 5.1 está com `RemoteSigned` na `LocalMachine` e o pwsh também. Com `Restricted`, o PowerShell recusa o adaptador e a delegação falha com a mensagem dele. O app não muda a política do usuário.
- Não foi medido, sem chamar modelo, como o Claude Code e o Codex casam a linha de comando com a regra. O Codex foi medido só nos argumentos (`codex execpolicy check`): o token do adaptador casa, e a regra da versão anterior também. A confirmação de fato é a primeira delegação real.

O fim da rodada de um executor é detectado pelo hook `Stop` do Claude Code e pelo `notify` do Codex, que chamam o próprio executável do app.

## 6. Configuração

A última escolha de cada papel e o teto de simultâneos ficam em `%LOCALAPPDATA%\orquestrador\config.json`. O projeto não é salvo.

```json
{
  "orchestrator": { "tool": "codex", "model": "", "effort": "" },
  "executor": { "tool": "claude", "model": "", "effort": "" },
  "maxParallel": 4
}
```

Modelo e esforço vazios deixam o CLI usar seus padrões. Esforços aceitos: Claude `low`, `medium`, `high`, `xhigh`, `max`; Codex os mesmos e `ultra`. O nome do modelo deve corresponder a `^[A-Za-z0-9][A-Za-z0-9._:/@+\[\]-]*$`. Um JSON ilegível ou valor inválido é um erro; não há fallback silencioso para os padrões.

## 7. Limites de segurança

**Orquestrador Codex.** Sandbox `read-only`, sem aprovações. Um arquivo de regras `prefix_rule` no diretório de regras do Codex libera, naquela sessão, o adaptador de delegação (um nome só, sorteado por sessão), `git` e `git.exe` com qualquer subcomando, e os programas das skills globais; o arquivo é removido no encerramento. O sandbox ainda pode ler o disco inteiro. Medido no Codex 0.162.1: com o sandbox de leitura, `git add` falha no `.git` (`index.lock: Permission denied`); com a regra `allow` do `git` ele passa, porque a regra autoriza rodar o comando fora do sandbox (é o que o próprio Codex 0.162.1 declara nas instruções de execução); um comando sem regra é recusado pela política (`blocked by policy`), inclusive escrita de arquivo por PowerShell. Também medido no 0.162.1: a regra deixa de valer quando o comando traz o separador `--` solto (`git add -- a.txt`) ou um pipe para outro programa (`git add a.txt | Out-Null`); o comando roda então dentro do sandbox e falha no `.git` com `Permission denied`. Aspas simples ou duplas, `-C` com caminho absoluto, `--all`, `--message=x`, vários `-m` e encadeamento de comandos git com `;` ou `&&` passam. Por isso o texto do orquestrador, em `src-tauri/src/roles.rs`, manda escrever os comandos git sem o `--`.

**Alcance das regras do Codex.** O arquivo de regras fica no diretório de configuração do Codex (`CODEX_HOME`), e as regras valem para qualquer processo `codex` que use esse diretório, não só para o orquestrador, enquanto a sessão existir. Medido no 0.162.1: um `codex exec` comum, em outra pasta, rodou `git add` com a regra da sessão presente. Com um orquestrador Codex aberto, o `git` liberado alcança, portanto, os outros Codex dessa mesma configuração. As opções do `codex exec` não trazem um arquivo de regras por processo; `--ignore-rules` só desliga o carregamento.

**Orquestrador Claude.** `--restricted`, `--permission-mode dontAsk`, sem servidor MCP. Ferramentas: `Read`, `Glob`, `Grep`, `PowerShell` e `Skill`; o PowerShell só passa o comando de delegação da sessão, `git` e `git.exe`, os programas das skills globais e o que o Claude Code considera somente leitura. Medido no Claude Code 2.1.296 com o `claude-settings.json` gerado pelo app: `git -C`, `git.exe` e `git commit` passam, e sem as regras `PowerShell(git *)` e `PowerShell(git.exe *)` o mesmo `git status` é negado. Também medido no 2.1.296: a regra casa o texto do comando com as aspas, então `node "C:\...\x.mjs" start` é negado por uma regra que traz o caminho sem aspas. Por isso cada programa de skill entra em três grafias (sem aspas, com aspas duplas e com aspas simples), nas duas barras; com elas, outro programa na mesma pasta e um segundo comando encadeado com `;` continuam negados.

A ativação é local à sessão. Nessa versão, no Windows com Git Bash instalado, o PowerShell só entra na lista de ferramentas com `CLAUDE_CODE_USE_POWERSHELL_TOOL=1` (medido com um stub local que mostra a lista enviada ao modelo: sem a variável, o PowerShell não aparece). O app põe essa variável no bloco `env` do `claude-settings.json` da sessão, que o orquestrador recebe por `--settings`; o ambiente do usuário e o dos executores não mudam.

**Git do orquestrador.** `git` e `git.exe` com qualquer subcomando, inclusive `push`, `reset --hard` e `clean`, porque o usuário pediu Git completo. A regra casa só o nome do programa, então os argumentos não são limitados: `-C` pode apontar para qualquer repositório. Os executores continuam barrados como antes. O orquestrador faz stage, commit e push só quando o usuário pede, e não desfaz trabalho sem pedido expresso; isso é orientação ao modelo, não uma trava do app.

**Skills do orquestrador.** Os programas `.mjs`, `.cjs`, `.js` e `.py` em `%USERPROFILE%\.agents\skills` e `%USERPROFILE%\.claude\skills` são liberados pelo nome no início da sessão. O que esses programas conseguem fazer, o orquestrador consegue fazer por meio deles.

**Git pelo painel.** O app roda `git` na pasta do projeto, só por clique do usuário: `git add -A` para preparar (com caminhos, só eles); para tirar do preparo, `git reset` só no índice, com os caminhos (ou tudo, sem caminho), sem `--hard`, e num repositório sem commit `git rm --cached`; a pasta de trabalho nunca é alterada por tirar do preparo. Também `commit`, `fetch`, `pull`, `push` e `git switch`, que troca de branch e cria uma nova. Não há descartar alterações, `reset --hard` nem push forçado. Os comandos rodam sem pedido de digitação no terminal (`GIT_TERMINAL_PROMPT=0`, sem editor), então a credencial vem do gerenciador de credenciais do Git, que pode abrir a janela dele. Receber, trocar e criar branch são recusados pelo backend enquanto houver executor que não terminou, e só uma dessas operações roda por vez.

**Executores.** Permissão total: Claude em `bypassPermissions`, Codex sem sandbox e sem aprovações. Continuam barrados `git push`, `git reset --hard`, `git clean`, `npm publish` e `pnpm publish`: no Claude por lista de negação e por um hook de guarda; no Codex por um hook `PreToolUse` que vale só para aquela sessão. As instruções do executor, em `src-tauri/src/roles.rs`, pedem ainda para não fazer commit, stage, deploy nem reproduzir segredos.

Quando o CLI do Claude ainda abre um pedido de confirmação, o hook `PermissionRequest` roda `orquestrador.exe --ask <pasta do nó>\ask`. O `--ask` publica o pedido na pasta do nó e espera a decisão por até 100 s (`ASK_DEADLINE`, em `src-tauri/src/hook.rs`); na definição do hook, o `timeout` é de 86400 s, para não ser ele a cortar. A aba do executor vai a `aguardando orquestrador` até a resposta. Se o prazo passa sem resposta, o `--ask` tira o pedido da pasta e nega com `O orquestrador ainda não respondeu a este pedido de confirmação...`, mensagem que manda repetir exatamente o mesmo comando. A aba volta a `executando`; quando o executor repete o comando, o CLI abre um pedido novo, que volta ao orquestrador como pergunta. Uma resposta que chega depois do prazo não tem efeito: o app não grava resposta para um pedido que já saiu da pasta. A decisão é do orquestrador, por orientação ao modelo em `src-tauri/src/roles.rs`; o app só confere a forma da resposta (aceitar, ou recusar com motivo), não o mérito. `allow` roda o comando e `deny` devolve a mensagem ao modelo como erro. Se o executor for parado ou encerrado antes da resposta, o pedido some e o `--ask` nega com `O executor foi encerrado antes de o orquestrador responder.`

Medido no Claude Code 2.1.296: um `rm` de arquivo na raiz do disco, fora do projeto, abre "Dangerous rm operation on critical path" mesmo em `bypassPermissions`, e nesse momento dispara o hook `PermissionRequest`. `allow` do hook roda o comando, e `deny` devolve a mensagem ao modelo como erro. Com o hook esperando 240 s, o diálogo apareceu aos ~4 s com "will automatically deny this request in 1:59"; aos ~124 s o próprio Claude Code negou ("Permission for this command was denied by a built-in Claude Code safety check, not by the user"), com o hook ainda rodando; o hook terminou aos ~244 s com `allow`, e a decisão não teve efeito: o comando não rodou. Ou seja: o hook só decide se responder em menos de 2 minutos contados do diálogo. Depois disso, o relógio embutido decide e a resposta tardia é ignorada. Por isso o prazo do `--ask` (100 s) fica abaixo desse limite. O `allow` do hook `PreToolUse` não evita essa confirmação.

Codex 0.162.1, medido com `--dangerously-bypass-approvals-and-sandbox` e `--dangerously-bypass-hook-trust` (o executor Codex): um `Remove-Item D:\arquivo.log`, fora do projeto, rodou sem nenhuma confirmação na tela; o hook `PreToolUse` disparou (`tool_name` "Bash", `permission_mode` "bypassPermissions") e o hook `PermissionRequest` não disparou. Por isso o executor Codex não recebe o hook `PermissionRequest`: com o bypass não há pedido para o orquestrador responder. Aceitar a chave na partida não basta, porque o evento não dispara. O `PreToolUse` de guarda continua valendo.

Essas barreiras comparam o texto do comando: orientam e pegam as grafias óbvias, não são uma parede. No Codex, se o hook de guarda falhar ao rodar, o comando passa.

Para o executor não parar num diálogo que ninguém pode responder, a confiança na pasta do projeto é dada pela sessão: no Codex por configuração da linha de comando, no Claude pela variável `CLAUDE_CODE_SANDBOXED=1`, que não é documentada e pode mudar entre versões do Claude Code. Escolher a pasta no app equivale a confiar nela.

Na inicialização de uma sessão, regras e pastas de sessões mortas são removidas quando o nome, a primeira linha e o dono encerrado comprovam a origem; o que não puder ser verificado é preservado. A primeira linha pode ser a regra atual ou a da versão anterior, que liberava o launcher `delegate.ps1` pelo caminho; qualquer outra, inclusive uma parecida, não é do app. A regra da delegação continua na primeira linha do arquivo; as do Git e as das skills vêm depois, no mesmo arquivo, e saem com ele.

## 8. Atualização automática

Em toda abertura do app instalado, antes de liberar a escolha da pasta ou qualquer terminal, ele consulta a última release **estável** de `luansilvadb/orquestrador` no GitHub (`releases/latest/download/latest.json`). Rascunhos e pré-releases não chegam ao app. Se houver versão superior, o app mostra o andamento, baixa, valida a assinatura, instala em modo silencioso (NSIS por usuário, sem administrador) e reabre sozinho na versão nova. Se já estiver em dia, libera o uso normal.

Não há downgrade: só instala versão maior que a instalada, e a versão anunciada precisa ser a versão assinada (`requireSignedVersion`). A atualização troca só os arquivos do programa; `%LOCALAPPDATA%\orquestrador\config.json` e os arquivos dos projetos ficam como estão.

Falha na verificação, no download ou na instalação (inclusive sem internet) mostra o motivo e **mantém o uso bloqueado**, com `Tentar novamente`. Não há opção de continuar na versão antiga. Em `npm run tauri dev` (build de debug) o uso fica liberado direto e nada é consultado. Os modos `--hook`, `--guard` e `--ask` do executável, e o programa `orquestrador-delegar.exe`, não abrem janela nem passam pelo updater.

O repositório é público, então o app baixa a release sem token. Se o repositório virar privado, a atualização automática deixa de funcionar e a distribuição precisa mudar.

### Antes da primeira release

Enquanto não houver release publicada, `releases/latest/download/latest.json` responde 404. Um build instalado fica então bloqueado com `nenhuma versão estável publicada foi encontrada` e `Tentar novamente`. Para usar o app antes disso, rode em desenvolvimento (`npm run tauri dev`) ou publique a primeira release.

### Chave de assinatura

- Chave privada: `%USERPROFILE%\.tauri\orquestrador-updater.key`, fora do repositório, com acesso só para o usuário. Não tem senha. A chave pública está em `plugins.updater.pubkey` de `src-tauri/tauri.conf.json`.
- Guarde um backup da chave privada. Sem ela não há como publicar atualizações que os apps instalados aceitem; trocar a chave exige nova instalação manual de todos.
- Secret do workflow, uma vez (o conteúdo da chave vai pela entrada padrão e não aparece no terminal):

```powershell
Get-Content -Raw "$env:USERPROFILE\.tauri\orquestrador-updater.key" | gh secret set TAURI_SIGNING_PRIVATE_KEY -R luansilvadb/orquestrador
```

- Build local assinado, para testar o instalador:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = "$env:USERPROFILE\.tauri\orquestrador-updater.key"
npm run tauri build
```

Saída: `src-tauri\target\release\bundle\nsis\Orquestrador_<versão>_x64-setup.exe` e o `.sig` ao lado.

### Publicar uma versão

1. Suba a versão nos três arquivos iguais: `package.json`, `src-tauri/tauri.conf.json` e `src-tauri/Cargo.toml`. Ela precisa ser maior que a última publicada.
2. Faça o commit e crie a tag: `git tag vX.Y.Z` e depois `git push origin vX.Y.Z`.
3. O workflow `.github/workflows/release.yml` confere se a tag bate com as três versões, gera o instalador NSIS assinado e o `latest.json` e cria a release **em rascunho**.
4. Ao fim do job, uma verificação confere o instalador, o `.sig` e o manifesto (versão, URL e assinatura). Se ela passar, revise a release e publique o rascunho no GitHub. Só a release publicada vira a `latest` que os apps consultam.
5. Se o job falhar, o rascunho fica sem publicar: apague o rascunho e a tag, corrija e faça o push da tag de novo.

## 9. Arquivos e testes

| Caminho | Papel |
| --- | --- |
| `src/` | Frontend em React: tela de atualização, a tela principal (árvore de pastas e terminais), a tela Git, terminais (xterm), formulário de escolha. `mock.ts` é um backend simulado, só em `npm run dev` fora do Tauri. O design system (Lobe UI) está em `docs/design-system.md`. |
| `src/terminal-screen.tsx` | Os terminais da tela principal: faixa de abas (orquestrador e um executor por aba), a barra da aba ativa e o terminal dela. |
| `src/explorer.tsx` | A coluna de ícones da navegação, a árvore de pastas (na tela principal) e a barra de status com a branch. |
| `src/git.tsx` | A tela Git: o painel (branch, commit, preparar e tirar) e, à direita, as alterações do arquivo selecionado. Os comandos vêm do backend. |
| `src-tauri/src/git.rs` | Os comandos do painel Git: status, diff, preparar, commit, fetch, pull, push e branches. |
| `src-tauri/src/` | Backend: PTYs, sessão e travas do orquestrador, lotes, executores e configuração. |
| `src-tauri/src/delegate.rs` | A lógica do programa de delegação: publica o lote na pasta da sessão, espera o resultado e o imprime; e o texto do adaptador `.ps1` da sessão, que só transporta. O ponto de entrada, `orquestrador-delegar.exe`, é `src-tauri/src/bin/orquestrador-delegar.rs`. |
| `src-tauri/src/hook.rs` | Os modos do executável chamados pelos CLIs dos executores: `--hook` (fim de rodada), `--guard` (comandos barrados) e `--ask` (pedido de confirmação, espera a resposta do orquestrador). |
| `src-tauri/src/session.rs` | A sessão do orquestrador: pasta, adaptador, regras do Codex (delegação, Git e skills), `claude-settings.json` do Claude e a limpeza das sobras. |
| `src-tauri/src/roles.rs` | Os textos enviados ao orquestrador (no `AGENTS.md` da sessão) e a cada executor, antes da tarefa. |
| `src-tauri/src/update.rs` | Atualização na abertura: o portão do uso, a fonte do plugin oficial e os testes dos cenários. |
| `.github/workflows/release.yml` | Release por tag: instalador assinado, `latest.json` e conferência, em rascunho. |

```powershell
cd src-tauri
cargo test
```

Os testes usam CLIs simulados e não chamam nenhum modelo. `tests\lote.rs` roda o programa `orquestrador-delegar.exe` de verdade no Windows PowerShell 5.1 e no pwsh. `tests\real.rs` faz o ciclo com os CLIs reais e fica ignorado por padrão: defina `ORQ_REAL_REPO` e `ORQ_REAL=codex,claude` e rode `cargo test --test real -- --ignored --nocapture`.

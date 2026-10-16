# Uso

## Visão geral

Um app para Windows (Tauri) que põe CLIs de IA para trabalhar em papéis separados sobre a pasta de um projeto, de preferência um repositório Git. Cada CLI tem uma aba no painel do Orquestrador, à direita da tela principal, com o terminal dele ao vivo. O orquestrador alinha o objetivo com o usuário, lê o projeto, delega e, quando o usuário pede, faz stage, commit e push; os executores recebem as tarefas, cada um na sua aba, e trabalham na pasta do projeto em paralelo. As ferramentas `claude` e `codex` podem ocupar qualquer um dos dois papéis.

## Como se usa

1. Abra o app. Antes da escolha da pasta, ele confere a versão e atualiza sozinho se houver uma nova ([atualização](atualizacao.md)). Depois, escolha a pasta do projeto. Uma pasta dentro de um repositório abre a raiz dele, como antes; se a pasta escolhida for uma subpasta, um aviso diz qual é a raiz e que o projeto aberto é ela. Uma pasta fora de repositório abre normalmente, sem os recursos de Git (ver [interface](interface.md)). Uma pasta que não existe é recusada.
2. No painel do Orquestrador, à direita da tela principal, sem orquestrador aberto, clique em `Abrir terminal` e escolha ferramenta, modelo e esforço do orquestrador; a última escolha já vem marcada. Sem Git na pasta, a janela avisa que o que os executores mudarem não fica registrado e não pode ser desfeito pelo Git, e diz o que fazer conforme o caso: inicializar o repositório no painel Git, instalar o Git ou abrir a raiz do repositório. O aviso não bloqueia. Há um só orquestrador, e é no terminal dele que você conversa com ele; os executores só mostram o que fazem.
3. Converse com o orquestrador e feche o objetivo.
4. Na primeira delegação, uma aba de executor aparece e espera a escolha de ferramenta, modelo e esforço; a aba ativa não muda. Os executores seguintes abrem sozinhos com essa escolha. O botão `Executores`, na barra da aba do orquestrador, mostra a escolha e deixa trocá-la; a troca vale para os próximos.
5. O orquestrador delega um lote e fica ocupado até todos voltarem. Cada executor tem uma aba com a tela completa do CLI, somente leitura, ligada ao orquestrador. Acima do teto de simultâneos, os excedentes ficam na fila. O teto não é ajustado à mão: o app o calcula (ver [configuração](#configuração)), e a fila anda sozinha quando a memória livre volta.
6. Quando um executor termina a rodada, a resposta dele volta sozinha ao orquestrador e a aba dele fecha sozinha. O botão `parar` interrompe só aquele executor; o orquestrador recebe `interrompido pelo usuário` no lugar da resposta.
7. Se o CLI de um executor parar num pedido de confirmação, a aba e a barra dela passam a `aguardando orquestrador`, e o executor continua ocupando o seu lugar no teto de simultâneos. Quem responde é o orquestrador, não você: ele recebe a pergunta com a ferramenta e o comando e aceita, ou recusa com um motivo que vai ao executor (ver [delegação](delegacao.md)). Você não precisa agir na aba. O botão `parar` continua valendo ali e encerra o executor como interrompido.
8. Fechar o app com alterações não salvas abre a confirmação: Salvar (grava tudo; se algo falhar ou sobrar texto novo, o app não fecha), Descartar (joga a edição fora e fecha) ou Cancelar (nada muda). Com terminais vivos, a confirmação também encerra todos, os shells do painel Terminal inclusive; os shells não contam para a confirmação. Nada de shell ou CLI é restaurado na próxima abertura. As abas, o layout e os rascunhos que ficam (quando não houve descarte) voltam; o que já mudou nos arquivos fica no projeto.

## Issues

A tela Issues guarda as ideias de tarefa de cada projeto só neste computador, num arquivo do app (`%LOCALAPPDATA%\orquestrador\issues`, ver [arquitetura](arquitetura.md)): funciona com ou sem Git, sem GitHub e sem login. As issues não vão para a pasta do projeto nem para o repositório. O arquivo é ligado ao projeto pelo caminho da pasta: renomear ou mover a pasta faz o projeto começar sem issues (o arquivo antigo continua no app e só é recuperado à mão).

1. Enquanto o orquestrador trabalha, abra a aba Nova issue com `Ctrl+Shift+I` (de qualquer tela, menos com o foco no terminal) ou, na tela Issues, pelo `+` do cabeçalho da faixa (sem issue aberta, também pelo botão Nova issue da área central). O foco já vem no Título; Criar issue (ou Ctrl+Enter) grava a issue com o número seguinte (#1, #2…; o número de uma excluída não volta). A descrição e as labels são opcionais, e as labels são livres: o `+` lista as já usadas no projeto e tem “Nova label…”. A tela Issues ocupa o lugar do editor enquanto está aberta (o detalhe da issue e o formulário, em abas dela); ao voltar para Arquivos, Pesquisar ou Git, o editor reaparece como estava, e o texto do formulário espera você voltar.
2. Quando quiser começar uma, escolha-a na faixa (clique ou Enter na linha: o detalhe dela abre no centro) e clique em Colar no orquestrador no detalhe (ou em Colar na linha escolhida, ou aperte Ctrl+Enter na linha). O número, o título e a descrição vão para o rascunho do orquestrador, sem enviar: revise e envie (o Colar só cola, não envia). Com o orquestrador ocupado, o botão abre um menu: Colar agora (a issue em andamento volta às abertas), Colar quando o orquestrador terminar (ela é colada sozinha no fim da rodada, também sem enviar) ou, na que já espera, Não colar depois. A issue fica “colada, aguardando envio” até você enviar e uma rodada começar com o texto dela; só então aparece “orquestrador trabalhando”. Enquanto o texto está no rascunho e não foi enviado, o botão some (colar de novo duplicaria); depois de enviado, ele volta como Colar de novo. No detalhe da issue em andamento, o painel “Com o orquestrador” tem o estado, os executores agora, Ir ao orquestrador, Concluir e Voltar às abertas.
3. Terminada a tarefa, Concluir (no detalhe ou no cartão Em andamento) fecha a issue (se o orquestrador ainda não trabalhou nela, o app pergunta antes e lembra que o texto continua no rascunho dele). O aviso de fechada tem Desfazer, que reabre e devolve a issue a Em andamento; mais tarde, uma fechada volta com Reabrir, no detalhe dela (escolha-a em Fechadas recentes).
4. No detalhe, o lápis (Editar) abre a issue no formulário, para corrigir o título, a descrição e as labels, e a lixeira (Excluir) apaga a issue de vez, depois de perguntar.

A versão anterior (0.1.16) usava as issues do GitHub. As ideias que ela guardou sem conseguir criar lá viram issues locais na primeira vez que o projeto abre; as issues que já estão no GitHub continuam lá e não são importadas.

## Configuração

A última escolha de cada papel fica em `%LOCALAPPDATA%\orquestrador\config.json`. O projeto não é salvo.

```json
{
  "orchestrator": { "tool": "codex", "model": "", "effort": "" },
  "executor": { "tool": "claude", "model": "", "effort": "" }
}
```

Não há mais ajuste de simultâneos. O teto é automático e vem de duas medidas do computador, calculadas ao abrir o app (`src-tauri/src/capacity.rs`):

- **CPU:** metade das CPUs lógicas, de 1 a 16. Lida uma vez, ao abrir o app.
- **Memória física:** a RAM total, menos 2 GiB reservados ao sistema e ao app, dá um executor por GiB. A memória livre agora dá as vagas: o que sobra depois da mesma reserva de 2 GiB, um executor por GiB. O teto de cada passe é o menor entre o da CPU, o da RAM total e os executores já vivos mais as vagas da RAM livre. Se nada está vivo, um executor pode começar mesmo sem vaga, para a fila não parar.

Isso é um orçamento aproximado, não uma medida do peso de cada tarefa. Executor que já roda nunca é parado nem reiniciado por falta de memória: o teto só deixa de abrir novos. A fila é reavaliada a cada segundo, enquanto houver nó esperando. Se CPU ou memória não puderem ser lidas, o teto cai para 2, e o app abre do mesmo jeito. Uma chave `maxParallel` de versões antigas é ignorada e some do arquivo no próximo salvamento.

Modelo e esforço vazios deixam o CLI usar seus padrões. Esforços aceitos: Claude `low`, `medium`, `high`, `xhigh`, `max`; Codex os mesmos e `ultra`. O nome do modelo deve corresponder a `^[A-Za-z0-9][A-Za-z0-9._:/@+\[\]-]*$`. Um JSON ilegível ou valor inválido é um erro; não há fallback silencioso para os padrões.

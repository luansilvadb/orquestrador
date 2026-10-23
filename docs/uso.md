# Uso

## Visão geral

Um app para Windows (Tauri) que põe CLIs de IA para trabalhar em papéis separados sobre a pasta de um projeto, de preferência um repositório Git. Cada CLI tem uma aba no painel do Orquestrador, à direita da tela principal, com o terminal dele ao vivo. O orquestrador alinha o objetivo com o usuário, lê o projeto, delega, faz stage e commit do trabalho da sessão e consulta o usuário antes de push; os executores recebem as tarefas, cada um na sua aba, e trabalham na pasta do projeto em paralelo. As ferramentas `claude` e `codex` podem ocupar qualquer um dos dois papéis.

## Como se usa

1. Abra o app. Antes do uso, ele confere a versão e atualiza sozinho se houver uma nova ([atualização](atualizacao.md)). Depois, reabre o último projeto se a pasta ainda existir; senão, abre sem projeto. Nesse modo, use o menu `≡` ou a tela central para criar um arquivo, abrir um arquivo ou abrir uma pasta. Uma pasta dentro de um repositório abre a raiz dele; se for subpasta, um aviso mostra a raiz. Uma pasta fora de repositório abre sem os recursos de Git (ver [interface](interface.md)).
2. No painel do Orquestrador, à direita da tela principal, sem orquestrador aberto, clique em `Abrir terminal` e escolha ferramenta, modelo e esforço do orquestrador; a última escolha já vem marcada. Sem Git na pasta, a janela avisa que o que os executores mudarem não fica registrado e não pode ser desfeito pelo Git, e diz o que fazer conforme o caso: inicializar o repositório no painel Git, instalar o Git ou abrir a raiz do repositório. O aviso não bloqueia. Há um só orquestrador, e é no terminal dele que você conversa com ele; os executores só mostram o que fazem.
3. Converse com o orquestrador e feche o objetivo.
4. Na primeira delegação, uma aba de executor aparece e espera a escolha de ferramenta, modelo e esforço; a aba ativa não muda. Os executores seguintes abrem sozinhos com essa escolha. O botão `Executores`, na barra da aba do orquestrador, mostra a escolha e deixa trocá-la; a troca vale para os próximos.
5. O orquestrador delega um lote e fica ocupado até todos voltarem. Cada executor tem uma aba com a tela completa do CLI, somente leitura, ligada ao orquestrador. Acima do teto de simultâneos, os excedentes ficam na fila. O teto não é ajustado à mão: o app o calcula (ver [configuração](#configuração)), e a fila anda sozinha quando a memória livre volta.
6. Quando um executor termina a rodada, a resposta dele volta sozinha ao orquestrador e a aba dele fecha sozinha. O botão `parar` interrompe só aquele executor; o orquestrador recebe `interrompido pelo usuário` no lugar da resposta.
7. Fechar o app com alterações não salvas abre a confirmação: Salvar, Descartar ou Cancelar. Com terminais vivos, a confirmação também encerra todos, inclusive os shells do painel Terminal. Shells e CLIs não voltam na próxima abertura; abas, layout e alterações do editor voltam. Os rascunhos sem projeto também são recuperados.

## Configuração

A última escolha de cada papel fica em `%LOCALAPPDATA%\orquestrador\config.json`. O último projeto e até 10 projetos recentes ficam no `localStorage` do app; `Fechar pasta` limpa o último projeto, mas mantém os recentes.

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

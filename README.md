# Orquestrador

## 1. O que é

Um app de canvas para Windows (Tauri) que põe CLIs de IA para trabalhar em papéis separados sobre um repositório Git. Cada CLI é um bloco no canvas, com o terminal dele ao vivo. O orquestrador alinha o objetivo com o usuário, só lê o projeto e delega; os executores recebem as tarefas, cada um no seu bloco, e operam o repositório em paralelo. As ferramentas `claude` e `codex` podem ocupar qualquer um dos dois papéis.

## 2. Requisitos

- Windows com WebView2.
- Git.
- Os CLIs `claude` e `codex` no `PATH`, somente os que forem usados.
- Para compilar: Node.js e Rust (toolchain MSVC).

## 3. Como rodar

```powershell
npm install
npm run tauri dev      # desenvolvimento
npm run tauri build    # gera o executável e o instalador em src-tauri\target\release
```

## 4. Como se usa

1. Abra o app e escolha a pasta do projeto. Uma pasta fora de um repositório Git é recusada.
2. No canvas vazio, abra um terminal e escolha ferramenta, modelo e esforço do orquestrador; a última escolha já vem marcada. Há um só orquestrador por canvas, e o bloco dele é o único em que se digita.
3. Converse com o orquestrador e feche o objetivo.
4. Na primeira delegação, um bloco de executor aparece e espera a escolha de ferramenta, modelo e esforço. Os executores seguintes abrem sozinhos com essa escolha. A etiqueta `executores: ...` no bloco do orquestrador mostra a escolha e deixa trocá-la; a troca vale para os próximos.
5. O orquestrador delega um lote e fica ocupado até todos voltarem. Cada executor é um bloco com a tela completa do CLI, somente leitura, ligado ao orquestrador. Acima do teto de simultâneos (ajustável na barra superior, padrão 4), os excedentes ficam na fila.
6. Quando um executor termina a rodada, a resposta dele volta sozinha ao orquestrador e o bloco fica marcado como concluído até `limpar concluídos`. O botão `parar` interrompe só aquele executor; o orquestrador recebe `interrompido pelo usuário` no lugar da resposta.
7. Fechar o app com terminais vivos pede confirmação e encerra todos. Nada é restaurado na próxima abertura; o que já mudou nos arquivos fica no projeto.

Os blocos podem ser arrastados e redimensionados; a roda do mouse no fundo dá zoom, e dentro de um terminal rola o terminal.

## 5. Como funciona uma delegação

O orquestrador recebe um único comando permitido por sessão: o caminho para `delegate.ps1` dentro da pasta temporária da sessão, seguido de uma ou mais tarefas, cada uma como um argumento entre aspas simples. A linha inteira deve ser esse comando: sem `&` antes, sem pipe, sem `;` e sem outro comando junto. Um apóstrofo dentro de uma tarefa deve ser duplicado, como em `d''água`.

```powershell
<pasta da sessão>\delegate.ps1 'Tarefa A. Escopo: ... Pronto quando: ...' 'Tarefa B. Escopo: ... Pronto quando: ...'
```

Cada argumento abre um executor novo, sem memória das tarefas anteriores. Todos trabalham na mesma pasta; quem garante que as tarefas de um lote não se cruzam é o orquestrador, pelas instruções de `strategist.md`.

Quando o lote termina, o comando imprime:

- Para cada tarefa, na ordem: `--- EXECUTOR i/N ---`, o título, a última resposta do executor e `executor_exit` (`0`, ou o motivo seguido de `(FALHOU)`).
- `--- GIT ---` com a saída de `git status --porcelain --untracked-files=all`: tudo o que está alterado no repositório naquele momento, de todos os executores e também o que já estava pendente antes.
- `--- FIM ---`: a linha que fecha toda saída completa.

Se a ferramenta do orquestrador desistir de esperar antes do fim, os executores continuam. `delegate.ps1 '--aguardar'` volta a esperar o lote mais recente ainda não entregue, sem lançar nada. Uma falha do próprio wrapper imprime `wrapper_error:`, fecha com `--- FIM ---` e sai com `4`.

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

**Orquestrador Codex.** Sandbox `read-only`, sem aprovações. Um arquivo de regras `prefix_rule` no diretório de regras do Codex libera o launcher daquela sessão e os programas das skills globais, e é removido no encerramento. O sandbox impede escrita, mas ele ainda pode ler o disco inteiro.

**Orquestrador Claude.** `--restricted`, `--permission-mode dontAsk`, sem servidor MCP. Ferramentas: `Read`, `Glob`, `Grep`, `PowerShell` e `Skill`; o PowerShell só passa o launcher da sessão, os programas das skills globais e o que o Claude Code considera somente leitura.

**Skills do orquestrador.** Os programas `.mjs`, `.cjs`, `.js` e `.py` em `%USERPROFILE%\.agents\skills` e `%USERPROFILE%\.claude\skills` são liberados pelo nome no início da sessão. O que esses programas conseguem fazer, o orquestrador consegue fazer por meio deles.

**Executores.** Permissão total, sem perguntar: Claude em `bypassPermissions`, Codex sem sandbox e sem aprovações. Continuam barrados `git push`, `git reset --hard`, `git clean`, `npm publish` e `pnpm publish`: no Claude por lista de negação e por um hook de guarda; no Codex por um hook `PreToolUse` que vale só para aquela sessão. As instruções de `executor.md` pedem ainda para não fazer commit, stage, deploy nem reproduzir segredos.

Essas barreiras comparam o texto do comando: orientam e pegam as grafias óbvias, não são uma parede. No Codex, se o hook de guarda falhar ao rodar, o comando passa.

Para o executor não parar num diálogo que ninguém pode responder, a confiança na pasta do projeto é dada pela sessão: no Codex por configuração da linha de comando, no Claude pela variável `CLAUDE_CODE_SANDBOXED=1`, que não é documentada e pode mudar entre versões do Claude Code. Escolher a pasta no app equivale a confiar nela.

Na inicialização de uma sessão, regras e pastas de sessões mortas são removidas quando o nome, a primeira linha e o dono encerrado comprovam a origem; o que não puder ser verificado é preservado.

## 8. Arquivos e testes

| Caminho | Papel |
| --- | --- |
| `src/` | Frontend: canvas, blocos, terminais (xterm), seletor. `mock.ts` é um backend simulado, só em `npm run dev` fora do Tauri. |
| `src-tauri/src/` | Backend: PTYs, sessão e travas do orquestrador, lotes, executores, configuração, `delegate.ps1`. |
| `strategist.md` | Instruções enviadas ao orquestrador. |
| `executor.md` | Instruções enviadas a cada executor, antes da tarefa. |

```powershell
cd src-tauri
cargo test
```

Os testes usam CLIs simulados e não chamam nenhum modelo. `tests\real.rs` faz o ciclo com os CLIs reais e fica ignorado por padrão: defina `ORQ_REAL_REPO` e `ORQ_REAL=codex,claude` e rode `cargo test --test real -- --ignored --nocapture`.

## 9. Launcher antigo

`strategist.cmd`, `strategist.ps1`, `strategist.config.ps1`, `strategist.onboarding.html` e os `strategist.tests*.ps1` são o launcher de terminal que este app substitui. Ficam no repositório até o app ser validado no uso real. O `strategist.md` já descreve lotes, que o launcher antigo não aceita.

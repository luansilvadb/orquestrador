# Orquestrador

## 1. O que é

Um launcher para Windows que põe dois CLIs de IA para trabalhar em papéis separados sobre um repositório Git. O estrategista (orquestrador) só lê o projeto e delega: o único comando que ele pode rodar é o launcher de delegação da sessão. O executor recebe uma tarefa por vez e opera o repositório. A cada delegação, um retrato Git tirado antes e depois mostra o que mudou no working tree, no índice e no `HEAD`. As ferramentas `claude` e `codex` podem ocupar qualquer um dos dois papéis.

## 2. Requisitos

- Windows.
- Windows PowerShell 5.1.
- Git.
- Os CLIs `claude` e `codex` no `PATH`, somente os que forem usados nos papéis escolhidos.

## 3. Como iniciar

Abra um terminal dentro do repositório Git do projeto alvo e execute o `strategist.cmd` pelo caminho da pasta onde este projeto está (ou ponha essa pasta no `PATH`):

```powershell
C:\caminho\para\orquestrador\strategist.cmd
```

O projeto usado é sempre o repositório Git que contém a pasta atual do terminal. O caminho do projeto não é salvo na configuração. Se a pasta atual não estiver dentro de um repositório Git, o início é interrompido.

Toda inicialização abre no navegador uma página de onboarding, servida por um servidor HTTP local em `127.0.0.1`, numa porta livre. O terminal imprime o endereço e fica aguardando:

- A página mostra o projeto e, para o orquestrador e para o executor, a ferramenta, o modelo e o esforço, com as escolhas salvas já selecionadas.
- `Salvar e iniciar` envia as escolhas. Se forem válidas, o servidor encerra e a orquestração começa no mesmo terminal; se não forem, os erros aparecem na página e nada inicia.
- `Ctrl+C` no terminal cancela o início sem gravar nada. Fechar a aba não cancela: o terminal continua aguardando no mesmo endereço.

As escolhas são salvas como padrão para a próxima inicialização.

O endereço contém um token aleatório, gerado a cada inicialização, sem o qual o servidor não responde. Com a variável de ambiente `ORQUESTRADOR_SEM_NAVEGADOR` definida, o navegador não é aberto e o endereço é apenas impresso.

## 4. Como funciona uma delegação

O orquestrador recebe um único comando permitido por sessão: o caminho para `delegate.ps1` dentro da pasta temporária da sessão, seguido de uma tarefa como um único argumento entre aspas simples. A linha inteira deve ser esse comando: sem `&` antes, sem pipe, sem `;` e sem outro comando junto.

A pasta da sessão é criada na pasta temporária do sistema, com o nome `strategist-` seguido de um identificador, e é removida quando a sessão termina. O caminho completo do launcher é entregue ao orquestrador nas instruções dele. A chamada tem este formato:

```powershell
<pasta da sessão>\delegate.ps1 'Remova o problema local X.
Escopo: Y. Preserve: Z. Pronto quando: W.'
```

O launcher encaminha a tarefa ao executor por entrada padrão. A tarefa pode ocupar várias linhas e conter aspas duplas; um apóstrofo dentro dela deve ser duplicado, como em `d''água`.

Antes e depois da execução, o wrapper obtém um retrato do repositório:

- Caminho e SHA-256 de cada arquivo rastreado, inclusive os ignorados.
- Caminho e SHA-256 dos arquivos não rastreados retornados por `git ls-files --others --exclude-standard`.
- Estado do índice Git: modo, objeto e estágio de cada entrada.
- Valor de `HEAD`.

O relatório completo fica entre os marcadores `--- EVIDENCIA GIT ---` e `--- FIM ---`. Os nomes e valores são:

- `delegation`: número da delegação na sessão.
- `unresolved_attempts`: tentativas anteriores e atual da mesma tarefa sobre o mesmo estado que não terminaram com validação aprovada.
- `executor_exit`: código de saída do executor, impresso após dois-pontos.
- `executor_started`: `yes` quando o executor foi chamado no relatório completo.
- `snapshot_status`: `complete` ou `incomplete`, junto da linha de auditoria.
- `working_tree_changes`: `A` novo rastreado; `??` novo não rastreado; `M` modificado; `D` removido; `I` ainda no disco, mas deixou de ser rastreado e é ignorado.
- `index_changes`: `A` entrou no stage; `M` entrada mudou em conteúdo, modo ou estágio; `D` saiu do stage.
- `head_changes`: mudança de `HEAD`, impressa como valor anterior e novo, ou `(nada)`.
- `validation_status`: `passed`, `failed` ou `incomplete`.

`validation_status=passed` significa que o executor terminou com código zero e a auditoria posterior foi concluída. `failed` significa que o executor terminou com código diferente de zero. `incomplete` indica que a auditoria posterior não pôde ser concluída ou que o wrapper parou antes do relatório completo. Esse campo não prova que os critérios da tarefa foram atendidos.

Há três resultados possíveis ao encerrar:

- `--- FIM ---`: relatório completo. O código de saída é o do executor; se ele saiu com zero e a auditoria posterior falhou, o wrapper retorna `2`.
- `--- FIM SEM RELATORIO ---`: falha do wrapper, com `wrapper_error:` e código de saída `4`. A saída informa `executor_started=no` ou `executor_started=yes`; `no` prova que o executor não foi chamado, enquanto `yes` significa que seus efeitos são desconhecidos.
- Sem nenhuma dessas linhas: a execução foi interrompida. O que o executor fez é desconhecido.

O freio de estagnação recusa a terceira tentativa da mesma tarefa sobre o mesmo estado do repositório na sessão quando não houve `validation_status=passed`. A combinação compara a tarefa, `HEAD`, conteúdo e índice. A contagem inclui tentativas anteriores dessa mesma combinação, mesmo que outras delegações tenham ocorrido no meio. A recusa ocorre antes da chamada ao executor e aparece em `unresolved_attempts`. Uma tarefa ou estado diferente forma outra combinação; voltar a uma combinação anterior ainda considera as tentativas dela sem validação aprovada. Uma combinação diferente, por si só, não prova progresso.

## 5. Configuração

A configuração fica em `%LOCALAPPDATA%\orquestrador\config.json`. O projeto não é um campo desse arquivo. O formato é:

```json
{
  "orchestrator": {
    "tool": "codex",
    "model": "",
    "effort": ""
  },
  "executor": {
    "tool": "claude",
    "model": "",
    "effort": ""
  }
}
```

Os padrões são orquestrador `codex` e executor `claude`. Modelo e esforço vazios deixam o CLI usar seus padrões. Cada papel aceita `claude` ou `codex`.

Esforços aceitos pelo arquivo de configuração:

- Claude: `low`, `medium`, `high`, `xhigh` e `max`.
- Codex: `low`, `medium`, `high`, `xhigh`, `max` e `ultra`.

Alguns modelos aceitam apenas parte dos esforços listados. O nome do modelo deve corresponder a `^[A-Za-z0-9][A-Za-z0-9._:/@+\[\]-]*$`: começar com letra ou dígito e usar somente letras, dígitos, `. _ : / @ + [ ] -`. Não pode começar com hífen, nem conter espaço ou aspas.

Um JSON ilegível ou valor inválido impede o início; não há fallback silencioso para os padrões. Ao iniciar, o launcher (`strategist.ps1`) valida a configuração e salva as escolhas da página de onboarding. A sessão recebe uma cópia congelada em `session-config.json`; todas as delegações daquela sessão usam essa cópia, não a configuração global.

## 6. Limites de segurança

As instruções de `executor.md` pedem para não fazer deploy, publicação, operação destrutiva ou irreversível, commit, stage ou reprodução de segredos. Também pedem que o executor pare e relate se uma operação proibida for necessária.

**Orquestrador Codex.** É iniciado com `--sandbox read-only` e `--ask-for-approval never`. Uma regra `prefix_rule` no diretório de regras do Codex libera o launcher daquela sessão. A regra é removida no encerramento normal. O sandbox impede escrita pelo orquestrador, mas ele ainda pode ler o disco inteiro.

**Orquestrador Claude.** É iniciado com `--restricted`, `--permission-mode dontAsk` e `--strict-mcp-config`, sem servidor MCP carregado. As ferramentas liberadas são `Read`, `Glob`, `Grep` e `PowerShell`; a configuração permite ao PowerShell chamar o launcher da sessão. O modo restrito confina `Read` ao projeto.

**Executor Claude.** É iniciado com `--permission-mode auto` e `--permission-prompts none`. A lista de comandos negados é aplicada a `Bash` e `PowerShell`: `git push*`, `git reset --hard*`, `git clean*`, `npm publish*` e `pnpm publish*`.

**Executor Codex.** É iniciado com `--sandbox workspace-write` e `--ignore-rules`. O prompt da tarefa chega pela entrada padrão.

Filtros de comando e prompts orientam e não são barreira de segurança. A regra do orquestrador libera o launcher, mas não limita o que o executor faz depois. A lista de comandos negados cobre apenas as grafias indicadas.

Limites do retrato impresso no relatório:

- Compara caminho e SHA-256 antes e depois; não prova autoria, pois outro processo pode ter escrito no intervalo.
- Não vê arquivos não rastreados ignorados.
- Submódulo ou repositório aninhado conta como uma entrada.
- Link simbólico é comparado pelo destino e não é seguido.
- Pasta que é junção é lida por dentro, como o Git a lista.

Na inicialização, o código procura regras antigas de sessões Codex. Se a pasta da sessão já não existe, remove a regra correspondente quando o nome e o conteúdo dela são os esperados. Se a pasta existe, também exige que o proprietário seja comprovadamente encerrado; então remove a regra e tenta limpar a sessão. Se a regra ou o proprietário não puderem ser verificados, são preservados. Uma sessão Claude morta pode deixar sua pasta temporária; a recuperação de regras Codex não a encontra.

## 7. Arquivos e testes

| Arquivo na raiz | Papel |
| --- | --- |
| `README.md` | Guia de uso e referência do projeto. |
| `strategist.cmd` | Entrada para iniciar `strategist.ps1` no Windows PowerShell. |
| `strategist.ps1` | Início, sessão temporária, permissões do orquestrador, delegação e retrato Git. |
| `strategist.config.ps1` | Padrões, leitura, validação e gravação da configuração; argumentos dos CLIs; servidor HTTP do onboarding. |
| `strategist.onboarding.html` | Página de onboarding servida no início. |
| `strategist.md` | Instruções enviadas ao orquestrador. |
| `executor.md` | Instruções enviadas ao executor em cada delegação. |
| `strategist.tests.ps1` | Verificações determinísticas do fluxo de sessão e delegação. |
| `strategist.config.tests.ps1` | Verificações da configuração, da página de onboarding e da integração. |
| `strategist.tests.common.ps1` | Preparação compartilhada dos CLIs simulados, repositórios temporários e funções auxiliares dos testes. |

Os testes usam Claude e Codex simulados e repositórios Git descartáveis. Não chamam nenhum modelo.

`strategist.tests.ps1`:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File strategist.tests.ps1
```

`strategist.config.tests.ps1`:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File strategist.config.tests.ps1
```

### Para quem vai mexer no código

Os cabeçalhos dos arquivos `.ps1` pedem que permaneçam somente ASCII para leitura pelo Windows PowerShell 5.1 sem BOM. As instruções dos papéis ficam nos arquivos Markdown correspondentes.

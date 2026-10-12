# Orquestrador

## 1. O que é

Um launcher para Windows que põe dois CLIs de IA para trabalhar em papéis separados sobre um repositório Git. O estrategista (orquestrador) alinha o objetivo com o usuário, só lê o projeto e delega: os únicos comandos que ele pode rodar são o launcher de delegação da sessão e os programas das skills globais do usuário. O executor recebe uma tarefa por vez e opera o repositório. Cada delegação devolve a resposta do executor e a lista do `git status`, e o loop segue até não haver pendência. As ferramentas `claude` e `codex` podem ocupar qualquer um dos dois papéis.

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

Quando o executor termina, o wrapper imprime, depois da resposta dele:

- `executor_exit`: o código de saída do executor, com `(FALHOU)` quando não é zero.
- A saída de `git status --porcelain --untracked-files=all`: tudo o que está alterado no repositório naquele momento, ou `(nada)`. Não é a diferença da delegação: o que já estava pendente antes aparece junto.
- `--- FIM ---`: a linha que fecha toda saída completa. Sem ela, a execução foi interrompida e o que o executor fez é desconhecido.

O código de saída do comando é o do executor. Uma falha do próprio wrapper (tarefa vazia, por exemplo) imprime `wrapper_error:`, fecha com `--- FIM ---` e sai com `4`.

Não há trava automática contra repetição nem limite de delegações: quem percebe que o loop não avança e muda a abordagem, ou encerra, é o orquestrador, pelas instruções de `strategist.md`.

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

**Orquestrador Codex.** É iniciado com `--sandbox read-only` e `--ask-for-approval never`. Um arquivo de regras `prefix_rule` no diretório de regras do Codex libera o launcher daquela sessão e os programas das skills globais. O arquivo é removido no encerramento normal. O sandbox impede escrita pelo orquestrador, mas ele ainda pode ler o disco inteiro.

**Orquestrador Claude.** É iniciado com `--restricted`, `--permission-mode dontAsk` e `--strict-mcp-config`, sem servidor MCP carregado. As ferramentas liberadas são `Read`, `Glob`, `Grep`, `PowerShell` e `Skill`; a configuração permite ao PowerShell chamar o launcher da sessão e os programas das skills globais. O modo restrito confina `Read` ao projeto e às pastas `.agents` e `.claude` do usuário; como ele descarta as skills do usuário, a pasta `.agents` é carregada como plugin e as skills aparecem como `.agents:nome`. Comandos que o próprio Claude Code considera somente leitura, como `git status`, também passam.

**Skills do orquestrador.** As skills globais ficam em `%USERPROFILE%\.agents\skills` e `%USERPROFILE%\.claude\skills`. No início, cada programa `.mjs`, `.cjs`, `.js` (por `node`) e `.py` (por `python`) encontrado ali, fora de `node_modules`, é liberado pelo nome, nas duas grafias de barra. Uma skill instalada com a sessão aberta só é liberada na sessão seguinte. O que esses programas conseguem fazer, o orquestrador consegue fazer por meio deles: essas pastas são do usuário, e o que entra nelas é responsabilidade dele.

**Executor Claude.** É iniciado com `--permission-mode auto` e `--permission-prompts none`. A lista de comandos negados é aplicada a `Bash` e `PowerShell`: `git push*`, `git reset --hard*`, `git clean*`, `npm publish*` e `pnpm publish*`.

**Executor Codex.** É iniciado com `--sandbox workspace-write` e `--ignore-rules`. O prompt da tarefa chega pela entrada padrão.

Filtros de comando e prompts orientam e não são barreira de segurança. A regra do orquestrador libera o launcher, mas não limita o que o executor faz depois. A lista de comandos negados cobre apenas as grafias indicadas.

Limites da lista impressa em cada delegação:

- É o `git status` depois da execução; não separa o que a delegação mudou do que já estava pendente.
- Não mostra arquivos ignorados nem um commit feito pelo executor.

Na inicialização, o código procura regras antigas de sessões Codex. Se a pasta da sessão já não existe, remove a regra correspondente quando o nome e a primeira linha dela são os esperados. Se a pasta existe, também exige que o proprietário seja comprovadamente encerrado; então remove a regra e tenta limpar a sessão. Se a regra ou o proprietário não puderem ser verificados, são preservados. Uma sessão Claude morta pode deixar sua pasta temporária; a recuperação de regras Codex não a encontra.

## 7. Arquivos e testes

| Arquivo na raiz | Papel |
| --- | --- |
| `README.md` | Guia de uso e referência do projeto. |
| `strategist.cmd` | Entrada para iniciar `strategist.ps1` no Windows PowerShell. |
| `strategist.ps1` | Início, sessão temporária, permissões do orquestrador e delegação. |
| `strategist.config.ps1` | Padrões, leitura, validação e gravação da configuração; argumentos dos CLIs; servidor HTTP do onboarding. |
| `strategist.onboarding.html` | Página de onboarding servida no início. |
| `strategist.md` | Instruções enviadas ao orquestrador. |
| `executor.md` | Instruções enviadas ao executor em cada delegação. |
| `strategist.tests.ps1` | Verificações do essencial: as quatro duplas, a trava do orquestrador, a delegação, a página e a limpeza de sessões mortas. |
| `strategist.tests.common.ps1` | Preparação dos testes: CLIs simulados, repositórios temporários e funções auxiliares. |

Os testes usam Claude e Codex simulados e repositórios Git descartáveis. Não chamam nenhum modelo.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File strategist.tests.ps1
```

### Para quem vai mexer no código

Os cabeçalhos dos arquivos `.ps1` pedem que permaneçam somente ASCII para leitura pelo Windows PowerShell 5.1 sem BOM. As instruções dos papéis ficam nos arquivos Markdown correspondentes.

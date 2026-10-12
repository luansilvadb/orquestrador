# Design system

A interface usa [shadcn/ui](https://ui.shadcn.com) de verdade (base Radix, estilo `radix-vega`, base de cor `neutral`), com Tailwind CSS v4 pela integração do Vite. Tema escuro neutro; a classe `dark` fica em `index.html`.

## Onde fica

| Caminho | Papel |
| --- | --- |
| `components.json` | Configuração do registry do shadcn (aliases, estilo, base de cor). |
| `src/components/ui/` | Componentes gerados pelo CLI oficial (`npx shadcn@latest add ...`). Não editar à mão; regenerar pelo CLI. |
| `src/lib/utils.ts` | `cn`, gerado pelo CLI. |
| `src/index.css` | Tokens semânticos do shadcn (`:root` e `.dark`) e os poucos estilos próprios. |

Componentes em uso: `alert`, `alert-dialog`, `badge`, `button`, `card`, `input`, `label`, `popover` (escolha de branch no painel Git), `progress`, `select`, `sonner`, `tabs` (faixa de abas dos terminais, em `src/terminal-screen.tsx`), `textarea` (mensagem de commit), `toggle-group` (ferramenta e esforço do formulário de escolha) e `toggle`, que é dependência de `toggle-group` (o CLI a instala junto). Para adicionar um componente, use o CLI e liste-o aqui.

Componentes novos do painel Git e das abas, todos pelo CLI oficial (`npx shadcn@latest add tabs popover textarea`), sem dependência nova: `tabs`, `popover` e `textarea`. Os demais componentes do painel (botões, badge, input) já existiam.

## Regras

- Cores, raio, borda, foco e espaçamento vêm dos tokens semânticos (`bg-card`, `text-muted-foreground`, `border`, `ring`…). Sem cor fixa no componente.
- Os tokens próprios, em `src/index.css`, são: `--status-*`, as cores dos ícones de estado nas abas dos executores; `--terminal`, o fundo dos terminais (`bg-terminal`), com o mesmo valor de `theme.background` em `src/term.ts`, que o xterm recebe por lá e não pela variável; e as cores do diff (`--diff-add` e `--diff-del`). Os de diff valem como texto do sinal `+`/`-` e, com 15% de opacidade, como fundo da linha (`bg-diff-add/15`, `text-diff-del`). Têm valor próprio no tema escuro, com contraste sobre o card.
- Estado de execução sempre tem texto e ícone, não só cor (`Badge` com rótulo).
- CSS próprio só para o xterm e a base da página: `.xterm .xterm-viewport` (fundo transparente, para o `bg-terminal` do corpo aparecer) e as alturas e a rolagem de `html`, `body` e `#root`. Os terminais xterm são superfícies especializadas e não usam componentes shadcn.
- Monoespaçado só em terminal e valores técnicos (caminho, modelo, esforço).
- Sem glows, gradientes decorativos ou acento colorido nas bordas de todos os cards.

## Ajuste manual em `progress`

O `src/components/ui/progress.tsx` gerado pelo CLI desestruturava `value` e usava no transform do indicador, mas não o repassava ao `ProgressPrimitive.Root`. Resultado: `data-state` ficava sempre `indeterminate` e `aria-valuenow` sumia no download com total conhecido. Corrigido com `value={value}` no `Root`, antes do `{...props}`; o resto do componente segue o oficial. Ao regenerar o componente pelo CLI, reaplique essa linha.

## Motion

As animações de entrada e saída são as do shadcn oficial (`tw-animate-css` + variantes `data-open`/`data-closed` de `shadcn/tailwind.css`, importados em `src/index.css`). Verificado no navegador: `AlertDialog` anima overlay e conteúdo nos dois sentidos (`enter`/`exit`, 0,1s). O `Select` do formulário de escolha (`src/choice-form.tsx`) usa `position="popper"` com `align="start"`: é a variante animada do `radix-vega`, sem `animate-none`. O menu abre com `enter` e fecha com `exit`, 0,1s, com `fade-in-0`/`zoom-in-95` e `fade-out-0`/`zoom-out-95`, ancorado logo abaixo do gatilho e com a largura dele.

## Telas e terminais montados

São duas telas, com a rota em `src/route.ts`: a principal (`#/arquivos`, o padrão), com a árvore de pastas à esquerda (faixa fixa de 280px) e os terminais à direita; e a do Git (`#/git`). Ambas ficam montadas o tempo todo, em `src/App.tsx`. A rota só escolhe qual aparece; a inativa vai para `invisible`, nunca é desmontada. Na tela principal vale o mesmo para os terminais: todos ficam montados e só o da aba ativa aparece. O motivo: o xterm recalcula colunas e linhas quando sai do layout ou volta, e manda o tamanho errado ao PTY se isso acontecer.

Tela cheia só troca classes no conjunto que já existe; nada é remontado.

# Design system

A interface usa o [Lobe UI](https://github.com/lobehub/lobe-ui) (`@lobehub/ui`, sobre Base UI e antd-style), com Tailwind CSS v4 pela integração do Vite. Tema escuro fixo, definido pelo `ThemeProvider` em `src/main.tsx`. Fonte: Inter.

## Onde fica

| Caminho | Papel |
| --- | --- |
| `src/main.tsx` | `ThemeProvider` (tema escuro, fonte Inter no antd), um `ConfigProvider` do `@lobehub/ui` com `motion` (de `motion/react`) dentro dele, `ToastHost` e `ModalHost`. |
| `src/index.css` | Tailwind, fonte Inter, os tokens do tema escuro copiados do Lobe UI, os tokens próprios do app e os poucos estilos globais. |
| `src/term.ts` | Tema dos terminais xterm (cores em `theme`). |

Componentes em uso, todos de `@lobehub/ui/base-ui` (o `Block` vem de `@lobehub/ui`):

| Componente | Onde |
| --- | --- |
| `Block` | `src/App.tsx` (cartões da tela de atualização e de escolha de pasta). |
| `Alert` | `src/App.tsx`, `src/choice-form.tsx`. |
| `Badge` | `src/explorer.tsx` (contador de alterações no ícone do Git). |
| `Button` | `src/App.tsx`, `src/choice-form.tsx`, `src/git.tsx`, `src/terminal-screen.tsx`. |
| `ActionIcon` | `src/explorer.tsx`, `src/git.tsx`. |
| `Input`, `TextArea` | `src/choice-form.tsx`, `src/git.tsx` (nome de branch e mensagem de commit). |
| `InputNumber` | `src/App.tsx` (limite de executores ao mesmo tempo). |
| `Segmented`, `Select` | `src/choice-form.tsx`. |
| `Modal` | `src/App.tsx` (confirmação de fechar com terminais trabalhando), `src/choice-form.tsx`. |
| `Popover` | `src/git.tsx` (escolha de branch). |
| `Progress` | `src/App.tsx` (download com total conhecido). |
| `ScrollArea` | `src/explorer.tsx` (árvore de pastas). |
| `Tag` | `src/terminal-screen.tsx` (estado de cada executor). |
| `TabsRoot`, `TabsList`, `TabsTab`, `TabsIndicator` | `src/terminal-screen.tsx` (faixa de abas dos terminais). |
| `toast` | `src/App.tsx`, `src/git.tsx`. |

Para trocar ou adicionar um componente, importe-o de `@lobehub/ui/base-ui` e atualize esta tabela.

## Regras

- Componentes vêm do Lobe UI. Tailwind fica só para layout, espaçamento e tipografia. Cores de texto e de fundo secundárias usam os tokens do Tailwind (`bg-card`, `text-muted-foreground`, `border`, `ring`…), sem cor fixa no componente.
- Os tokens do Tailwind em `src/index.css` copiam os valores do tema escuro do Lobe UI (`colorBgLayout`, `colorBgContainer`, `colorTextSecondary` etc., comentados ao lado de cada um). Não há CSS variables do tema em runtime, então se o tema mudar, os valores precisam ser atualizados à mão.
- Os tokens próprios, em `src/index.css`, são: `--status-*`, as cores dos ícones de estado nas abas dos executores; `--terminal`, o fundo dos terminais (`bg-terminal`), com o mesmo valor de `theme.background` em `src/term.ts`, que o xterm recebe por lá e não pela variável; e as cores do diff (`--diff-add` e `--diff-del`). Os de diff valem como texto do sinal `+`/`-` e, com 15% de opacidade, como fundo da linha (`bg-diff-add/15`, `text-diff-del`). Têm valor próprio no tema escuro, com contraste sobre o card.
- Estado de execução sempre tem texto e ícone, não só cor (`Tag` com rótulo e ícone).
- A cor do `Tag` de estado vem do token `--status-*` por `style`, porque a prop `color` do `Tag` só aceita hex ou cor predefinida.
- A barra de progresso indeterminada (download sem total conhecido) é feita com Tailwind (`animate-pulse`), porque o `Progress` do Lobe UI não tem esse modo.
- Toasts pelo `toast` imperativo; o `ToastHost` em `src/main.tsx` é quem os renderiza.
- CSS próprio só para o xterm e a base da página: `.xterm .xterm-viewport` (fundo transparente, para o `bg-terminal` do corpo aparecer) e as alturas e a rolagem de `html`, `body` e `#root`. Os terminais xterm são superfícies especializadas e não usam componentes do Lobe UI.
- Monoespaçado só em terminal e valores técnicos (caminho, modelo, esforço).
- Sem glows, gradientes decorativos ou acento colorido nas bordas de todos os cards.
- O `ConfigProvider` com `motion` (de `motion/react`) é obrigatório dentro do `ThemeProvider`: sem ele, os componentes de `@lobehub/ui/base-ui` lançam erro ao renderizar. Não tire esse par de `src/main.tsx`.
- Classe do Tailwind posta num componente do Lobe UI perde quando o componente define a mesma propriedade, porque o CSS do Lobe UI não fica em camada e as utilitárias do Tailwind ficam. Nesses casos use a prop do componente (`size`, `style`, `classNames`, `styles`) ou o modificador `!` do Tailwind, como em `className="w-16! font-mono!"` no `InputNumber` de `src/App.tsx`.

## Telas e terminais montados

São duas telas, com a rota em `src/route.ts`: a principal (`#/arquivos`, o padrão), com a árvore de pastas à esquerda (faixa fixa de 280px) e os terminais à direita; e a do Git (`#/git`). Ambas ficam montadas o tempo todo, em `src/App.tsx`. A rota só escolhe qual aparece; a inativa vai para `invisible`, nunca é desmontada. Na tela principal vale o mesmo para os terminais: todos ficam montados e só o da aba ativa aparece. O motivo: o xterm recalcula colunas e linhas quando sai do layout ou volta, e manda o tamanho errado ao PTY se isso acontecer.

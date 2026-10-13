# Design system

A interface usa o [Lobe UI](https://github.com/lobehub/lobe-ui) (`@lobehub/ui`, sobre Base UI e antd-style), com Tailwind CSS v4 pela integração do Vite. Tema escuro fixo, definido pelo `ThemeProvider` em `src/main.tsx`. Fonte: Inter.

## Onde fica

| Caminho | Papel |
| --- | --- |
| `src/main.tsx` | `ThemeProvider` (tema escuro, fonte Inter no antd), um `ConfigProvider` do `@lobehub/ui` com `motion` (de `motion/react`) dentro dele, `ToastHost` e `ModalHost`. |
| `src/index.css` | Ordem da camada `lobe-base`, Tailwind, fonte Inter, os tokens do Tailwind ligados às variáveis `--ant-*` do antd, o token do fundo dos terminais e os poucos estilos globais. |
| `src/term.ts` | Tema dos terminais xterm (cores em `theme`). |

Componentes em uso. Os de `@lobehub/ui` são `Block`, `Center`, `Flexbox` e `PatchDiff`; os demais vêm de `@lobehub/ui/base-ui`:

| Componente | Onde |
| --- | --- |
| `Alert` | `src/App.tsx`, `src/choice-form.tsx`. |
| `Block` | `src/App.tsx` (cartões da tela de atualização e de escolha de pasta). |
| `Button` | `src/App.tsx`, `src/choice-form.tsx`, `src/git.tsx`, `src/terminal-screen.tsx`. |
| `Center` | `src/App.tsx`, `src/terminal-screen.tsx` (centralização de conteúdo). |
| `Flexbox` | `src/titlebar.tsx`, `src/explorer.tsx`, `src/choice-form.tsx`, `src/terminal-screen.tsx`, `src/git.tsx`, `src/App.tsx` (layout em coluna e em linha). |
| `ActionIcon` | `src/explorer.tsx`, `src/git.tsx`. |
| `Badge` | `src/explorer.tsx` (contador de alterações no ícone do Git). |
| `DraggablePanel` | `src/git.tsx` (painel lateral da tela do Git, redimensionável). |
| `Input` | `src/choice-form.tsx`, `src/git.tsx` (nome de branch). |
| `TextArea` | `src/git.tsx` (mensagem de commit). |
| `InputNumber` | `src/App.tsx` (limite de executores ao mesmo tempo). |
| `Modal` | `src/App.tsx` (confirmação de fechar com terminais trabalhando), `src/choice-form.tsx`. |
| `Popover` | `src/git.tsx` (escolha de branch). |
| `PatchDiff` | `src/git.tsx` (diff da alteração selecionada). |
| `Progress` | `src/App.tsx` (download com total conhecido e download sem total, indeterminado). |
| `ScrollArea` | `src/explorer.tsx` (árvore de pastas). |
| `Segmented`, `Select` | `src/choice-form.tsx`. |
| `Tag` | `src/terminal-screen.tsx` (estado de cada executor). |
| `TabsRoot`, `TabsList`, `TabsTab`, `TabsIndicator` | `src/terminal-screen.tsx` (faixa de abas dos terminais). |
| `Text` | `src/titlebar.tsx`, `src/explorer.tsx`, `src/choice-form.tsx`, `src/terminal-screen.tsx`, `src/git.tsx`, `src/App.tsx`. |
| `Tree` | `src/explorer.tsx` (árvore de pastas). |
| `toast` | `src/App.tsx`, `src/git.tsx`. |

Para trocar ou adicionar um componente, importe-o de `@lobehub/ui/base-ui` (ou de `@lobehub/ui`, para `Block`, `Center`, `Flexbox` e `PatchDiff`) e atualize esta tabela.

## Regras

- Componentes vêm do Lobe UI. Antes de qualquer classe, use as props do componente: texto usa `Text` (`code`, `type="secondary"`, `fontSize`, `weight`, `ellipsis`…), e layout flex usa `Flexbox` ou `Center`.
- Em `Flexbox` e `Center`, direção (`horizontal`), `flex`, tamanho (`width`, `height`), `padding`, `paddingInline`, `paddingBlock`, `gap`, `align`, `justify` e `wrap` vão por prop, não por classe.
- Tailwind fica para o que o Lobe UI não cobre: posicionamento (`absolute`, `inset-0`, `m-auto`), borda e fundo (`border`, `bg-card`, `rounded-*`), overflow, `min-h-0`/`min-w-0`, espaçamento de elementos sem componente e tamanho de ícone (`size-3.5`).
- Cores de texto e de fundo secundárias usam os tokens do Tailwind (`bg-card`, `text-muted-foreground`, `border`, `ring`…), sem cor fixa no componente.
- Os tokens do Tailwind em `src/index.css` são ligados às variáveis `--ant-*` do antd, sem copiar valores: `--color-background` vem de `--ant-color-bg-layout`, `--color-card` de `--ant-color-bg-container`, `--color-foreground` de `--ant-color-text`, `--color-muted-foreground` de `--ant-color-text-secondary` etc., e os `--radius-*` partem de `--ant-border-radius`. O antd escreve essas variáveis na classe `css-var-<key>` do `div.ant-app`, que o `ThemeProvider` põe em volta de todo o conteúdo, então elas valem dentro do app. Fora dele (`html` e `body`) não valem; o fundo do `body` vem do `GlobalStyle` do Lobe UI. Se o tema mudar, os valores acompanham sozinhos. Para conferir a ligação em runtime, renderize a árvore e procure as variáveis na classe do `div.ant-app`.
- O único token próprio do app é `--terminal`, o fundo dos terminais (`bg-terminal`), com o mesmo valor de `theme.background` em `src/term.ts`, que o xterm recebe por lá e não pela variável.
- Estado de execução sempre tem texto e ícone, não só cor (`Tag` com rótulo e ícone). Exceção: em execução, o cabeçalho do terminal não mostra `Tag`, porque a aba já mostra o ícone girando; o rótulo fica no `title` e no `aria-label` da aba.
- A cor do `Tag` de estado é uma cor predefinida do Lobe UI, passada pela prop `color` (tipo `PresetColorKey`, na tabela `STATES` de `src/terminal-screen.tsx`). A cor do ícone da aba usa `colorsPreset` por `style`.
- A barra indeterminada (download sem total conhecido) é o `Progress` com `status="active"`, no código com `percent={100}`.
- Diff usa o `PatchDiff`; o app não define cores de diff.
- Toasts pelo `toast` imperativo; o `ToastHost` em `src/main.tsx` é quem os renderiza.
- CSS próprio em `src/index.css` só para o que o Lobe UI não cobre: a ordem da camada `lobe-base`, declarada no topo antes do `@import` do Tailwind, entre `base` e `components` (`@layer theme, base, lobe-base, components, utilities`), para o Flexbox do Lobe UI vencer o preflight do Tailwind (que zera `padding`) e as utilitárias do Tailwind vencerem o Flexbox; `.xterm .xterm-viewport` (fundo transparente, para o `bg-terminal` do corpo aparecer); e as alturas e a rolagem de `html`, `body` e `#root`. Os terminais xterm são superfícies especializadas e não usam componentes do Lobe UI.
- Monoespaçado só em terminal e valores técnicos (caminho, modelo, esforço).
- Sem glows, gradientes decorativos ou acento colorido nas bordas de todos os cards.
- O `ConfigProvider` com `motion` (de `motion/react`) é obrigatório dentro do `ThemeProvider`: sem ele, os componentes de `@lobehub/ui/base-ui` lançam erro ao renderizar. Não tire esse par de `src/main.tsx`.
- Se uma classe do Tailwind não ganhar de um componente do Lobe UI, use a prop do componente (`size`, `style`, `classNames`, `styles`) ou o modificador `!` do Tailwind, como em `className="w-16! font-mono!"` no `InputNumber` de `src/App.tsx`.

## Telas e terminais montados

São duas telas, com a rota em `src/route.ts`: a principal (`#/arquivos`, o padrão), com a árvore de pastas à esquerda (faixa fixa de 280px) e os terminais à direita; e a do Git (`#/git`). Ambas ficam montadas o tempo todo, em `src/App.tsx`. A rota só escolhe qual aparece; a inativa vai para `invisible`, nunca é desmontada. Na tela principal vale o mesmo para os terminais: todos ficam montados e só o da aba ativa aparece. O motivo: o xterm recalcula colunas e linhas quando sai do layout ou volta, e manda o tamanho errado ao PTY se isso acontecer.

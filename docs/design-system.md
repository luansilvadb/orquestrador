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
| `Button` | `src/App.tsx`, `src/choice-form.tsx`, `src/explorer.tsx`, `src/git.tsx`, `src/terminal-screen.tsx`. |
| `Center` | `src/App.tsx`, `src/terminal-screen.tsx` (centralização de conteúdo). |
| `Flexbox` | `src/titlebar.tsx`, `src/explorer.tsx`, `src/choice-form.tsx`, `src/terminal-screen.tsx`, `src/file-view.tsx`, `src/git.tsx`, `src/App.tsx`, `src/shell-panel.tsx`, `src/search.tsx` (layout em coluna e em linha). |
| `ActionIcon` | `src/explorer.tsx`, `src/file-view.tsx`, `src/git.tsx`, `src/shell-panel.tsx`, `src/search.tsx` (opções, atualizar, limpar e recolher; no painel Terminal, fechar a aba, novo terminal e esconder o painel; na área do arquivo, salvar, recarregar e fechar). |
| `Badge` | `src/explorer.tsx` (contador de alterações no ícone do Git). |
| `DraggablePanel` | `src/git.tsx` (painel lateral da tela do Git, redimensionável), `src/shell-panel.tsx` (painel Terminal, redimensionável pela borda de cima), `src/terminal-screen.tsx` (painel do Orquestrador, redimensionável pela borda esquerda), `src/App.tsx` (faixa da árvore e da pesquisa, redimensionável pela borda direita). |
| `Input` | `src/choice-form.tsx`, `src/git.tsx` (nome de branch), `src/search.tsx` (campo da pesquisa, com as opções no `suffix`). |
| `TextArea` | `src/git.tsx` (mensagem de commit). |
| `InputNumber` | `src/App.tsx` (limite de executores ao mesmo tempo). |
| `Modal` | `src/App.tsx` (confirmação de fechar com terminais trabalhando), `src/choice-form.tsx`. |
| `Popover` | `src/git.tsx` (escolha de branch). |
| `PatchDiff` | `src/git.tsx` (diff da alteração selecionada). |
| `Progress` | `src/App.tsx` (download com total conhecido e download sem total, indeterminado). |
| `ScrollArea` | `src/explorer.tsx` (árvore de pastas), `src/search.tsx` (lista de resultados). |
| `Segmented`, `Select` | `src/choice-form.tsx`. |
| `Tag` | `src/terminal-screen.tsx` (estado de cada executor). |
| `TabsRoot`, `TabsList`, `TabsTab`, `TabsIndicator` | `src/terminal-screen.tsx` (faixa de abas do painel do Orquestrador), `src/shell-panel.tsx` (faixa de abas dos shells). |
| `Text` | `src/titlebar.tsx`, `src/explorer.tsx`, `src/choice-form.tsx`, `src/terminal-screen.tsx`, `src/file-view.tsx`, `src/git.tsx`, `src/App.tsx`, `src/shell-panel.tsx`, `src/search.tsx`. |
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
- Estado de execução sempre tem texto e ícone, não só cor (`Tag` com rótulo e ícone). Exceção: em execução, a barra da aba ativa do painel do Orquestrador não mostra `Tag`, porque a aba já mostra o ícone girando; o rótulo fica no `title` e no `aria-label` da aba.
- A cor do `Tag` de estado é uma cor predefinida do Lobe UI, passada pela prop `color` (tipo `PresetColorKey`, na tabela `STATES` de `src/terminal-screen.tsx`). A cor do ícone da aba usa `colorsPreset` por `style`.
- A barra indeterminada (download sem total conhecido) é o `Progress` com `status="active"`, no código com `percent={100}`.
- Diff usa o `PatchDiff`; o app não define cores de diff.
- Conteúdo de arquivo (a área do arquivo, `src/file-view.tsx`) usa CodeMirror 6, com linguagem carregada pela extensão sob demanda até 200 mil caracteres. O editor não quebra linhas e rola dentro da área do arquivo; arquivos não UTF-8 ficam somente leitura.
- Toasts pelo `toast` imperativo; o `ToastHost` em `src/main.tsx` é quem os renderiza.
- CSS próprio em `src/index.css` só para o que o Lobe UI não cobre: a ordem da camada `lobe-base`, declarada no topo antes do `@import` do Tailwind, entre `base` e `components` (`@layer theme, base, lobe-base, components, utilities`), para o Flexbox do Lobe UI vencer o preflight do Tailwind (que zera `padding`) e as utilitárias do Tailwind vencerem o Flexbox; `.xterm .xterm-viewport` (fundo transparente, para o `bg-terminal` do corpo aparecer); as alturas de `html`, `body` e `#root`; e `html body { overflow: hidden }`, que impede a janela de rolar. O GlobalStyle do Lobe UI põe `body { overflow: hidden auto }` sem camada, depois deste arquivo, e um `body` simples perderia para ele (mesma especificidade, o último vence); `html body` tem mais especificidade e vence. Os terminais xterm são superfícies especializadas e não usam componentes do Lobe UI.
- Monoespaçado só em terminal e valores técnicos (caminho, modelo, esforço, número, trecho de linha da pesquisa e conteúdo de arquivo).
- Sem glows, gradientes decorativos ou acento colorido nas bordas de todos os cards.
- O `ConfigProvider` com `motion` (de `motion/react`) é obrigatório dentro do `ThemeProvider`: sem ele, os componentes de `@lobehub/ui/base-ui` lançam erro ao renderizar. Não tire esse par de `src/main.tsx`.
- Se uma classe do Tailwind não ganhar de um componente do Lobe UI, use a prop do componente (`size`, `style`, `classNames`, `styles`) ou o modificador `!` do Tailwind, como em `className="w-16! font-mono!"` no `InputNumber` de `src/App.tsx`. O mesmo vale para regras globais sem camada: o GlobalStyle do Lobe UI põe `* { scrollbar-width: thin }` sem camada, e uma utilitária do Tailwind, que fica em camada, perde para ela. Por isso as faixas de abas do painel do Orquestrador (`src/terminal-screen.tsx`) e do painel Terminal (`src/shell-panel.tsx`) escondem a barra de rolagem com `[scrollbar-width:none]!`. Sem barra visível, é a roda do mouse em cima da faixa que rola as abas, pelo `onWheel` dos dois arquivos.

## Telas e terminais montados

São três telas, com a rota em `src/App.tsx` (um estado, com o tipo `Route` em `src/explorer.tsx`): Arquivos, o padrão; Pesquisar; e Git. Todas ficam montadas o tempo todo, em `src/App.tsx`, e a rota só escolhe qual aparece; nada é desmontado e nada usa `display: none`, com a exceção do painel Terminal fechado, explicada abaixo. Arquivos e Pesquisar dividem a região do meio. A faixa redimensionável à esquerda tem a árvore e a pesquisa sobrepostas (`absolute inset-0`), e a inativa vai para `invisible`. Ao lado, a coluna central tem a área do arquivo em cima e o painel Terminal embaixo. A tela Git ocupa a região do meio inteira, também com `invisible` quando inativa. O painel do Orquestrador fica fora dessa troca: é montado com a tela principal, à direita, e nenhuma rota o desmonta nem muda o tamanho dele.

O motivo: o xterm recalcula colunas e linhas quando sai do layout ou volta, e manda o tamanho errado ao PTY se isso acontecer. Por isso a troca entre Arquivos e Pesquisar não pode desmontar a coluna central nem mudar o tamanho dela, e a tela Git não pode desmontar o painel do Orquestrador. Os terminais desse painel seguem a mesma regra: todos ficam montados, e só o da aba ativa aparece. O painel Terminal (`src/shell-panel.tsx`) também: as abas trocam com `invisible` e todos os `TermHost` ficam montados.

A exceção é o próprio painel Terminal escondido: ele sai do layout com `hidden` (display none), para a área do arquivo crescer. Ao abrir de novo, o terminal da aba ativa recalcula o tamanho (`fit`) antes de ganhar o foco, no efeito que segue `shellOpen` e `shellActive` em `src/App.tsx`.

A área do arquivo não tem terminal dentro. Ela é montada ao abrir um arquivo e desmontada ao fechá-lo, e sem arquivo aberto fica vazia.

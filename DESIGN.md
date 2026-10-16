---
name: Orquestrador
description: Bancada escura para orquestrar CLIs de IA sobre a pasta de um projeto.
colors:
  ink-white: "#eeeeee"
  ink-white-hover: "#ffffff"
  ink-white-active: "#cccccc"
  pitch-black: "#000000"
  panel-black: "#0d0d0d"
  raised-black: "#1a1a1a"
  terminal-black: "#0b0c0d"
  text-primary: "#ffffff"
  text-secondary: "#aaaaaa"
  text-tertiary: "#6f6f6f"
  hairline: "#202020"
  hairline-soft: "#1a1a1a"
  fill-muted: "rgba(255, 255, 255, 0.06)"
  fill-hover: "rgba(255, 255, 255, 0.10)"
  signal-error: "#f4416c"
  signal-success: "#c4f042"
  signal-warning: "#ffb224"
  signal-info: "#60b1ff"
  focus-blue: "#007fd4"
  close-red: "#e81123"
typography:
  headline:
    fontFamily: "Inter Variable, Inter, sans-serif"
    fontSize: "20px"
    fontWeight: 600
  title:
    fontFamily: "Inter Variable, Inter, sans-serif"
    fontSize: "16px"
    fontWeight: 600
  body:
    fontFamily: "Inter Variable, Inter, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.5714
  label:
    fontFamily: "Inter Variable, Inter, sans-serif"
    fontSize: "12px"
    fontWeight: 400
  menu:
    fontFamily: "Inter Variable, Inter, sans-serif"
    fontSize: "13px"
    lineHeight: "26px"
  mono:
    fontFamily: "Cascadia Mono, Cascadia Code, Consolas, Courier New, monospace"
    fontSize: "13px"
    lineHeight: 1.15
rounded:
  xs: "4px"
  menu: "5px"
  sm: "6px"
  md: "8px"
  lg: "12px"
spacing:
  row: "24px"
  menu-item: "26px"
  strip: "36px"
  nav: "44px"
components:
  button-fill:
    backgroundColor: "{colors.ink-white}"
    textColor: "{colors.pitch-black}"
    rounded: "{rounded.sm}"
    height: "36px"
  button-fill-hover:
    backgroundColor: "{colors.ink-white-hover}"
  button-default:
    backgroundColor: "{colors.fill-muted}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.sm}"
    height: "36px"
  card:
    backgroundColor: "{colors.panel-black}"
    rounded: "{rounded.md}"
    padding: "24px"
  input:
    backgroundColor: "{colors.fill-muted}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.sm}"
    height: "32px"
  nav-rail:
    backgroundColor: "{colors.panel-black}"
    width: "44px"
  list-row:
    textColor: "{colors.text-primary}"
    height: "24px"
  list-row-selected:
    backgroundColor: "{colors.fill-muted}"
  menu-item:
    textColor: "{colors.text-primary}"
    rounded: "{rounded.xs}"
    height: "26px"
    padding: "0 26px"
  titlebar:
    backgroundColor: "{colors.panel-black}"
    height: "36px"
---

# Design System: Orquestrador

## Overview

**Creative North Star: "A Bancada Escura"**

O Orquestrador é uma bancada de trabalho, não uma vitrine. O fundo é preto puro, os painéis são um preto um tom acima, e o único "acento" é o branco do texto e do botão principal. A cor existe só onde carrega sinal: estado de executor, diff, letra de status do Git, erro. Tudo o mais é neutro, para que os terminais e o código sejam o que se vê.

A densidade é a de um IDE: linhas de 24 px, faixas de 36 px, menus de 26 px, sempre com as medidas do VS Code / Antigravity quando há referência. Quem vem do VS Code deve reconhecer cada painel sem aprender nada. Os componentes vêm do Lobe UI (tema escuro fixo), e os tokens do Tailwind são ligados às variáveis `--ant-*` do tema, sem copiar valores; quando o tema muda, tudo acompanha.

A fonte detalhada das regras, com o porquê de cada medida, é `docs/design-system.md`. Este arquivo é o resumo para agentes; em caso de conflito, o código e `docs/design-system.md` vencem.

**Key Characteristics:**
- Preto puro com painéis em tons de preto; profundidade por tom, não por sombra.
- Branco como acento único; cor reservada a sinal.
- Densidade de IDE, com medidas herdadas do VS Code / Antigravity.
- Lobe UI como autoridade de componentes; Tailwind só para o que ele não cobre.
- Inter na interface; monoespaçado só em valor técnico.

## Colors

Uma paleta de neutros quase sem matiz, onde o branco faz o papel de cor primária e as cores saturadas só aparecem como sinal.

### Primary
- **Branco de Tinta** (ink-white): botão principal (`type="primary"`), filete de 2 px da navegação ativa e da seleção no Git, indicador da aba ativa no grupo com foco. Fica mais branco no hover e acinzentado no clique.

### Neutral
- **Preto Absoluto** (pitch-black): fundo da janela (`bg-background`), atrás de tudo.
- **Preto de Painel** (panel-black): barras, faixas, cards e a navegação (`bg-card`).
- **Preto Elevado** (raised-black): menus, popovers e modais.
- **Preto de Terminal** (terminal-black): fundo dos terminais xterm (`bg-terminal`), o único token próprio do app.
- **Texto** (text-primary / text-secondary / text-tertiary): conteúdo, detalhe (pasta, contagem, dica) e texto apagado.
- **Filete** (hairline / hairline-soft): bordas de 1 px e guias de recuo da árvore.
- **Preenchimento** (fill-muted / fill-hover): seleção de linha, contadores, hover de botão.

### Sinal
- **Erro** (signal-error), **sucesso** (signal-success), **alerta** (signal-warning) e **informação** (signal-info), do tema; estados de executor usam as cores predefinidas do Lobe UI no `Tag`.
- **Azul de Foco** (focus-blue): foco e unidade atual no comparador, como no VS Code.
- **Vermelho de Fechar** (close-red, a 90%): hover do botão Fechar da janela, como no Windows.

### Named Rules
**The Signal-Only Rule.** Cor saturada só comunica estado, mudança ou erro. Nada de cor decorativa.

**The Token Rule.** Componentes usam os tokens do tema (`bg-card`, `text-muted-foreground`, `border`…). Cores fixas só nas exceções documentadas: titlebar, comparador (cores do VS Code), destaque da pesquisa e as cores de estado nos ícones das Issues (aberta, fechada e o âmbar da em andamento, as do GitHub), centralizadas em `GH` no `src/issue-ui.tsx`, com a paleta fixa das bolinhas de label (`labelColor` em `src/issue-text.ts`). O resto das Issues usa os tokens: texto neutro, primário branco do app, labels como chip neutro com bolinha.

## Typography

**Body Font:** Inter Variable (com Inter, sans-serif)
**Mono Font:** Cascadia Mono nos terminais (com Cascadia Code, Consolas); o monoespaçado do tema nos demais valores técnicos.

**Character:** Inter neutra e legível em tamanho pequeno, sem personalidade de marca. O monoespaçado marca o que é dado técnico, não estilo.

### Hierarchy
- **Headline** (600, 20px): títulos raros, como o de telas vazias.
- **Title** (600, 16px): título de modal e de card.
- **Body** (400, 14px, 1.57): texto da interface.
- **Menu** (13px, linha de 26px): itens de menu de contexto e suspenso; atalho à direita a 70%.
- **Label** (400, 12px): contadores, metadados, texto secundário denso.
- **Mono** (13px, 1.15): terminal, caminho, modelo, esforço, número, trecho de resultado e conteúdo de arquivo.

### Named Rules
**The Technical-Mono Rule.** Monoespaçado só em terminal e valores técnicos. Nunca como estilo de rótulo ou título.

## Layout

Layout de IDE em colunas fixas, nada rola na janela (`html body { overflow: hidden }`): titlebar de 36 px no topo; navegação de 44 px à esquerda (Arquivos, Pesquisar, Git, Issues); faixa lateral redimensionável (mínimo 200 px); área central com editor em cima e painel Terminal embaixo; painel do Orquestrador à direita. Os painéis são `DraggablePanel` redimensionáveis e lembram o tamanho.

O ritmo é dado pelas alturas fixas: linha de lista de 24 px, faixa de abas e cabeçalho de 36 px, campo de 32 px, item de menu de 26 px. As faixas de abas rolam só na horizontal, com a barra escondida. Barras estreitas usam container queries (`@container`) para encolher rótulos abaixo de ~448–460 px, mantendo o texto completo no `title` e no `aria-label`.

Telas e terminais ficam montados o tempo todo; a troca usa `invisible`, nunca `display: none`, porque o xterm erra o tamanho ao sair do layout.

## Elevation & Depth

Plano por padrão. A profundidade vem do tom (preto → preto de painel → preto elevado) e de filetes de 1 px. Sombras só nas camadas que flutuam: menus, popovers e modais, com a sombra neutra do tema.

### Shadow Vocabulary
- **Flutuante** (`--ant-box-shadow-secondary`): modais e popovers.

### Named Rules
**The Flat-Bench Rule.** Superfícies de trabalho (painéis, faixas, listas) não têm sombra. Sem glow, sem gradiente decorativo, sem acento colorido na borda de cards.

## Shapes

Cantos discretos: 8 px nos cards, 6 px em botões e campos, 5 px em menus e 4 px nos itens de menu e badges. Os controles da janela não têm raio. Bordas de 1 px no tom do filete delimitam regiões; a aba ativa é marcada por uma linha de 2 px embaixo (`variant="square"`), não por fundo.

## Components

### Buttons
- **Shape:** cantos discretos (6px), altura padrão do tema (36px), pequeno nas barras.
- **Fill:** branco de tinta com texto preto, para a ação principal (Salvar, Commit, Escolher pasta; no detalhe da issue, o Colar no orquestrador).
- **Default:** preenchimento neutro, para Cancelar e ações secundárias.
- **Danger:** só na ação destrutiva, que também diz o que faz no texto ("Descartar e encerrar todos").
- **ActionIcon:** ações pequenas de ícone (24–36 px); foco por teclado com `focus-visible:ring-2`.

### Cards / Containers
- **Corner Style:** 8px.
- **Background:** preto de painel sobre o preto absoluto.
- **Shadow Strategy:** nenhuma (ver Elevation & Depth).
- **Border:** filete de 1px.

### Inputs / Fields
- **Style:** `Input` e `TextArea` do Lobe UI, 32 px na pesquisa; opções em `ActionIcon` no `suffix` com `aria-pressed`.
- **Focus:** padrão do Lobe UI.

### Navigation
- **Rail:** 44 px, `bg-card`, quatro `ActionIcon` de 36 px com ícone de 20 px. O ativo tem o fundo ativo do Lobe UI e um filete de 2 px na cor primária à esquerda. Contador do Git em `Badge` pequeno no canto do botão.
- **Tabs:** `TabsRoot variant="square"`, faixa de 36 px, linha de 2 px na aba ativa, divisor de 1 px em toda a largura.

### List Rows
- 24 px, ícone `File`, nome com prioridade (até 70%) e pasta em texto secundário que corta primeiro; ações num slot de largura fixa que aparece no hover ou no foco, sem deslocar o nome. Seleção em `bg-muted` com filete primário à esquerda; na faixa Issues, a linha escolhida (a que o detalhe mostra) é marcada assim, e o Colar compacto, um botão de ícone (`ActionIcon` com o ícone de colar, na cor do texto), só aparece nela e na próxima, no fluxo da linha. As linhas da faixa Issues têm 24 px, numa linha só (ícone de estado, `#N` em mono de 12 px numa coluna alinhada à direita, para os títulos começarem na mesma coluna, título e, na escolhida e na próxima, o botão de ícone), sem bolinhas de label; tempo e nomes das labels ficam no `title` e na descrição acessível.

### Execution State
- `Tag` com rótulo e ícone, cor predefinida do Lobe UI. Estado nunca só por cor. Na issue: Aberta, Fechada e Em andamento são `Tag` com texto neutro e a cor só no ícone (verde, roxo e âmbar do GitHub); a Próxima é neutra, com relógio (na linha da faixa, só o relógio antes do título). As quatro são `Tag` `outlined`, com o filete do tema e sem preenchimento, porque o chip de label é preenchido. Nunca pílula com a forma de uma label.

## Do's and Don'ts

### Do:
- **Do** usar componentes do Lobe UI e as props deles (`Text`, `Flexbox`, `Center`) antes de qualquer classe do Tailwind.
- **Do** usar os tokens do tema (`bg-card`, `text-muted-foreground`, `border`) para cor.
- **Do** seguir as medidas do VS Code / Antigravity quando houver referência (linha de 24 px, menu de 26 px, faixa de 36 px).
- **Do** mostrar estado com texto e ícone, além da cor.
- **Do** manter telas e terminais montados e trocar com `invisible`.
- **Do** atualizar `docs/design-system.md` ao trocar ou adicionar componente.

### Don't:
- **Don't** usar glows, gradientes decorativos ou acento colorido nas bordas de cards.
- **Don't** usar cor fixa num componente fora das exceções documentadas (titlebar, comparador, pesquisa e os ícones de estado das Issues).
- **Don't** usar monoespaçado fora de terminal e valores técnicos.
- **Don't** pôr sombra em superfícies de trabalho.
- **Don't** trocar a Inter pela fonte padrão do Lobe UI (HarmonyOS Sans, que viria da CDN).

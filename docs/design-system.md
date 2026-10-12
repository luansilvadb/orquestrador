# Design system

A interface usa [shadcn/ui](https://ui.shadcn.com) de verdade (base Radix, estilo `radix-vega`, base de cor `neutral`), com Tailwind CSS v4 pela integração do Vite. Tema escuro neutro; a classe `dark` fica em `index.html`.

## Onde fica

| Caminho | Papel |
| --- | --- |
| `components.json` | Configuração do registry do shadcn (aliases, estilo, base de cor). |
| `src/components/ui/` | Componentes gerados pelo CLI oficial (`npx shadcn@latest add ...`). Não editar à mão; regenerar pelo CLI. |
| `src/lib/utils.ts` | `cn`, gerado pelo CLI. |
| `src/index.css` | Tokens semânticos do shadcn (`:root` e `.dark`) e os poucos estilos próprios. |

Componentes em uso: `alert`, `alert-dialog`, `badge`, `button`, `card`, `input`, `label`, `progress`, `select`, `sonner`, `toggle-group` (ferramenta e esforço do formulário de escolha) e `toggle`, que é dependência de `toggle-group` (o CLI a instala junto). Para adicionar um componente, use o CLI e liste-o aqui.

## Regras

- Cores, raio, borda, foco e espaçamento vêm dos tokens semânticos (`bg-card`, `text-muted-foreground`, `border`, `ring`…). Sem cor fixa no componente.
- Os únicos tokens próprios são as cores de estado das linhas do canvas (`--status-*`) e a cor da grade (`--canvas-dot`), em `src/index.css`.
- Estado de execução sempre tem texto e ícone, não só cor (`Badge` com rótulo).
- CSS próprio só para geometria e comportamento do canvas e do xterm: `.canvas-grid`, o transform do mundo e o cursor durante arrasto. Canvas e terminais xterm são superfícies especializadas e não usam componentes shadcn.
- Monoespaçado só em terminal e valores técnicos (caminho, modelo, esforço).
- Sem glows, gradientes decorativos ou acento colorido nas bordas de todos os cards.

## Ajuste manual em `progress`

O `src/components/ui/progress.tsx` gerado pelo CLI desestruturava `value` e usava no transform do indicador, mas não o repassava ao `ProgressPrimitive.Root`. Resultado: `data-state` ficava sempre `indeterminate` e `aria-valuenow` sumia no download com total conhecido. Corrigido com `value={value}` no `Root`, antes do `{...props}`; o resto do componente segue o oficial. Ao regenerar o componente pelo CLI, reaplique essa linha.

## Motion

As animações de entrada e saída são as do shadcn oficial (`tw-animate-css` + variantes `data-open`/`data-closed` de `shadcn/tailwind.css`, importados em `src/index.css`). Verificado no navegador: `AlertDialog` anima overlay e conteúdo nos dois sentidos (`enter`/`exit`, 0,1s). O `Select` do formulário de escolha (`src/choice-form.tsx`) usa `position="popper"` com `align="start"`: é a variante animada do `radix-vega`, sem `animate-none`. O menu abre com `enter` e fecha com `exit`, 0,1s, com `fade-in-0`/`zoom-in-95` e `fade-out-0`/`zoom-out-95`, ancorado logo abaixo do gatilho e com a largura dele.

## Canvas e portais

Pan, zoom, arrasto e redimensionamento são listeners nativos do DOM (`src/canvas.tsx`). Blocos têm `data-block`, e o pan ignora qualquer evento dentro de um bloco. Select e AlertDialog usam portal no `body`, fora do mundo transformado, então funcionam sobre o canvas com zoom.

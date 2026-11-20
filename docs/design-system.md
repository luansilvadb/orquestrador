# Design system

Referência visual: o Lobe UI da antiga versão React (tema escuro fixo, fonte Inter) e as medidas do VS Code e do Antigravity.

## Regras

- Componentes do Kit (`Button`, `PopupMenu`, `Input`, `Tooltip`) antes de desenho próprio; o que é comum a duas ou mais telas vai para `crates/ui`.
- Dicas pelo `tooltip_texto` comum.
- Modais sobre a `mascara` comum; texto corrido de diálogo com `texto_pretty`.
- Estado de execução com texto e ícone, não só cor.

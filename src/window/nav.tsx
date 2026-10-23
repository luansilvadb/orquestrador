// A navegação da coluna de ícones: Arquivos, Pesquisa e Git, com o contador de alterações do Git.
import type { ReactNode } from 'react';
import { Files, type LucideIcon, Search } from 'lucide-react';
import { ActionIcon, Badge } from '@lobehub/ui/base-ui';
import { Flexbox } from '@lobehub/ui';
import { SourceControl } from './source-control-icon';

export type Route = 'arquivos' | 'pesquisa' | 'git';

/** Coluna de ícones: uma tela por botão, na ordem Arquivos, Pesquisar, Git. O botão da tela aberta tem aria-current e um filete à esquerda. */
export function Nav({ route, changed, onGo }: { route: Route | null; changed: number; onGo: (to: Route) => void }) {
  // Botão de 36 px com ícone de 20 px (o size numérico já dá essa proporção). A linha ocupa a faixa toda, para o filete encostar na borda.
  const button = (to: Route, label: string, icon: LucideIcon, title = label, badge?: ReactNode) => (
    <Flexbox width="100%" align="center" className="relative">
      {route === to && <span aria-hidden="true" className="absolute inset-y-2 left-0 w-0.5 rounded-full bg-[var(--ant-color-primary)]" />}
      <div className="relative">
        <ActionIcon
          icon={icon} size={20} variant="borderless" active={route === to}
          aria-label={label} aria-current={route === to ? 'page' : undefined} title={title}
          onClick={() => onGo(to)}
        />
        {badge}
      </div>
    </Flexbox>
  );
  const gitLabel = changed ? `Git (${changed} ${changed === 1 ? 'arquivo alterado' : 'arquivos alterados'})` : 'Git';
  // Contador no canto de baixo à direita do botão, com as medidas do badge do Antigravity (pílula de 12 px, fonte de 10 px, preta para dar contraste sobre o vermelho) e a posição 19/5 px dentro do botão de 36 px. Sem filhos, o Badge é a própria pílula: o `style` troca o tamanho e a sombra do tema, e não há deslocamento para fora do canto.
  const gitBadge = changed > 0 && (
    <Badge
      aria-hidden="true" count={changed} size="small" className="pointer-events-none absolute"
      style={{ top: 19, right: 5, minWidth: 12, height: 12, padding: '0 3px', fontSize: 10, fontWeight: 600, color: '#000', lineHeight: '12px', borderRadius: 4, boxShadow: 'none' }}
    />
  );
  return (
    <Flexbox as="nav" aria-label="Telas" flex="none" width={44} align="center" gap={4} paddingBlock={8} className="border-r bg-card">
      {button('arquivos', 'Arquivos', Files)}
      {button('pesquisa', 'Pesquisar', Search, 'Pesquisar (Ctrl+Shift+F)')}
      {button('git', gitLabel, SourceControl, 'Git', gitBadge)}
    </Flexbox>
  );
}

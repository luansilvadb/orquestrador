// Ícones dos controles da janela, como os do Antigravity: os codicons chrome-minimize (U+EABA), chrome-maximize (U+EAB9),
// chrome-restore (U+EABB) e chrome-close (U+EAB8) da fonte codicon.ttf instalada na IDE. Os contornos foram lidos da fonte
// (só esses quatro glifos, sem copiar a fonte) e convertidos para a grade de 16 px. Fonte: @vscode/codicons, © Microsoft,
// licença CC BY 4.0 (texto em src/assets/codicons/LICENSE).
import type { ComponentType } from 'react';

type WindowIconProps = { className?: string };

function windowIcon(d: string): ComponentType<WindowIconProps> {
  return function WindowIcon({ className }) {
    return (
      <svg xmlns="http://www.w3.org/2000/svg" width={16} height={16} viewBox="0 0 16 16" fill="currentColor" aria-hidden="true" className={className}>
        <path d={d} />
      </svg>
    );
  };
}

export const MinimizeIcon = windowIcon('M14.03 8.00L14.03 9.01L2.99 9.01L2.99 8.00L14.03 8.00Z');
export const MaximizeIcon = windowIcon('M2.99 2.99L2.99 13.01L13.01 13.01L13.01 2.99L2.99 2.99ZM12.00 12.00L4.00 12.00L4.00 4.00L12.00 4.00L12.00 12.00Z');
export const RestoreIcon = windowIcon('M2.99 5.01L2.99 13.97L12.00 13.97L12.00 5.01L2.99 5.01ZM10.99 13.01L4.00 13.01L4.00 5.97L10.99 5.97L10.99 13.01ZM5.01 5.01L6.03 5.01L6.03 4.00L13.01 4.00L13.01 10.99L12.00 10.99L12.00 12.00L14.03 12.00L14.03 2.99L5.01 2.99L5.01 5.01Z');
export const CloseIcon = windowIcon('M7.09 8.00L2.56 12.53L3.47 13.44L8.00 8.91L12.53 13.44L13.44 12.53L8.91 8.00L13.44 3.47L12.53 2.56L8.00 7.09L3.47 2.56L2.56 3.47L7.09 8.00Z');

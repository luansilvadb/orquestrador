// A marca do app (src-tauri/icons/icon.svg sem o fundo), estática. `color` pinta tudo de uma cor só, como a marca d'água do editor vazio.
export function AppMark({ className, color }: { className?: string; color?: string }) {
  const ink = color ?? '#ECEAE4';
  return (
    <svg viewBox="28 28 200 200" aria-hidden="true" className={className}>
      <g stroke={ink} strokeWidth={19} strokeLinecap="round">
        <line x1={73.9} y1={182.1} x2={73.9} y2={59.1} />
        <line x1={73.9} y1={182.1} x2={156.3} y2={91.1} />
        <line x1={73.9} y1={182.1} x2={196.3} y2={169.2} />
      </g>
      <g fill={ink}>
        <circle cx={73.9} cy={59.1} r={23} />
        <circle cx={156.3} cy={91.1} r={23} />
        <circle cx={196.3} cy={169.2} r={23} />
      </g>
      <circle cx={73.9} cy={182.1} r={36.5} fill={color ?? '#F5A524'} />
    </svg>
  );
}

import React, { useState } from 'react';
import type { FC } from 'react';

type Props = { titulo: string; contador?: number };

export const Painel: FC<Props> = ({ titulo, contador = 0 }) => {
  const [aberto, setAberto] = useState<boolean>(false);
  // Alterna o painel
  const alternar = () => setAberto((a) => !a);
  return (
    <div className="painel" onClick={alternar} data-x={1}>
      <h1 style={{ color: 'red' }}>{titulo}</h1>
      {aberto && <span>{contador * 2} itens</span>}
      <Botao rotulo="Fechar" desabilitado />
    </div>
  );
};

export default function Botao({ rotulo }: { rotulo: string; desabilitado?: boolean }) {
  return <button type="button">{rotulo.toUpperCase()}</button>;
}

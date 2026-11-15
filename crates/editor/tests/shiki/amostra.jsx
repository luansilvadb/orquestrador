import React from 'react';

export function Lista({ itens, aoClicar }) {
  const vazio = itens.length === 0;
  // Renderiza a lista
  return (
    <ul className="lista">
      {vazio ? <li>Nada</li> : itens.map((i) => (
        <li key={i.id} onClick={() => aoClicar(i)}>
          {i.nome} - <strong>{i.valor.toFixed(2)}</strong>
        </li>
      ))}
      <Rodape total={itens.length} visivel />
    </ul>
  );
}

const Rodape = ({ total }) => <footer>Total: {total}</footer>;
export default Lista;

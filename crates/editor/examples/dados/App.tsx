import { useState } from 'react';

export function App() {
  const [route, setRoute] = useState('arquivos');
  return <main>Orquestrador {route}</main>;
}

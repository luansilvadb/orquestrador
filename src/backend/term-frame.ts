// Quadro binário da saída dos terminais (formato em `src-tauri/src/core/frame.rs`):
// [u8 n][projeto n bytes][u8 m][id m bytes][u64 seq LE][bytes]; n = 0 é o terminal sem projeto.
import type { TermData } from './types';

const text = new TextDecoder();

export function decodeTermFrame(buffer: ArrayBuffer): TermData {
  const view = new DataView(buffer);
  const n = view.getUint8(0);
  const project = n === 0 ? null : text.decode(new Uint8Array(buffer, 1, n));
  const m = view.getUint8(1 + n);
  const id = text.decode(new Uint8Array(buffer, 2 + n, m));
  const at = 2 + n + m;
  // O contador de pedaços não passa de 2^53; getBigUint64 evitaria a conta, mas custa uma alocação por quadro.
  const seq = view.getUint32(at, true) + view.getUint32(at + 4, true) * 2 ** 32;
  return { project, id, seq, data: new Uint8Array(buffer, at + 8) };
}

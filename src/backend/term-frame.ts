// Quadro binário da saída dos terminais (formato em `src-tauri/src/core/frame.rs`):
// [u8 n][projeto n bytes][u8 m][id m bytes][u64 seq LE][bytes]; n = 0 é o terminal sem projeto.
// Um envio do canal leva vários, cada um com o tamanho na frente ([u32 LE][quadro]); o retrato é um quadro só, sem o tamanho.
import type { TermData } from './types';

const text = new TextDecoder();

/** O quadro em `buffer[start, start + length)`; sem os limites, o buffer inteiro. */
export function decodeTermFrame(buffer: ArrayBuffer, start = 0, length = buffer.byteLength - start): TermData {
  const view = new DataView(buffer, start, length);
  const n = view.getUint8(0);
  const project = n === 0 ? null : text.decode(new Uint8Array(buffer, start + 1, n));
  const m = view.getUint8(1 + n);
  const id = text.decode(new Uint8Array(buffer, start + 2 + n, m));
  const at = 2 + n + m;
  // O contador de pedaços não passa de 2^53; getBigUint64 evitaria a conta, mas custa uma alocação por quadro.
  const seq = view.getUint32(at, true) + view.getUint32(at + 4, true) * 2 ** 32;
  return { project, id, seq, data: new Uint8Array(buffer, start + at + 8, length - at - 8) };
}

/** Os quadros de um envio do canal, em ordem. */
export function* decodeTermFrames(buffer: ArrayBuffer): Generator<TermData> {
  const view = new DataView(buffer);
  for (let at = 0; at < buffer.byteLength;) {
    const length = view.getUint32(at, true);
    yield decodeTermFrame(buffer, at + 4, length);
    at += 4 + length;
  }
}

// Comentario de linha
import { readFile } from 'node:fs/promises';
import type { Config } from './config';

/** Documentacao do tipo. */
export interface Item<T> {
  id: number;
  nome?: string;
  valor: T;
}

export enum Cor { Vermelho = 1, Azul = 'azul' }

const LIMITE = 200_000;
let ativo = true;

export class Lista<T> extends Array<T> implements Iterable<T> {
  private readonly itens: Item<T>[] = [];
  constructor(public nome: string) { super(); }
  async carregar(caminho: string): Promise<number> {
    const texto = await readFile(caminho, 'utf8');
    const re = /^\s*(\d+)$/gm;
    for (const m of texto.matchAll(re)) this.itens.push({ id: Number(m[1]), valor: null as T });
    return this.itens.length > LIMITE ? -1 : this.itens.length;
  }
}

export function soma(a: number, b = 2): number {
  return a + b * 3 - (ativo ? 0 : 1);
}

const modelo = `total: ${soma(1)} itens`;
console.log(modelo, typeof modelo, new Date(), null, undefined);

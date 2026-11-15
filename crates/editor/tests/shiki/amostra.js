// Modulo de exemplo
'use strict';
const fs = require('fs');
import { join } from 'path';

export const LIMITE = 42;
let contador = 0, nome = "valor";

class Fila extends Base {
  #itens = [];
  static criar() { return new Fila(); }
  get tamanho() { return this.#itens.length; }
  async *percorrer() {
    for await (const item of this.#itens) yield item;
  }
}

function soma(a, b = 1, ...resto) {
  if (a === null || typeof b !== 'number') throw new Error(`invalido: ${a}`);
  return a + b + resto.reduce((x, y) => x + y, 0);
}

const re = /ab+c/gi;
const obj = { chave: true, outra: undefined, [nome]: 0x1f, n: 1e3 };
console.log(soma(1, 2), re.test('abc'), obj?.chave ?? false);

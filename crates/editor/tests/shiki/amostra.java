package com.exemplo;

import java.util.List;
import java.util.ArrayList;

/**
 * Classe de exemplo.
 */
@SuppressWarnings("unchecked")
public class Lista<T> extends Base implements Iterable<T> {
    private static final int LIMITE = 100;
    private final List<T> itens = new ArrayList<>();

    public Lista(String nome) {
        super(nome);
    }

    @Override
    public int tamanho() {
        // comentario
        return itens.size() > LIMITE ? -1 : itens.size();
    }

    public static void main(String[] args) throws Exception {
        var l = new Lista<String>("x");
        System.out.println("total: " + l.tamanho() + 'c' + 1.5f + true + null);
    }
}

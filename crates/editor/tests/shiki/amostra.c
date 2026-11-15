#include <stdio.h>
#include "util.h"

#define MAX 100
#define QUADRADO(x) ((x) * (x))

/* Estrutura de exemplo */
typedef struct {
    int x;
    const char *nome;
} Ponto;

static int contador = 0;

int soma(int a, unsigned long b) {
    // comentario de linha
    return a + (int)b;
}

int main(int argc, char **argv) {
    Ponto p = { .x = 1, .nome = "ponto" };
    for (int i = 0; i < MAX; i++) {
        if (argv[i] == NULL) break;
        printf("%d %s %c\n", i, p.nome, 'a');
    }
    return sizeof(Ponto) > 0 ? 0 : -1;
}

package main

import (
	"fmt"
	"strings"
)

// Ponto e um tipo.
type Ponto struct {
	X, Y int
	Nome string `json:"nome"`
}

const Limite = 10

func (p *Ponto) Mover(dx int) error {
	if p == nil {
		return fmt.Errorf("nulo: %d", dx)
	}
	p.X += dx
	return nil
}

func main() {
	itens := []string{"a", "b"}
	for i, s := range itens {
		fmt.Println(i, strings.ToUpper(s), true, 3.14)
	}
	ch := make(chan int)
	go func() { ch <- 1 }()
	defer close(ch)
}

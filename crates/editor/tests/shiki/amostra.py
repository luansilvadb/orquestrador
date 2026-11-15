#!/usr/bin/env python3
"""Docstring do modulo."""
import os
from typing import Optional, List

LIMITE = 100

@dataclass
class Ponto(Base):
    """Um ponto."""
    x: int = 0

    def __init__(self, nome: str, valor: Optional[float] = None) -> None:
        super().__init__()
        self.nome = nome
        self._valor = valor or 1.5

    async def carregar(self, caminho):
        with open(caminho, encoding="utf-8") as f:
            return [linha.strip() for linha in f if not linha.startswith('#')]

def soma(*args, **kwargs) -> int:
    # comentario
    total = sum(args) + len(kwargs)
    print(f"total = {total!r} {True} {None}")
    return total if total > LIMITE else -1

lambda x: x ** 2

import 'dart:async';
import 'package:flutter/material.dart';

// Classe de exemplo
abstract class Forma {
  double get area;
}

/// Um ponto.
class Ponto extends Forma with Igual implements Comparable<Ponto> {
  final int x;
  double y;
  static const limite = 100;

  Ponto(this.x, {this.y = 0.0});

  @override
  double get area => 0;

  Future<int> carregar(String caminho) async {
    final texto = 'arquivo: $caminho ${x + 1}';
    return texto.length > limite ? -1 : x;
  }
}

void main() {
  var p = Ponto(1, y: 2.5);
  print(p.area == null ? true : false);
}

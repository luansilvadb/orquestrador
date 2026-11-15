import Foundation

// Protocolo de exemplo
protocol Forma {
    var area: Double { get }
}

/// Um ponto.
struct Ponto: Forma, Equatable {
    let x: Int
    var y: Double = 0.0
    var area: Double { return 0 }
}

final class Lista<T>: Base {
    private var itens: [T] = []
    static let limite = 100

    init(nome: String) {
        super.init()
    }

    func carregar(_ caminho: String) async throws -> Int {
        guard !caminho.isEmpty else { return -1 }
        let texto = "arquivo: \(caminho)"
        return texto.count > Lista.limite ? nil ?? 0 : itens.count
    }
}

let p = Ponto(x: 1)
print(p, true, 1.5)

package exemplo

import kotlin.math.max

// Classe de dados
data class Ponto(val x: Int, var y: Double = 0.0)

sealed interface Forma
object Vazio : Forma

/** Lista generica. */
class Lista<T>(private val itens: MutableList<T> = mutableListOf()) : Iterable<T> {
    companion object { const val LIMITE = 100 }

    override fun iterator(): Iterator<T> = itens.iterator()

    suspend fun carregar(caminho: String): Int {
        val texto = "arquivo: $caminho ${itens.size}"
        return if (texto.length > LIMITE) -1 else max(1, 2)
    }
}

fun main() {
    val l = Lista<String>()
    println(l.count() + 'c'.code + 1.5f.toInt() + (null ?: 0))
    when (val n = 3) { in 1..5 -> println(true) else -> {} }
}

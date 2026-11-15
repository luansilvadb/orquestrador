using System;
using System.Collections.Generic;

namespace Exemplo
{
    /// <summary>Classe de exemplo.</summary>
    public class Lista<T> : Base, IEnumerable<T> where T : class
    {
        private const int Limite = 100;
        private readonly List<T> itens = new();

        public string Nome { get; set; } = "padrao";

        public Lista(string nome) : base(nome) { }

        [Obsolete("usar outro")]
        public async Task<int> CarregarAsync(string caminho)
        {
            // comentario
            var texto = await File.ReadAllTextAsync(caminho);
            return texto.Length > Limite ? -1 : $"{texto}".Length;
        }

        public static void Main(string[] args) => Console.WriteLine(true ? 'c' : null);
    }
}

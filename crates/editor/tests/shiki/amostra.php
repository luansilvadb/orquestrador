<?php
declare(strict_types=1);

namespace App\Modelos;

use App\Base;

/**
 * Classe de exemplo.
 */
final class Usuario extends Base implements \JsonSerializable
{
    private const LIMITE = 10;
    public static int $total = 0;

    public function __construct(private string $nome, protected ?int $idade = null)
    {
        parent::__construct();
    }

    public function saudar(): string
    {
        // comentario
        $texto = "Ola, {$this->nome}!";
        return strlen($texto) > self::LIMITE ? $texto : 'curto';
    }
}

$u = new Usuario('Ana', 30);
echo $u->saudar(), PHP_EOL, true, null;

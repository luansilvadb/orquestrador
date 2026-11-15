-- Consulta de exemplo
CREATE TABLE usuarios (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    nome VARCHAR(100) NOT NULL,
    criado TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO usuarios (nome) VALUES ('Ana'), ('Bia');

SELECT u.id, COUNT(*) AS total, MAX(p.valor)
FROM usuarios u
LEFT JOIN pedidos p ON p.usuario_id = u.id
WHERE u.nome LIKE 'A%' AND p.valor > 10.5
GROUP BY u.id
HAVING COUNT(*) > 1
ORDER BY total DESC
LIMIT 10;

/* bloco */
UPDATE usuarios SET nome = NULL WHERE id IN (1, 2);

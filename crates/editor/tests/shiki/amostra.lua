-- Modulo de exemplo
local M = {}

local LIMITE = 10

--[[ Comentario
em bloco ]]
function M.soma(a, b)
  b = b or 1
  return a + b
end

local function saudar(nome)
  local texto = "Ola, " .. nome .. '!'
  if #texto > LIMITE and not false then
    print(texto)
  elseif nome == nil then
    return nil
  end
end

for i = 1, 10, 2 do
  saudar(tostring(i))
end

local t = { chave = true, [1] = 0x1F, outra = 3.14 }
return M

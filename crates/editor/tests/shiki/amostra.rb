# Exemplo Ruby
require 'json'

module Exemplo
  LIMITE = 10

  class Usuario < Base
    attr_reader :nome

    def initialize(nome, idade = nil)
      super()
      @nome = nome
      @@total ||= 0
    end

    def saudar
      texto = "Ola, #{@nome}!"
      texto.length > LIMITE ? texto : :curto
    end
  end
end

u = Exemplo::Usuario.new('Ana', 30)
[1, 2, 3].each { |i| puts i * 2 if i.even? }
puts u.saudar, nil, true, /ab+c/

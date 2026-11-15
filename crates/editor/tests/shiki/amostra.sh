#!/usr/bin/env bash
# Script de exemplo
set -euo pipefail

NOME="mundo"
readonly LIMITE=10
export CAMINHO=$HOME/bin

saudar() {
  local quem="$1"
  echo "Ola, ${quem}! $(date +%Y)"
}

for i in $(seq 1 $LIMITE); do
  if [[ $i -gt 5 && -f "$CAMINHO/x" ]]; then
    saudar "$NOME" > /dev/null 2>&1
  fi
done

case "$1" in
  start) echo 'iniciando' ;;
  *) exit 1 ;;
esac

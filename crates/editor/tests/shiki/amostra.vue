<template>
  <div class="painel" :class="{ ativo }" @click="alternar">
    <h1>{{ titulo }}</h1>
    <span v-if="ativo">{{ contador * 2 }} itens</span>
    <Botao rotulo="Fechar" />
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
// Estado
const props = defineProps<{ titulo: string }>();
const ativo = ref(false);
const contador = ref(0);
function alternar() {
  ativo.value = !ativo.value;
}
</script>

<style scoped>
.painel { color: #ccc; }
</style>

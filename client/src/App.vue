<script setup lang="ts">
import AppWindow from '@/components/shell/AppWindow.vue'

/**
 * Корень приложения. Всё, что есть на экране, лежит внутри окна (F13) — включая
 * экран блокировки и онбординг: полоса заголовка есть и там.
 */
</script>

<template>
  <AppWindow>
    <RouterView />
  </AppWindow>
</template>

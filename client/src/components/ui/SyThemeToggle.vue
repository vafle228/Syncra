<script setup lang="ts">
import { computed } from 'vue'

import { useTheme } from '@/composables/useTheme'

/** Переключатель темы (F2): тёмная → светлая → системная. */

const { preference, cycleTheme } = useTheme()

const label = computed(() => {
  if (preference.value === 'dark') return 'Тёмная'
  if (preference.value === 'light') return 'Светлая'
  return 'Системная'
})
</script>

<template>
  <button type="button" class="sy-theme-toggle" :title="`Тема: ${label}`" @click="cycleTheme()">
    <span class="sy-theme-toggle__caption">Тема</span>
    <span class="sy-theme-toggle__value">{{ label }}</span>
  </button>
</template>

<style scoped>
.sy-theme-toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--sy-space-4);
  height: var(--sy-control-height-sm);
  padding: 0 var(--sy-space-4);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  cursor: pointer;
}

.sy-theme-toggle:hover {
  border-color: var(--sy-accent);
}

.sy-theme-toggle__caption {
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.sy-theme-toggle__value {
  font-size: var(--sy-text-small);
  font-weight: var(--sy-weight-medium);
  color: var(--sy-text);
}
</style>

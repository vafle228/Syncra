<script setup lang="ts">
import { useToastStore } from '@/stores/useToastStore'

/**
 * Хост тостов (F2). Монтируется один раз в `App.vue`, читает очередь из стора.
 * Текст приходит из стора — про запрет секретов см. `useToastStore.ts`.
 */

const toasts = useToastStore()
</script>

<template>
  <div class="sy-toast-host" role="status" aria-live="polite">
    <div
      v-for="toast in toasts.toasts"
      :key="toast.id"
      class="sy-toast"
      :class="`sy-toast--${toast.tone}`"
    >
      <span class="sy-toast__dot" aria-hidden="true" />
      <span class="sy-toast__text">{{ toast.text }}</span>
      <button
        type="button"
        class="sy-toast__close"
        title="Скрыть"
        @click="toasts.dismiss(toast.id)"
      >
        ×
      </button>
    </div>
  </div>
</template>

<style scoped>
.sy-toast-host {
  position: fixed;
  left: 0;
  right: 0;
  bottom: var(--sy-space-9);
  z-index: 70;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--sy-space-3);
  padding: 0 var(--sy-space-6);
  pointer-events: none;
}

.sy-toast {
  display: flex;
  align-items: center;
  gap: 11px;
  min-height: 42px;
  padding: var(--sy-space-3) var(--sy-space-3) var(--sy-space-3) var(--sy-space-5);
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius-pill);
  background: var(--sy-surface);
  box-shadow: var(--sy-shadow-window-2);
  font-size: 13px;
  line-height: 1.4;
  color: var(--sy-text);
  pointer-events: auto;
}

.sy-toast--warning {
  border-color: var(--sy-warn);
}

.sy-toast--danger {
  border-color: var(--sy-danger);
}

.sy-toast__dot {
  flex: none;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--sy-accent);
}

.sy-toast--warning .sy-toast__dot {
  background: var(--sy-warn);
}

.sy-toast--danger .sy-toast__dot {
  background: var(--sy-danger);
}

.sy-toast__text {
  text-wrap: pretty;
}

.sy-toast__close {
  flex: none;
  width: 26px;
  height: 26px;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: var(--sy-text-3);
  font-size: 16px;
  line-height: 1;
  cursor: pointer;
}

.sy-toast__close:hover {
  background: var(--sy-surface-2);
  color: var(--sy-text);
}
</style>

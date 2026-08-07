<script setup lang="ts">
import { useId } from 'vue'

/**
 * Тумблер (F7, §4.2 — синхронизация секции).
 *
 * Это `role="switch"`, а не чекбокс: он не «отмечает пункт», а прямо сейчас
 * включает и выключает поведение. Подпись состояния — словами рядом, а не
 * только положением ручки: «включено/выключено» цветом читается не всеми.
 */

withDefaults(
  defineProps<{
    modelValue: boolean
    /** Подпись — что именно переключается. */
    label: string
    /** Пояснение под подписью: чем состояния отличаются. */
    description?: string
    /** Текст справа от ручки — состояние словами. */
    stateText?: string
    disabled?: boolean
    /** Команда в ядре ещё идёт: тумблер не двигаем, пока не ответило. */
    busy?: boolean
  }>(),
  { disabled: false, busy: false },
)

const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>()

const labelId = useId()
const descriptionId = `${labelId}-note`
</script>

<template>
  <div class="sy-toggle" :class="{ 'sy-toggle--on': modelValue }">
    <div class="sy-toggle__text">
      <span :id="labelId" class="sy-toggle__label">{{ label }}</span>
      <span v-if="description" :id="descriptionId" class="sy-toggle__description">
        {{ description }}
      </span>
    </div>

    <span v-if="stateText" class="sy-toggle__state">{{ stateText }}</span>

    <button
      type="button"
      class="sy-toggle__switch"
      role="switch"
      :aria-checked="modelValue"
      :aria-labelledby="labelId"
      :aria-describedby="description ? descriptionId : undefined"
      :aria-busy="busy || undefined"
      :disabled="disabled || busy"
      @click="emit('update:modelValue', !modelValue)"
    >
      <span class="sy-toggle__knob" aria-hidden="true" />
    </button>
  </div>
</template>

<style scoped>
.sy-toggle {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
}

.sy-toggle__text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
}

.sy-toggle__label {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-medium);
}

.sy-toggle__description {
  font-size: var(--sy-text-small);
  line-height: 1.5;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.sy-toggle__state {
  flex: none;
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  letter-spacing: 0.06em;
  color: var(--sy-text-3);
}

.sy-toggle--on .sy-toggle__state {
  color: var(--sy-accent);
}

.sy-toggle__switch {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: flex-start;
  width: 38px;
  height: 22px;
  padding: 2px;
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius-pill);
  background: var(--sy-surface-2);
  cursor: pointer;
  transition:
    background var(--sy-transition),
    border-color var(--sy-transition),
    justify-content var(--sy-transition);
}

.sy-toggle--on .sy-toggle__switch {
  justify-content: flex-end;
  border-color: transparent;
  background: var(--sy-accent);
}

.sy-toggle__switch:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

.sy-toggle__switch:disabled {
  cursor: not-allowed;
  opacity: 0.6;
}

.sy-toggle__knob {
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--sy-text-3);
  transition: background var(--sy-transition);
}

.sy-toggle--on .sy-toggle__knob {
  background: var(--sy-accent-fg);
}
</style>

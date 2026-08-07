<script setup lang="ts">
import { computed, useId } from 'vue'

/**
 * Выпадающий список (F7 — выбор секции в форме записи).
 *
 * Намеренно нативный `<select>`: он одинаково работает с клавиатуры, с
 * экранного диктора и с сенсорного экрана, а самодельный дропдаун всё это
 * пришлось бы отвоёвывать заново. Оформление — токенами, как у `SyInput`.
 */

const props = defineProps<{
  modelValue: string
  label?: string
  options: { value: string; label: string }[]
  hint?: string
  disabled?: boolean
}>()

const emit = defineEmits<{ 'update:modelValue': [value: string] }>()

const selectId = useId()
const describedById = `${selectId}-note`

const empty = computed(() => props.options.length === 0)

function onChange(event: Event): void {
  emit('update:modelValue', (event.target as HTMLSelectElement).value)
}
</script>

<template>
  <div class="sy-select">
    <label v-if="label" class="sy-select__label" :for="selectId">{{ label }}</label>

    <div class="sy-select__box">
      <select
        :id="selectId"
        class="sy-select__control"
        :value="modelValue"
        :disabled="disabled || empty"
        :aria-describedby="hint ? describedById : undefined"
        @change="onChange"
      >
        <option v-for="option in options" :key="option.value" :value="option.value">
          {{ option.label }}
        </option>
      </select>
      <span class="sy-select__chevron" aria-hidden="true" />
    </div>

    <p v-if="hint" :id="describedById" class="sy-select__note">{{ hint }}</p>
  </div>
</template>

<style scoped>
.sy-select {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.sy-select__label {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.sy-select__box {
  position: relative;
  display: flex;
  align-items: center;
  height: var(--sy-control-height-lg);
  padding: 0 var(--sy-space-5);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  transition:
    border-color var(--sy-transition),
    box-shadow var(--sy-transition);
}

.sy-select__box:focus-within {
  border-color: var(--sy-accent);
  box-shadow: var(--sy-focus-ring);
}

.sy-select__control {
  flex: 1;
  min-width: 0;
  height: 100%;
  padding-right: var(--sy-space-6);
  border: none;
  outline: none;
  background: transparent;
  color: var(--sy-text);
  font-family: inherit;
  font-size: 14px;
  appearance: none;
  cursor: pointer;
}

.sy-select__control:disabled {
  color: var(--sy-text-3);
  cursor: not-allowed;
}

.sy-select__chevron {
  position: absolute;
  right: var(--sy-space-5);
  width: 7px;
  height: 7px;
  border-right: 1.5px solid var(--sy-text-3);
  border-bottom: 1.5px solid var(--sy-text-3);
  transform: translateY(-2px) rotate(45deg);
  pointer-events: none;
}

.sy-select__note {
  font-size: 11.5px;
  line-height: 1.45;
  color: var(--sy-text-3);
}
</style>

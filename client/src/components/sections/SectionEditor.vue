<script setup lang="ts">
import { SyButton, SyInput, VAULT_COLOR_NAMES, vaultColorVar } from '@/components/ui'
import { VAULT_COLORS, VAULT_NAME_MAX_LENGTH, type VaultColor } from '@/core/contract'

/**
 * Форма секции — создание и переименование (F7, экран «Секции» макета).
 *
 * Разворачивается на месте карточки, а не в диалоге: переименование не то
 * действие, ради которого стоит закрывать собой остальные секции.
 */

const props = defineProps<{
  name: string
  color: VaultColor
  /** Подпись поля: «Название секции» или «Название новой секции». */
  label: string
  submitLabel: string
  saving?: boolean
  /** Сообщение ядра или проверки имени. */
  error?: string | null
}>()

const emit = defineEmits<{
  'update:name': [value: string]
  'update:color': [value: VaultColor]
  submit: []
  cancel: []
}>()

/** Максимум длины дублирован из контракта: ядро проверит повторно. */
const maxLength = VAULT_NAME_MAX_LENGTH
</script>

<template>
  <form class="section-editor" @submit.prevent="emit('submit')" @keydown.escape="emit('cancel')">
    <SyInput
      class="section-editor__name"
      :model-value="props.name"
      :label="label"
      placeholder="Например, Учёба"
      :error="error"
      :hint="`Не длиннее ${maxLength} символов`"
      autofocus
      @update:model-value="emit('update:name', $event)"
      @submit="emit('submit')"
    />

    <fieldset class="section-editor__colors">
      <legend class="section-editor__colors-label">Цвет метки</legend>
      <button
        v-for="option in VAULT_COLORS"
        :key="option"
        type="button"
        class="section-editor__color"
        :class="{ 'section-editor__color--on': props.color === option }"
        :style="{ background: vaultColorVar(option) }"
        :aria-pressed="props.color === option"
        :title="VAULT_COLOR_NAMES[option]"
        @click="emit('update:color', option)"
      >
        <span class="section-editor__color-name">{{ VAULT_COLOR_NAMES[option] }}</span>
      </button>
    </fieldset>

    <div class="section-editor__actions">
      <SyButton variant="primary" size="sm" type="submit" :loading="saving">
        {{ submitLabel }}
      </SyButton>
      <SyButton size="sm" :disabled="saving" @click="emit('cancel')">Отмена</SyButton>
    </div>
  </form>
</template>

<style scoped>
.section-editor {
  display: flex;
  align-items: flex-start;
  gap: var(--sy-space-6);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-accent);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-0);
  box-shadow: var(--sy-focus-ring);
}

.section-editor__name {
  flex: 1;
  min-width: 0;
}

.section-editor__colors {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
  margin: 0;
  padding: 0;
  border: none;
}

.section-editor__colors-label {
  padding: 0;
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.section-editor__color {
  position: relative;
  display: inline-block;
  width: 24px;
  height: 24px;
  margin-right: var(--sy-space-3);
  border: 1px solid var(--sy-border);
  border-radius: 7px;
  cursor: pointer;
}

.section-editor__color--on {
  border: 2px solid var(--sy-text);
}

.section-editor__color:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

/* Название цвета нужно скринридеру и не нужно глазу — кружка достаточно. */
.section-editor__color-name {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
}

.section-editor__actions {
  flex: none;
  display: flex;
  gap: var(--sy-space-3);
  margin-top: 22px;
}
</style>

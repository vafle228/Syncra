<script setup lang="ts">
import { useId } from 'vue'

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

const colorsLabelId = useId()
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

    <!--
      Не `<fieldset>/<legend>`: подпись и ряд кружков должны быть двумя
      отдельными ячейками сетки, а `<legend>` обязан лежать внутри `<fieldset>`.
      `role="group"` с `aria-labelledby` даёт диктору ровно то же самое.
    -->
    <span :id="colorsLabelId" class="section-editor__colors-label">Цвет метки</span>

    <div class="section-editor__colors" role="group" :aria-labelledby="colorsLabelId">
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
    </div>

    <div class="section-editor__actions">
      <SyButton variant="primary" size="field" type="submit" :loading="saving">
        {{ submitLabel }}
      </SyButton>
      <SyButton size="field" :disabled="saving" @click="emit('cancel')">Отмена</SyButton>
    </div>
  </form>
</template>

<style scoped>
.section-editor {
  /*
   * Имя · цвет · действия, и общая строка подписей сверху. Кнопки встают
   * вровень с полем не потому, что им отмерено 22px сверху, а потому, что они
   * стоят в той же строке сетки, что и кружки, — под общей строкой подписей.
   * Высота этой строки складывается из тех же токенов, из которых её считает
   * себе `SyInput`, так что разъехаться им теперь нечем.
   */
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto;
  /*
   * Первая строка ровно в высоту подписи, а не `auto`: поле занимает обе
   * строки, и при `auto` сетка раздала бы его высоту им обеим — вторая строка
   * уехала бы вниз вместе с кнопками.
   */
  grid-template-rows: var(--sy-text-label-lh) auto;
  align-items: start;
  gap: var(--sy-space-2) var(--sy-space-6);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-accent);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-0);
  box-shadow: var(--sy-focus-ring);
}

/* Поле само рисует подпись, значение и подсказку — отдаём ему обе строки. */
.section-editor__name {
  grid-column: 1;
  grid-row: 1 / span 2;
  min-width: 0;
}

.section-editor__colors-label {
  grid-column: 2;
  grid-row: 1;
  padding: 0;
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

/*
 * Ряд, а не колонка. Раньше кружки выстраивались в строку только потому, что
 * каждый был `inline-block` с полем справа: стоило одному стать блочным — и
 * ряд рассыпался бы. Высота фиксирована по полю ввода, чтобы кружки стояли
 * вровень с ним и с кнопками справа.
 */
.section-editor__colors {
  grid-column: 2;
  grid-row: 2;
  display: flex;
  align-items: center;
  gap: var(--sy-space-3);
  height: var(--sy-control-height-field);
}

.section-editor__color {
  position: relative;
  flex: none;
  width: 24px;
  height: 24px;
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-inner);
  cursor: pointer;
}

/*
 * Выбор рисуется тенью ВНУТРЬ, а не второй рамкой: рамка в 2px увеличила бы
 * кружок и сдвинула соседей — метка «выбрано» не должна двигать вёрстку.
 */
.section-editor__color--on {
  box-shadow: inset 0 0 0 2px var(--sy-text);
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
  grid-column: 3;
  grid-row: 2;
  display: flex;
  gap: var(--sy-space-3);
}
</style>

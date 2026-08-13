<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId, watch } from 'vue'

/**
 * Выпадающий список (F7 — выбор секции в форме записи).
 *
 * Раньше это был нативный `<select>`: он бесплатно давал клавиатуру и диктора,
 * и ради этого мы мирились с тем, что список рисует ОС. Оказалось, что цена
 * выше: раскрытый список приходит в системных цветах, со своим шрифтом и своими
 * скруглениями — посреди тёмного окна это выглядит чужой деталью, и никакими
 * токенами это не лечится. Поэтому здесь свой дропдаун по макету
 * (`Прототип:1741-1769`), а всё, что нативный список давал даром, отвоёвано
 * руками: роли `combobox`/`listbox`, стрелки, Home/End, Enter, Escape и
 * закрытие щелчком мимо.
 *
 * Публичный API прежний (`modelValue`, `label`, `options`, `hint`, `disabled`),
 * добавились цветная точка у варианта и слот `#footer` под ссылку вроде
 * «Управление секциями…».
 */

export type SySelectOption = {
  value: string
  label: string
  /** Цвет метки слева — готовое CSS-значение, обычно `var(--sy-vault-…)`. */
  dot?: string
}

const props = defineProps<{
  modelValue: string
  label?: string
  options: SySelectOption[]
  hint?: string
  disabled?: boolean
}>()

const emit = defineEmits<{ 'update:modelValue': [value: string] }>()

const baseId = useId()
const triggerId = `${baseId}-trigger`
const labelId = `${baseId}-label`
const listId = `${baseId}-list`
const describedById = `${baseId}-note`

const optionId = (index: number): string => `${baseId}-option-${index}`

const empty = computed(() => props.options.length === 0)
const unavailable = computed(() => props.disabled === true || empty.value)

const selectedIndex = computed(() => props.options.findIndex((o) => o.value === props.modelValue))
const selected = computed(() => props.options[selectedIndex.value] ?? null)

const open = ref(false)
const root = ref<HTMLElement | null>(null)
const panel = ref<HTMLElement | null>(null)

/**
 * Подсвеченный вариант при работе с клавиатуры. Это НЕ выбор: выбор происходит
 * только по Enter или щелчку, иначе стрелки молча меняли бы секцию записи.
 */
const activeIndex = ref(0)

function openPanel(): void {
  if (unavailable.value || open.value) return
  activeIndex.value = selectedIndex.value >= 0 ? selectedIndex.value : 0
  open.value = true
}

function closePanel(): void {
  open.value = false
}

function toggle(): void {
  if (open.value) closePanel()
  else openPanel()
}

function pick(value: string): void {
  emit('update:modelValue', value)
  closePanel()
}

/** Держим подсвеченный вариант в поле зрения: панель прокручивается. */
watch([open, activeIndex], async () => {
  if (!open.value) return
  await nextTick()
  const item = panel.value?.querySelector<HTMLElement>(`[data-index="${activeIndex.value}"]`)
  // В jsdom прокрутки нет — вызов необязательный, чтобы тесты не падали на ней.
  item?.scrollIntoView?.({ block: 'nearest' })
})

function move(delta: number): void {
  const last = props.options.length - 1
  activeIndex.value = Math.min(last, Math.max(0, activeIndex.value + delta))
}

function onKeydown(event: KeyboardEvent): void {
  if (unavailable.value) return

  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault()
      if (open.value) move(1)
      else openPanel()
      return
    case 'ArrowUp':
      event.preventDefault()
      if (open.value) move(-1)
      else openPanel()
      return
    case 'Home':
      if (!open.value) return
      event.preventDefault()
      activeIndex.value = 0
      return
    case 'End':
      if (!open.value) return
      event.preventDefault()
      activeIndex.value = props.options.length - 1
      return
    case 'Enter':
    case ' ':
      event.preventDefault()
      if (!open.value) openPanel()
      else pick(props.options[activeIndex.value]!.value)
      return
    case 'Escape':
      if (!open.value) return
      // Гасим событие: иначе Escape заодно закроет форму, в которой стоит поле.
      event.preventDefault()
      event.stopPropagation()
      closePanel()
      return
    case 'Tab':
      closePanel()
  }
}

/**
 * Закрытие щелчком мимо — тот же приём, что у поповера «···» в карточке. Без
 * него список выглядел бы ловушкой: открыл и не понял, чем закрыть.
 */
function onDocumentPointerDown(event: MouseEvent): void {
  if (!open.value) return
  if (root.value?.contains(event.target as Node)) return
  closePanel()
}

onMounted(() => document.addEventListener('mousedown', onDocumentPointerDown))
onBeforeUnmount(() => document.removeEventListener('mousedown', onDocumentPointerDown))
</script>

<template>
  <div class="sy-select">
    <span v-if="label" :id="labelId" class="sy-select__label">{{ label }}</span>

    <div ref="root" class="sy-select__box">
      <button
        :id="triggerId"
        type="button"
        class="sy-select__trigger"
        role="combobox"
        aria-haspopup="listbox"
        :aria-expanded="open"
        :aria-controls="open ? listId : undefined"
        :aria-activedescendant="open ? optionId(activeIndex) : undefined"
        :aria-labelledby="label ? `${labelId} ${triggerId}` : undefined"
        :aria-describedby="hint ? describedById : undefined"
        :disabled="unavailable"
        @click="toggle"
        @keydown="onKeydown"
      >
        <span class="sy-select__value">
          <span
            v-if="selected?.dot"
            class="sy-select__dot"
            :style="{ background: selected.dot }"
            aria-hidden="true"
          />
          <span class="sy-select__value-text">{{ selected?.label ?? '' }}</span>
        </span>
        <span class="sy-select__chevron" aria-hidden="true" />
      </button>

      <div v-if="open" ref="panel" class="sy-select__panel">
        <div :id="listId" class="sy-select__list" role="listbox" :aria-labelledby="labelId">
          <!--
            `tabindex="-1"`: фокус остаётся на триггере, а какой вариант «под
            рукой», диктору сообщает `aria-activedescendant`. Иначе Tab уводил бы
            по одному варианту за раз.
          -->
          <button
            v-for="(option, index) in options"
            :id="optionId(index)"
            :key="option.value"
            type="button"
            tabindex="-1"
            role="option"
            :data-index="index"
            class="sy-select__option"
            :class="{
              'sy-select__option--on': option.value === modelValue,
              'sy-select__option--active': index === activeIndex,
            }"
            :aria-selected="option.value === modelValue"
            @click="pick(option.value)"
            @mousemove="activeIndex = index"
          >
            <span
              v-if="option.dot"
              class="sy-select__dot"
              :style="{ background: option.dot }"
              aria-hidden="true"
            />
            <span class="sy-select__option-label">{{ option.label }}</span>
          </button>
        </div>

        <template v-if="$slots.footer">
          <span class="sy-select__rule" aria-hidden="true" />
          <div class="sy-select__footer"><slot name="footer" /></div>
        </template>
      </div>
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
}

/* Триггер */

.sy-select__trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 9px;
  width: 100%;
  height: var(--sy-control-height-field);
  padding: 0 var(--sy-space-5);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  font-family: inherit;
  cursor: pointer;
  transition:
    border-color var(--sy-transition),
    box-shadow var(--sy-transition);
}

.sy-select__trigger:hover:not(:disabled) {
  border-color: var(--sy-border-strong);
}

.sy-select__trigger:disabled {
  cursor: not-allowed;
}

.sy-select__value {
  display: flex;
  align-items: center;
  gap: 9px;
  min-width: 0;
}

.sy-select__value-text {
  font-size: 14px;
  color: var(--sy-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sy-select__trigger:disabled .sy-select__value-text {
  color: var(--sy-text-3);
}

.sy-select__dot {
  flex: none;
  width: 7px;
  height: 7px;
  border-radius: 2px;
}

.sy-select__chevron {
  flex: none;
  width: 7px;
  height: 7px;
  border-right: 1.5px solid var(--sy-text-3);
  border-bottom: 1.5px solid var(--sy-text-3);
  transform: rotate(45deg) translateY(-2px);
}

/* Панель */

.sy-select__panel {
  position: absolute;
  top: 42px;
  right: 0;
  left: 0;
  z-index: 20;
  display: flex;
  flex-direction: column;
  max-height: 200px;
  overflow: auto;
  padding: 5px;
  border: 1px solid var(--sy-border-strong);
  border-radius: 9px;
  background: var(--sy-surface);
  box-shadow: var(--sy-shadow-window-2);
  animation: sy-in 0.14s ease-out;
}

.sy-select__list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.sy-select__option {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  height: var(--sy-control-height-md);
  padding: 0 var(--sy-space-4);
  border: none;
  border-radius: var(--sy-radius-inner);
  background: transparent;
  font-family: inherit;
  text-align: left;
  cursor: pointer;
}

.sy-select__option-label {
  flex: 1;
  min-width: 0;
  font-size: var(--sy-text-body);
  color: var(--sy-text-2);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/*
 * Подсветка — только у невыбранных. У выбранного своя заливка, и вторая поверх
 * неё читалась бы как «выбраны оба».
 */
.sy-select__option--active:not(.sy-select__option--on),
.sy-select__option:hover:not(.sy-select__option--on) {
  background: var(--sy-surface-2);
}

.sy-select__option--on {
  background: var(--sy-accent-quiet);
}

.sy-select__option--on .sy-select__option-label {
  color: var(--sy-accent);
  font-weight: var(--sy-weight-semibold);
}

.sy-select__rule {
  height: 1px;
  margin: var(--sy-space-1) var(--sy-space-2);
  background: var(--sy-border);
}

/*
 * Содержимое слота приходит из вызывающего компонента и своей вёрстки не несёт:
 * ссылка в подвале списка везде выглядит одинаково, и решать это должен список.
 */
.sy-select__footer :deep(button) {
  width: 100%;
  height: 32px;
  padding: 0 var(--sy-space-4);
  border: none;
  border-radius: var(--sy-radius-inner);
  background: transparent;
  color: var(--sy-accent);
  font-family: inherit;
  font-size: var(--sy-text-small);
  text-align: left;
  cursor: pointer;
}

.sy-select__footer :deep(button):hover {
  background: var(--sy-surface-2);
}

.sy-select__note {
  font-size: var(--sy-text-caption);
  line-height: 1.45;
  color: var(--sy-text-3);
}
</style>

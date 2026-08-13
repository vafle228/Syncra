<script setup lang="ts">
import { computed, ref, useId } from 'vue'

/**
 * Поле ввода (F2).
 *
 * Метка сверху — моноширинная: она читается как служебная подпись и не спорит
 * со значением. Для `type="password"` есть кнопка «показать»: это переключение
 * маски НАД СОБСТВЕННЫМ вводом пользователя — секретов ядра здесь нет и быть
 * не может (значения из `get_secret` показывает отдельный компонент F5).
 */

const props = withDefaults(
  defineProps<{
    modelValue: string
    label?: string
    type?: 'text' | 'password' | 'email' | 'url'
    placeholder?: string
    /** Подсказка под полем. Показывается, когда нет `error`. */
    hint?: string
    /** Текст ошибки. Красит рамку и заменяет подсказку. */
    error?: string | null
    /** Моноширинное значение — для того, что читают по символам. */
    mono?: boolean
    disabled?: boolean
    autocomplete?: string
    autofocus?: boolean
    /** Показывать кнопку «показать/скрыть» у пароля. */
    revealable?: boolean
    /**
     * Спрятать метку визуально, оставив её для скринридера.
     *
     * Нужно там, где подпись очевидна из соседства (пронумерованный список
     * адресов), но поле всё равно обязано быть подписанным: «текстовое поле» без
     * имени — это то, что слышит человек с экранным диктором.
     */
    labelHidden?: boolean
  }>(),
  {
    type: 'text',
    mono: false,
    disabled: false,
    autofocus: false,
    revealable: true,
    labelHidden: false,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: string]
  submit: []
}>()

const inputId = useId()
const describedById = `${inputId}-note`

const revealed = ref(false)

const isPassword = computed(() => props.type === 'password')
const showReveal = computed(() => isPassword.value && props.revealable && !props.disabled)
const effectiveType = computed(() => (isPassword.value && revealed.value ? 'text' : props.type))
const note = computed(() => props.error ?? props.hint ?? null)

function onInput(event: Event): void {
  emit('update:modelValue', (event.target as HTMLInputElement).value)
}
</script>

<template>
  <div class="sy-input" :class="{ 'sy-input--error': Boolean(error) }">
    <label
      v-if="label"
      class="sy-input__label"
      :class="{ 'sy-input__label--hidden': labelHidden }"
      :for="inputId"
      >{{ label }}</label
    >

    <div class="sy-input__box">
      <input
        :id="inputId"
        class="sy-input__control"
        :class="{ 'sy-input__control--mono': mono || isPassword }"
        :type="effectiveType"
        :value="modelValue"
        :placeholder="placeholder"
        :disabled="disabled"
        :autocomplete="autocomplete"
        :autofocus="autofocus"
        :aria-invalid="error ? 'true' : undefined"
        :aria-describedby="note ? describedById : undefined"
        @input="onInput"
        @keydown.enter="emit('submit')"
      />

      <button
        v-if="showReveal"
        type="button"
        class="sy-input__reveal"
        :aria-pressed="revealed"
        :title="revealed ? 'Скрыть' : 'Показать'"
        @click="revealed = !revealed"
      >
        {{ revealed ? 'Скрыть' : 'Показать' }}
      </button>
    </div>

    <p v-if="note" :id="describedById" class="sy-input__note">{{ note }}</p>
  </div>
</template>

<style scoped>
.sy-input {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.sy-input__label {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

/*
 * Скрыта для глаза, но не для скринридера: `display: none` убрал бы её и из
 * дерева доступности, оставив поле без имени.
 */
.sy-input__label--hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
}

.sy-input--error .sy-input__label {
  color: var(--sy-danger);
}

.sy-input__box {
  display: flex;
  align-items: center;
  gap: var(--sy-space-3);
  height: var(--sy-control-height-field);
  padding: 0 var(--sy-space-2) 0 var(--sy-space-5);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  transition:
    border-color var(--sy-transition),
    box-shadow var(--sy-transition);
}

.sy-input__box:focus-within {
  border-color: var(--sy-accent);
  box-shadow: var(--sy-focus-ring);
}

.sy-input--error .sy-input__box {
  border-color: var(--sy-danger);
  background: var(--sy-danger-quiet);
}

.sy-input__control {
  flex: 1;
  min-width: 0;
  height: 100%;
  border: none;
  background: transparent;
  font-size: 14px;
  outline: none;
}

.sy-input__control--mono {
  font-family: var(--sy-font-mono);
  letter-spacing: 0.08em;
}

.sy-input__control::placeholder {
  color: var(--sy-text-3);
  letter-spacing: normal;
}

.sy-input__control:disabled {
  color: var(--sy-text-3);
  cursor: not-allowed;
}

.sy-input__reveal {
  flex: none;
  height: var(--sy-control-height-sm);
  padding: 0 var(--sy-space-4);
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius-xs);
  background: var(--sy-surface-2);
  color: var(--sy-text);
  font-size: var(--sy-text-small);
  cursor: pointer;
}

.sy-input__reveal:hover {
  border-color: var(--sy-accent);
}

.sy-input__note {
  font-size: 11.5px;
  line-height: 1.45;
  color: var(--sy-text-3);
}

.sy-input--error .sy-input__note {
  color: var(--sy-danger);
}
</style>

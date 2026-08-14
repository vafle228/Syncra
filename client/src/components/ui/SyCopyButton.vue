<script setup lang="ts">
import { computed } from 'vue'

/**
 * Кнопка копирования (F5, §4.5 макета).
 *
 * Главное решение макета: таймер очистки буфера вписан В КНОПКУ, а не спрятан
 * в тост. Тост уедет через три секунды, а знать, что пароль ещё лежит в буфере,
 * пользователю нужно все двадцать.
 *
 * ЗАКОН №1: компонент принимает только подпись и состояние. Копируемого
 * значения у него нет — его берёт и сразу отпускает вызывающая сторона.
 */

const props = withDefaults(
  defineProps<{
    /** Подпись в покое, напр. «Копировать пароль». */
    label: string
    /** Скопировано только что — показываем отсчёт. */
    copied?: boolean
    /** Секунд до очистки буфера. `0` — очистки не будет, отсчёт не рисуем. */
    seconds?: number
    /** Копировать нечего (пустое поле). */
    empty?: boolean
    /** Буфер обмена недоступен: небезопасный контекст, нет разрешения. */
    unavailable?: boolean
    busy?: boolean
    /** Только иконка — для тесных мест (строка метаданных). */
    compact?: boolean
    /** Основное действие экрана: заливка вместо обводки. */
    primary?: boolean
  }>(),
  {
    copied: false,
    seconds: 0,
    empty: false,
    unavailable: false,
    busy: false,
    compact: false,
    primary: false,
  },
)

defineEmits<{ click: [] }>()

const disabled = computed(() => props.empty || props.unavailable || props.busy)

const text = computed(() => {
  if (props.unavailable) return 'Буфер недоступен'
  if (props.empty) return 'Нечего копировать'
  // «с» из макета (`Прототип:2904`): без единицы число читается как счётчик
  // копий, а не как «сколько ещё лежит в буфере».
  if (props.copied) return props.seconds > 0 ? `Скопировано · ${props.seconds} с` : 'Скопировано'
  return props.label
})

/** Подпись для иконочного варианта — она уходит в `title` и в скринридер. */
const hint = computed(() => (props.compact && props.copied ? text.value : props.label))
</script>

<template>
  <button
    type="button"
    class="sy-copy"
    :class="{
      'sy-copy--copied': copied,
      'sy-copy--compact': compact,
      'sy-copy--primary': primary && !copied && !disabled,
      'sy-copy--error': unavailable,
    }"
    :disabled="disabled"
    :title="compact ? hint : undefined"
    :aria-label="compact ? hint : undefined"
    @click="$emit('click')"
  >
    <span v-if="copied" class="sy-copy__done" aria-hidden="true"><span /></span>
    <span v-else class="sy-copy__mark" aria-hidden="true" />

    <span v-if="!compact" class="sy-copy__text">{{ text }}</span>
    <span v-else-if="copied && seconds > 0" class="sy-copy__tick">{{ seconds }}</span>
  </button>
</template>

<style scoped>
.sy-copy {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--sy-space-3);
  height: var(--sy-control-height-sm);
  padding: 0 11px;
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius-xs);
  background: var(--sy-surface);
  color: var(--sy-text-2);
  font-family: var(--sy-font-sans);
  font-size: var(--sy-text-small);
  font-weight: var(--sy-weight-medium);
  white-space: nowrap;
  cursor: pointer;
  transition:
    background var(--sy-transition),
    border-color var(--sy-transition),
    color var(--sy-transition);
}

.sy-copy:hover:not(:disabled) {
  background: var(--sy-surface-2);
  color: var(--sy-text);
}

.sy-copy:disabled {
  border-color: var(--sy-border);
  background: transparent;
  color: var(--sy-text-3);
  cursor: not-allowed;
}

.sy-copy--primary {
  border-color: transparent;
  background: var(--sy-accent);
  color: var(--sy-accent-fg);
  font-weight: var(--sy-weight-semibold);
}

.sy-copy--primary:hover:not(:disabled) {
  background: var(--sy-accent);
  color: var(--sy-accent-fg);
  filter: brightness(1.08);
}

.sy-copy--copied {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
  color: var(--sy-accent);
}

.sy-copy--copied:hover:not(:disabled) {
  background: var(--sy-accent-quiet);
  color: var(--sy-accent);
}

.sy-copy--error:disabled {
  border-color: var(--sy-danger);
  background: var(--sy-danger-quiet);
  color: var(--sy-danger);
}

.sy-copy--compact {
  width: 32px;
  padding: 0;
}

.sy-copy--compact.sy-copy--copied {
  width: auto;
  padding: 0 var(--sy-space-3);
}

.sy-copy__mark {
  flex: none;
  width: 12px;
  height: 12px;
  border: 1.5px solid currentColor;
  border-radius: 3px;
}

.sy-copy__done {
  flex: none;
  display: grid;
  place-items: center;
  width: 13px;
  height: 13px;
  border: 1.5px solid currentColor;
  border-radius: 50%;
}

.sy-copy__done span {
  width: 4.5px;
  height: 4.5px;
  border-radius: 50%;
  background: currentColor;
}

.sy-copy__tick {
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
}
</style>

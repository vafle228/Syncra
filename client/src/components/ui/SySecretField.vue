<script setup lang="ts">
import { computed } from 'vue'

import SyCopyButton from './SyCopyButton.vue'

/**
 * Поле-секрет с reveal (F5, §3.3) — вариант A из дизайн-системы.
 *
 * Три решения макета, которые здесь важнее красоты:
 *  1. Маска фиксированной длины. Десять точек всегда, независимо от пароля:
 *     длина — это тоже сведения о секрете, и подглядывающему через плечо их
 *     знать незачем.
 *  2. Копировать можно, не открывая. Секрет вообще не обязан появляться на
 *     экране — это основной путь, а не запасной.
 *  3. Открытое состояние честно говорит, что закроется само и когда.
 *
 * ЗАКОН №1: компонент — чистое представление. Он не ходит в ядро и не хранит
 * значение: `value` приходит сверху, пока поле открыто, и становится `null`,
 * как только владелец его закрыл.
 */

const props = withDefaults(
  defineProps<{
    label: string
    /** Открытое значение. `null` — поле закрыто (или его нет). */
    value?: string | null
    /** Заполнено ли поле в записи — известно из метаданных, без чтения секрета. */
    present?: boolean
    /** Что написать, когда поле пустое: «Заметок нет», «Не подключён». */
    emptyText?: string
    /** Секунд до авто-скрытия. */
    hideIn?: number
    busy?: boolean
    /** Состояние кнопки копирования. */
    copied?: boolean
    copySeconds?: number
    clipboardUnavailable?: boolean
    /** Многострочное значение (заметки). */
    multiline?: boolean
    /** Пояснение под полем. */
    hint?: string
    /**
     * Подпись кнопки копирования в ЗАКРЫТОМ состоянии. У пароля это «Копировать
     * пароль» (`Прототип:1639`): пока значения на экране нет, из подписи должно
     * быть понятно, что именно уедет в буфер. Открытое поле говорит само за
     * себя, и подпись там всегда короткая.
     */
    copyLabel?: string
    /**
     * Заметки (`Прототип:1682-1689`). Закрытое поле показывает не маску, а две
     * полосы-скелета: у заметки нет «длины пароля», которую надо прятать, — есть
     * только факт, что текст там есть. Копирования у такого поля нет: заметку
     * читают, а не вставляют в форму входа, — и весь бокс работает переключателем.
     */
    skeleton?: boolean
  }>(),
  {
    value: null,
    present: true,
    emptyText: 'Пусто',
    hideIn: 0,
    busy: false,
    copied: false,
    copySeconds: 0,
    clipboardUnavailable: false,
    multiline: false,
    copyLabel: 'Копировать',
    skeleton: false,
  },
)

const emit = defineEmits<{ toggle: []; copy: [] }>()

const revealed = computed(() => props.value !== null)

/** Длина маски постоянна и не выдаёт длину пароля. */
const MASK = '••••••••••'
</script>

<template>
  <div class="sy-secret" :class="{ 'sy-secret--open': revealed }">
    <span class="sy-secret__label">{{ label }}</span>

    <div v-if="!present" class="sy-secret__empty">{{ emptyText }}</div>

    <template v-else>
      <!--
        Вариант со скелетом: одно действие на всё поле, поэтому переключателем
        работает сам бокс. Вложенных кнопок в нём нет — ни копирования, ни
        отдельного «Показать».
      -->
      <button
        v-if="skeleton"
        type="button"
        class="sy-secret__box sy-secret__box--button"
        :class="{
          'sy-secret__box--open': revealed,
          'sy-secret__box--multiline': multiline && revealed,
        }"
        :disabled="busy"
        :aria-pressed="revealed"
        @click="emit('toggle')"
      >
        <span v-if="revealed" class="sy-secret__value sy-secret__value--wrap">{{ value }}</span>
        <template v-else>
          <span class="sy-secret__bars" aria-hidden="true">
            <span class="sy-secret__bar" />
            <span class="sy-secret__bar sy-secret__bar--short" />
          </span>
          <span class="sy-secret__chip">Показать</span>
        </template>
      </button>

      <div
        v-else
        class="sy-secret__box"
        :class="{
          'sy-secret__box--open': revealed,
          'sy-secret__box--multiline': multiline && revealed,
        }"
      >
        <span
          v-if="revealed"
          class="sy-secret__value"
          :class="{ 'sy-secret__value--wrap': multiline }"
          >{{ value }}</span
        >
        <span v-else class="sy-secret__mask" aria-hidden="true">{{ MASK }}</span>

        <span class="sy-secret__actions">
          <button
            type="button"
            class="sy-secret__toggle"
            :disabled="busy"
            :aria-pressed="revealed"
            @click="emit('toggle')"
          >
            {{ revealed ? 'Скрыть' : 'Показать' }}
          </button>

          <SyCopyButton
            :label="revealed ? 'Копировать' : copyLabel"
            :copied="copied"
            :seconds="copySeconds"
            :unavailable="clipboardUnavailable"
            :busy="busy"
            :primary="!revealed"
            @click="emit('copy')"
          />
        </span>
      </div>

      <p v-if="revealed" class="sy-secret__note sy-secret__note--open">
        Открыт · скроется автоматически через {{ hideIn }} с
      </p>
      <p v-else class="sy-secret__note">
        {{
          hint ??
          (skeleton ? 'Скрыт · откроется по нажатию' : 'Скрыт · копировать можно, не открывая')
        }}
      </p>
    </template>
  </div>
</template>

<style scoped>
.sy-secret {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
  min-width: 0;
}

.sy-secret__label {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.sy-secret__empty {
  display: flex;
  align-items: center;
  min-height: 46px;
  padding: 0 var(--sy-space-6);
  border: 1px dashed var(--sy-border-strong);
  border-radius: var(--sy-radius-sm);
  font-size: var(--sy-text-body);
  color: var(--sy-text-3);
}

.sy-secret__box {
  display: flex;
  align-items: center;
  gap: var(--sy-space-3);
  min-height: 46px;
  padding: 0 var(--sy-space-2) 0 var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
}

.sy-secret__box--open {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
}

/* Бокс-переключатель: кнопка, которая выглядит ровно как обычный бокс. */
.sy-secret__box--button {
  width: 100%;
  justify-content: space-between;
  gap: var(--sy-space-5);
  font: inherit;
  color: inherit;
  text-align: left;
  cursor: pointer;
}

.sy-secret__box--button:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

.sy-secret__box--button:disabled {
  cursor: progress;
}

/*
 * Скелет вместо маски. Точки говорили бы про длину, которой у заметки нет;
 * две полосы говорят ровно то, что известно из метаданных: текст здесь есть.
 */
.sy-secret__bars {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.sy-secret__bar {
  width: 70%;
  height: 8px;
  border-radius: 3px;
  background: var(--sy-surface-2);
}

.sy-secret__bar--short {
  width: 45%;
}

.sy-secret__chip {
  flex: none;
  display: grid;
  place-items: center;
  height: 32px;
  padding: 0 11px;
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius-inner);
  background: var(--sy-surface-2);
  color: var(--sy-text);
  font-size: var(--sy-text-small);
}

.sy-secret__box--multiline {
  align-items: flex-start;
  padding-top: var(--sy-space-4);
  padding-bottom: var(--sy-space-4);
}

.sy-secret__mask {
  flex: 1;
  min-width: 0;
  font-family: var(--sy-font-mono);
  font-size: 16px;
  letter-spacing: 0.22em;
  color: var(--sy-text-2);
}

.sy-secret__value {
  flex: 1;
  min-width: 0;
  font-family: var(--sy-font-mono);
  font-size: 15px;
  letter-spacing: 0.02em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sy-secret__value--wrap {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  line-height: 1.5;
}

.sy-secret__actions {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--sy-space-2);
}

.sy-secret__toggle {
  height: var(--sy-control-height-sm);
  padding: 0 11px;
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius-xs);
  background: var(--sy-surface-2);
  color: var(--sy-text);
  font-family: var(--sy-font-sans);
  font-size: var(--sy-text-small);
  cursor: pointer;
  transition: border-color var(--sy-transition);
}

.sy-secret__toggle:hover:not(:disabled) {
  border-color: var(--sy-accent);
}

.sy-secret__box--open .sy-secret__toggle {
  border-color: var(--sy-accent-border);
  background: transparent;
  color: var(--sy-accent);
}

.sy-secret__toggle:disabled {
  color: var(--sy-text-3);
  cursor: not-allowed;
}

.sy-secret__note {
  font-size: 11.5px;
  line-height: 1.45;
  color: var(--sy-text-3);
}

.sy-secret__note--open {
  color: var(--sy-accent);
}
</style>

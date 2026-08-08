<script setup lang="ts">
import { useWindowControls } from '@/composables/useWindowControls'

/**
 * Кнопки окна из прототипа: свернуть, развернуть, закрыть (F13).
 *
 * В прототипе это три нарисованных `<div>` без обработчиков. Здесь — настоящие
 * `<button>` с `aria-label`: пиксели прототипа это спецификация, его DOM — нет.
 * Иначе полосу заголовка нельзя было бы пройти с клавиатуры, а скринридер
 * прочитал бы три пустых блока.
 *
 * Глифы собраны из `<span>` с рамками, а не из шрифта или SVG: ни один пиксель
 * интерфейса не должен зависеть от загрузки чего-то из сети (см. `CLAUDE.md`).
 */

const controls = useWindowControls()
</script>

<template>
  <div class="win">
    <button
      type="button"
      class="win__button"
      aria-label="Свернуть окно"
      data-test="window-min"
      @click="controls.minimize()"
    >
      <span class="win__minimize" aria-hidden="true" />
    </button>

    <button
      type="button"
      class="win__button"
      aria-label="Развернуть окно"
      data-test="window-max"
      @click="controls.toggleMaximize()"
    >
      <span class="win__maximize" aria-hidden="true" />
    </button>

    <button
      type="button"
      class="win__button win__button--close"
      aria-label="Закрыть окно"
      data-test="window-close"
      @click="controls.close()"
    >
      <span class="win__close" aria-hidden="true" />
    </button>
  </div>
</template>

<style scoped>
.win {
  display: flex;
  align-items: center;
  gap: var(--sy-space-2);
  /* Полоса заголовка тянется мышью — кнопки из этой зоны исключены. */
  -webkit-app-region: no-drag;
}

.win__button {
  display: grid;
  place-items: center;
  width: 26px;
  height: 22px;
  padding: 0;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: var(--sy-text-3);
  cursor: pointer;
  transition: background var(--sy-transition);
}

.win__button:hover {
  background: var(--sy-surface-2);
  color: var(--sy-text);
}

/* Закрытие подсвечивается красным: единственная кнопка рамки с последствиями. */
.win__button--close:hover {
  background: var(--sy-danger);
  color: var(--sy-danger-fg);
}

.win__minimize {
  width: 10px;
  height: 1.5px;
  background: currentColor;
}

.win__maximize {
  width: 9px;
  height: 9px;
  border: 1.5px solid currentColor;
  border-radius: 2px;
}

.win__close {
  position: relative;
  width: 11px;
  height: 11px;
}

.win__close::before,
.win__close::after {
  content: '';
  position: absolute;
  top: 5px;
  left: -1px;
  width: 13px;
  height: 1.5px;
  background: currentColor;
}

.win__close::before {
  transform: rotate(45deg);
}

.win__close::after {
  transform: rotate(-45deg);
}
</style>

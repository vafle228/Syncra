<script setup lang="ts">
import { computed } from 'vue'

import type { QrMatrix } from '@/core/contract'

/**
 * Рисовалка QR-кода (F8, §3.6 макета).
 *
 * Компонент ничего не кодирует: матрицу считает ядро, здесь она только
 * превращается в квадратики. Так пейлоад сопряжения — а в нём одноразовый ключ
 * сеанса — не существует во фронте отдельной строкой.
 *
 * Экранному диктору код бесполезен, поэтому картинка помечена как декоративная,
 * а доступная альтернатива — шестизначный код рядом: его можно прочитать,
 * продиктовать и набрать руками.
 */

const props = defineProps<{
  matrix: QrMatrix
  /** Код уже недействителен: гасим, но с экрана не убираем. */
  stale?: boolean
}>()

/** Плоский список модулей: сетка раскладывает его сама по `size` в строке. */
const modules = computed(() => props.matrix.rows.flatMap((row) => [...row]))
</script>

<template>
  <div class="qr" :class="{ 'qr--stale': stale }">
    <div
      class="qr__grid"
      :style="{ '--qr-modules': matrix.size }"
      role="presentation"
      aria-hidden="true"
    >
      <span
        v-for="(module, index) in modules"
        :key="index"
        class="qr__module"
        :class="{ 'qr__module--dark': module === '1' }"
      />
    </div>
  </div>
</template>

<style scoped>
.qr {
  /* Светлое поле вокруг кода — часть кода: без него камера не находит границы. */
  padding: var(--sy-space-7);
  border-radius: var(--sy-radius-lg);
  background: var(--sy-qr-bg);
  box-shadow: var(--sy-shadow);
  transition: opacity var(--sy-transition);
}

.qr--stale {
  opacity: 0.32;
}

.qr__grid {
  display: grid;
  grid-template-columns: repeat(var(--qr-modules), 9px);
  grid-auto-rows: 9px;
}

.qr__module {
  width: 9px;
  height: 9px;
}

.qr__module--dark {
  background: var(--sy-qr-ink);
}

@media (max-width: 1100px) {
  .qr__grid {
    grid-template-columns: repeat(var(--qr-modules), 7px);
    grid-auto-rows: 7px;
  }

  .qr__module {
    width: 7px;
    height: 7px;
  }
}
</style>

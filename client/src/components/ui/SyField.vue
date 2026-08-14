<script setup lang="ts">
/**
 * Поле с действием сбоку (F5 — «Показать текущий» рядом с паролем и заметками).
 *
 * Задача одна: поставить кнопку вровень с рамкой поля, а не с его подписью.
 * Раньше это делал `margin-top: 22px` — сумма высоты подписи и отступа под ней,
 * посчитанная руками и повторённая в трёх местах. Числу неоткуда узнать, что
 * подпись стала на строку выше, и разъехалось бы оно молча.
 *
 * Здесь вместо этого сетка: подпись — первая строка, контрол и действие — вторая.
 * Кнопка стоит вровень с полем потому, что стоит с ним в одной строке.
 *
 * Подпись — настоящий `<label>` с `display: contents`: он не создаёт своей
 * коробки (дети остаются ячейками сетки), но связывает подпись с первым
 * контролом внутри. Поэтому оборачиваемому `SyInput` подпись передавать не надо
 * — иначе их станет две.
 */

withDefaults(
  defineProps<{
    label: string
    /** Ошибка в поле — красим подпись, как это делает `SyInput` со своей. */
    invalid?: boolean
  }>(),
  { invalid: false },
)
</script>

<template>
  <div class="sy-field">
    <label class="sy-field__labelled">
      <span class="sy-field__label" :class="{ 'sy-field__label--invalid': invalid }">
        {{ label }}
      </span>
      <span class="sy-field__control"><slot /></span>
    </label>

    <!--
      Действие в строке ПОДПИСИ, а не рядом с рамкой: так оно читается как часть
      поля, а не как отдельная кнопка формы, и не отбирает у поля ширину.
    -->
    <div v-if="$slots.aside" class="sy-field__aside"><slot name="aside" /></div>

    <div v-if="$slots.action" class="sy-field__action"><slot name="action" /></div>
  </div>
</template>

<style scoped>
.sy-field {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: var(--sy-space-2) var(--sy-space-4);
  align-items: start;
}

/* Не создаёт коробки: подпись и контрол остаются ячейками сетки выше. */
.sy-field__labelled {
  display: contents;
}

.sy-field__label {
  grid-column: 1;
  grid-row: 1;
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  line-height: var(--sy-text-label-lh);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.sy-field__label--invalid {
  color: var(--sy-danger);
}

.sy-field__control {
  grid-column: 1;
  grid-row: 2;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.sy-field__aside {
  grid-column: 2;
  grid-row: 1;
  align-self: center;
}

/* Та же строка, что и у контрола, — отсюда и выравнивание. */
.sy-field__action {
  grid-column: 2;
  grid-row: 2;
}
</style>

<script setup lang="ts">
/**
 * Кнопка (F2).
 *
 * Правила из макета:
 *  - одна `primary` на экран;
 *  - опасное действие — не красная заливка, а обводка: её нужно прочитать,
 *    а не нажать по привычке. Заливается только на ховере.
 */

withDefaults(
  defineProps<{
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger'
    /**
     * Высота. `header` — 34px из шапок панелей прототипа: на ладдере 30/36/42
     * такой ступени не было, поэтому каждая шапка рисовала её числом у себя.
     */
    size?: 'md' | 'sm' | 'lg' | 'header'
    type?: 'button' | 'submit'
    disabled?: boolean
    loading?: boolean
    /** Растянуть по ширине родителя. */
    block?: boolean
    /**
     * Кнопка под один глиф: квадрат со стороной в высоту выбранного размера.
     * Без этого пропа такие кнопки собирались вручную в каждом компоненте — и
     * разъезжались по высоте с соседями (в шапке карточки 30px против 34px).
     */
    icon?: boolean
  }>(),
  {
    variant: 'secondary',
    size: 'md',
    type: 'button',
    disabled: false,
    loading: false,
    block: false,
    icon: false,
  },
)
</script>

<template>
  <button
    :type="type"
    :class="[
      'sy-button',
      `sy-button--${variant}`,
      `sy-button--${size}`,
      { 'sy-button--block': block, 'sy-button--icon': icon },
    ]"
    :disabled="disabled || loading"
    :aria-busy="loading || undefined"
  >
    <span v-if="loading" class="sy-button__spinner" aria-hidden="true" />
    <slot />
  </button>
</template>

<style scoped>
.sy-button {
  /*
   * Высота живёт в локальной переменной, а не только в `height`: иконочный
   * вариант делает из неё же ширину, и два числа рядом рано или поздно
   * разъехались бы. Префикс `--sy-` тут нарочно не берём — он за токенами.
   */
  --button-height: var(--sy-control-height);

  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--sy-space-3);
  height: var(--button-height);
  padding: 0 var(--sy-space-6);
  border-radius: var(--sy-radius-sm);
  border: 1px solid transparent;
  font-family: var(--sy-font-sans);
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-medium);
  white-space: nowrap;
  cursor: pointer;
  transition:
    background var(--sy-transition),
    border-color var(--sy-transition),
    color var(--sy-transition);
}

.sy-button--sm {
  --button-height: var(--sy-control-height-sm);

  padding: 0 var(--sy-space-5);
  font-size: var(--sy-text-small);
}

/* Действие в шапке панели: 34px, подпись 13px — размеры прототипа. */
.sy-button--header {
  --button-height: var(--sy-control-height-md);

  padding: 0 14px;
  font-size: var(--sy-text-note);
}

.sy-button--lg {
  --button-height: var(--sy-control-height-lg);

  padding: 0 var(--sy-space-7);
  font-size: 14px;
}

.sy-button--block {
  width: 100%;
}

.sy-button--icon {
  width: var(--button-height);
  padding: 0;
}

/*
 * Глиф тише подписи: в макете «Изменить» набрано основным цветом текста, а
 * соседнее «···» — вторым. Разница читается как «это действие, а это ещё
 * действия», и держать её стоит на уровне варианта, а не каждого места.
 */
.sy-button--icon.sy-button--secondary {
  color: var(--sy-text-2);
}

.sy-button:disabled {
  cursor: not-allowed;
}

/* primary */
.sy-button--primary {
  background: var(--sy-accent);
  color: var(--sy-accent-fg);
  font-weight: var(--sy-weight-semibold);
}

.sy-button--primary:hover:not(:disabled) {
  filter: brightness(1.08);
}

.sy-button--primary:active:not(:disabled) {
  filter: brightness(0.94);
}

.sy-button--primary:disabled {
  background: var(--sy-surface-2);
  color: var(--sy-text-3);
  filter: none;
}

/* secondary */
.sy-button--secondary {
  border-color: var(--sy-border-strong);
  background: var(--sy-surface);
  color: var(--sy-text);
}

.sy-button--secondary:hover:not(:disabled) {
  background: var(--sy-surface-2);
}

.sy-button--secondary:disabled {
  border-color: var(--sy-border);
  background: transparent;
  color: var(--sy-text-3);
}

/* ghost */
.sy-button--ghost {
  background: transparent;
  color: var(--sy-text-2);
}

.sy-button--ghost:hover:not(:disabled) {
  background: var(--sy-surface);
  color: var(--sy-text);
}

.sy-button--ghost:disabled {
  color: var(--sy-text-3);
}

/* danger — обводка, заливка только на ховере */
.sy-button--danger {
  border-color: var(--sy-danger);
  background: var(--sy-danger-quiet);
  color: var(--sy-danger);
  font-weight: var(--sy-weight-semibold);
}

.sy-button--danger:hover:not(:disabled) {
  background: var(--sy-danger);
  color: var(--sy-danger-fg);
}

.sy-button--danger:disabled {
  border-color: var(--sy-border);
  background: transparent;
  color: var(--sy-text-3);
}

.sy-button__spinner {
  width: 13px;
  height: 13px;
  border: 1.5px solid currentColor;
  border-top-color: transparent;
  border-radius: 50%;
  animation: sy-button-spin 0.7s linear infinite;
}

@keyframes sy-button-spin {
  to {
    transform: rotate(360deg);
  }
}

@media (prefers-reduced-motion: reduce) {
  .sy-button__spinner {
    animation-duration: 2.4s;
  }
}
</style>

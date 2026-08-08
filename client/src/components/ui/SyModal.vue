<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from 'vue'

/**
 * Модальный диалог (F2).
 *
 * Из макета: заголовок — это вопрос или факт, а не «Внимание!». Риск
 * объясняется словами и обводкой (`tone`), а не восклицательным знаком.
 */

const props = withDefaults(
  defineProps<{
    open: boolean
    title: string
    /** Окраска рамки предупреждения над содержимым. */
    tone?: 'neutral' | 'warning' | 'danger'
    /** Текст предупреждения. Формулировки из спека НЕ смягчать. */
    warning?: string
    /** Запретить закрытие по Escape и клику по подложке. */
    persistent?: boolean
    /**
     * Ширина по НАЗНАЧЕНИЮ, а не по пикселям: размер диалога — следствие того,
     * что в нём делают, и подбирать его на глаз в каждом месте нельзя.
     *
     * - `default` (460) — короткий вопрос;
     * - `confirm` (480) — подтверждение необратимого действия: заголовок,
     *   объяснение цены и две кнопки;
     * - `form` (520) — диалог с полями ввода;
     * - `wizard` (560) — многошаговый сценарий (сопряжение, импорт);
     * - `wide` (860) — сравнение двух колонок (конфликт версий, F11).
     *
     * Шире делать без причины не нужно: чем длиннее строка, тем хуже читается
     * вопрос.
     */
    size?: 'default' | 'confirm' | 'form' | 'wizard' | 'wide'
  }>(),
  { tone: 'neutral', persistent: false, size: 'default' },
)

const emit = defineEmits<{ close: [] }>()

const dialog = ref<HTMLElement | null>(null)

function requestClose(): void {
  if (!props.persistent) emit('close')
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape') requestClose()
}

watch(
  () => props.open,
  async (open) => {
    const doc = globalThis.document
    if (!doc) return

    if (open) {
      doc.addEventListener('keydown', onKeydown)
      await nextTick()
      dialog.value?.focus()
    } else {
      doc.removeEventListener('keydown', onKeydown)
    }
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  globalThis.document?.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="sy-modal" @mousedown.self="requestClose">
      <div
        ref="dialog"
        class="sy-modal__dialog"
        :class="[`sy-modal__dialog--${size}`, `sy-modal__dialog--tone-${tone}`]"
        role="dialog"
        aria-modal="true"
        :aria-label="title"
        tabindex="-1"
      >
        <h2 class="sy-modal__title">{{ title }}</h2>

        <p v-if="warning" class="sy-modal__warning" :class="`sy-modal__warning--${tone}`">
          <span class="sy-modal__dot" aria-hidden="true" />
          <span>{{ warning }}</span>
        </p>

        <div class="sy-modal__body">
          <slot />
        </div>

        <!--
          Сноска слева, кнопки справа: в опасных диалогах прототипа рядом с
          «Удалить» стоит напоминание, что именно эта кнопка и есть подтверждение.
          Пояснение стоит там, где решают, а не выше по тексту.
        -->
        <div v-if="$slots.actions || $slots.note" class="sy-modal__actions">
          <p v-if="$slots.note" class="sy-modal__note"><slot name="note" /></p>
          <div v-if="$slots.actions" class="sy-modal__buttons">
            <slot name="actions" />
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.sy-modal {
  position: fixed;
  inset: 0;
  z-index: 50;
  display: grid;
  place-items: center;
  padding: var(--sy-space-10);
  background: var(--sy-scrim);
  backdrop-filter: blur(3px);
}

.sy-modal__dialog {
  width: 460px;
  max-width: 100%;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  padding: 26px;
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius-lg);
  background: var(--sy-surface);
  box-shadow: var(--sy-shadow-window-2);
  animation: sy-in 0.18s ease-out;
}

.sy-modal__dialog--confirm {
  width: 480px;
}

.sy-modal__dialog--form {
  width: 520px;
}

.sy-modal__dialog--wizard {
  width: 560px;
}

.sy-modal__dialog--wide {
  width: 860px;
}

/*
 * Опасный диалог обведён красным целиком, а не только полоской внутри: то, что
 * этот вопрос про потерю данных, должно быть видно раньше, чем прочитан текст.
 */
.sy-modal__dialog--tone-danger {
  border-color: var(--sy-danger);
  box-shadow: var(--sy-shadow-window-2), var(--sy-shadow-danger);
}

.sy-modal__dialog--tone-warning {
  border-color: var(--sy-warn);
}

.sy-modal__dialog:focus {
  outline: none;
}

.sy-modal__title {
  font-size: var(--sy-text-h2);
  line-height: var(--sy-text-h2-lh);
  font-weight: var(--sy-weight-semibold);
}

.sy-modal__warning {
  display: flex;
  gap: 11px;
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  font-size: 13px;
  line-height: 1.5;
  color: var(--sy-text);
}

.sy-modal__warning--warning {
  border-color: var(--sy-warn);
  background: var(--sy-warn-quiet);
}

.sy-modal__warning--danger {
  border-color: var(--sy-danger);
  background: var(--sy-danger-quiet);
}

.sy-modal__dot {
  flex: none;
  width: 7px;
  height: 7px;
  margin-top: 5px;
  border-radius: 50%;
  background: var(--sy-text-3);
}

.sy-modal__warning--warning .sy-modal__dot {
  background: var(--sy-warn);
}

.sy-modal__warning--danger .sy-modal__dot {
  background: var(--sy-danger);
}

.sy-modal__body {
  font-size: var(--sy-text-body);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.sy-modal__actions {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
  /* Без сноски кнопки прижаты вправо; со сноской она встаёт слева от них. */
  justify-content: flex-end;
  padding-top: var(--sy-space-1);
}

.sy-modal__note {
  flex: 1;
  font-size: 12px;
  line-height: 1.4;
  color: var(--sy-text-3);
  text-wrap: pretty;
}

.sy-modal__buttons {
  flex: none;
  display: flex;
  gap: var(--sy-space-4);
}
</style>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'

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
    /** Окраска рамки диалога целиком. */
    tone?: 'neutral' | 'warning' | 'danger'
    /** Текст предупреждения. Формулировки из спека НЕ смягчать. */
    warning?: string
    /**
     * Окраска полоски предупреждения. По умолчанию совпадает с `tone`, но
     * уровней риска в макете два, а не один: диалог удаления обведён красным
     * (`Прототип:2263`) и содержит ЯНТАРНУЮ оговорку (`Прототип:2267`). Красная
     * оговорка внутри красной рамки — это уже не предупреждение, а фон.
     */
    warningTone?: 'neutral' | 'warning' | 'danger'
    /**
     * Где стоит полоска. `top` — предупреждение о том, что человек собирается
     * сделать (экспорт в CSV): его читают ДО текста. `bottom` — оговорка о
     * побочном следствии уже объяснённого действия (`Прототип:2267`): сначала
     * цена решения, потом сноска, иначе сноска перебивает главное.
     */
    warningPlacement?: 'top' | 'bottom'
    /** Запретить закрытие по Escape и клику по подложке. */
    persistent?: boolean
    /**
     * Полосный диалог: шапка с разделителем, прокручиваемое тело и панель
     * кнопок на `--sy-bg-1` (`Прототип:2298-2376`, `2378-2438`). Такую форму
     * держат крупные диалоги — сопряжение, импорт, конфликт версий, — где тело
     * длиннее экрана и кнопки не должны уезжать вместе с ним.
     */
    banded?: boolean
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
  {
    tone: 'neutral',
    persistent: false,
    size: 'default',
    banded: false,
    warningPlacement: 'top',
  },
)

const emit = defineEmits<{ close: [] }>()

const dialog = ref<HTMLElement | null>(null)

/** Полоска предупреждения по умолчанию идёт в тон рамке. */
const stripTone = computed(() => props.warningTone ?? props.tone)

/**
 * Куда вернуть фокус после закрытия. Диалог забирает фокус на себя, и без
 * возврата пользователь после «Отмены» оказывается в начале страницы — в
 * продукте про безопасность это ещё и потеря места в списке записей.
 */
let focusOrigin: HTMLElement | null = null

const FOCUSABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(', ')

function focusable(): HTMLElement[] {
  const root = dialog.value
  if (!root) return []
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (node) => !node.hasAttribute('hidden') && node.getAttribute('aria-hidden') !== 'true',
  )
}

function requestClose(): void {
  if (!props.persistent) emit('close')
}

/**
 * Фокус не выходит за пределы диалога: пока задан вопрос про необратимое
 * действие, Tab не должен уводить в интерфейс за подложкой.
 */
function trapFocus(event: KeyboardEvent): void {
  const root = dialog.value
  if (!root) return

  const items = focusable()
  const active = globalThis.document?.activeElement as HTMLElement | null

  if (items.length === 0) {
    event.preventDefault()
    root.focus()
    return
  }

  const first = items[0]!
  const last = items[items.length - 1]!

  if (!active || !root.contains(active)) {
    event.preventDefault()
    ;(event.shiftKey ? last : first).focus()
  } else if (event.shiftKey && (active === first || active === root)) {
    event.preventDefault()
    last.focus()
  } else if (!event.shiftKey && active === last) {
    event.preventDefault()
    first.focus()
  }
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape') requestClose()
  else if (event.key === 'Tab') trapFocus(event)
}

function restoreFocus(): void {
  const origin = focusOrigin
  focusOrigin = null
  if (origin?.isConnected) origin.focus()
}

watch(
  () => props.open,
  async (open) => {
    const doc = globalThis.document
    if (!doc) return

    if (open) {
      focusOrigin = doc.activeElement as HTMLElement | null
      doc.addEventListener('keydown', onKeydown)
      await nextTick()
      dialog.value?.focus()
    } else {
      doc.removeEventListener('keydown', onKeydown)
      restoreFocus()
    }
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  globalThis.document?.removeEventListener('keydown', onKeydown)
  restoreFocus()
})
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="sy-modal" @mousedown.self="requestClose">
      <div
        ref="dialog"
        class="sy-modal__dialog"
        :class="[
          `sy-modal__dialog--${size}`,
          `sy-modal__dialog--tone-${tone}`,
          { 'sy-modal__dialog--banded': banded },
        ]"
        role="dialog"
        aria-modal="true"
        :aria-label="title"
        tabindex="-1"
      >
        <div class="sy-modal__head">
          <div class="sy-modal__head-row">
            <h2 class="sy-modal__title">{{ title }}</h2>
            <!-- Приписка ВОЗЛЕ заголовка: номер шага — часть вопроса, а не ответ. -->
            <span v-if="$slots['title-aside']" class="sy-modal__title-aside">
              <slot name="title-aside" />
            </span>
            <span v-if="$slots['head-actions']" class="sy-modal__head-actions">
              <slot name="head-actions" />
            </span>
          </div>
          <p v-if="$slots.lead" class="sy-modal__lead"><slot name="lead" /></p>
        </div>

        <div class="sy-modal__content">
          <p
            v-if="warning && warningPlacement === 'top'"
            class="sy-modal__warning"
            :class="`sy-modal__warning--${stripTone}`"
          >
            <span class="sy-modal__dot" aria-hidden="true" />
            <span>{{ warning }}</span>
          </p>

          <div class="sy-modal__body">
            <slot />
          </div>

          <p
            v-if="warning && warningPlacement === 'bottom'"
            class="sy-modal__warning"
            :class="`sy-modal__warning--${stripTone}`"
          >
            <span class="sy-modal__dot" aria-hidden="true" />
            <span>{{ warning }}</span>
          </p>
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

/*
 * Полосный диалог. Отступы уезжают с диалога внутрь полос, и тело получает
 * собственную прокрутку: шапка с вопросом и кнопки ответа остаются на месте,
 * сколько бы содержимого ни было между ними.
 */
.sy-modal__dialog--banded {
  gap: 0;
  padding: 0;
  max-height: 100%;
  overflow: hidden;
}

.sy-modal__dialog--banded .sy-modal__head {
  padding: var(--sy-space-7) var(--sy-space-8);
  border-bottom: 1px solid var(--sy-border);
}

.sy-modal__dialog--banded .sy-modal__content {
  padding: var(--sy-space-7) var(--sy-space-8);
  min-height: 0;
  overflow-y: auto;
}

.sy-modal__dialog--banded .sy-modal__actions {
  padding: var(--sy-space-6) var(--sy-space-8);
  border-top: 1px solid var(--sy-border);
  background: var(--sy-bg-1);
}

.sy-modal__head {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.sy-modal__head-row {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
}

.sy-modal__title-aside {
  flex: 1;
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-meta);
  color: var(--sy-text-3);
}

.sy-modal__head-actions {
  flex: none;
  margin-left: auto;
}

.sy-modal__content {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
}

.sy-modal__lead {
  font-size: var(--sy-text-note);
  line-height: 1.5;
  color: var(--sy-text-2);
  text-wrap: pretty;
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

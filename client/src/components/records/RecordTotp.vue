<script setup lang="ts">
import { computed } from 'vue'

/**
 * Код подтверждения в карточке записи (`Прототип:1652-1673`).
 *
 * Почему это отдельный компонент, а не ещё одно `SySecretField`: здесь другой
 * предмет. У поля-секрета значение постоянное и его прячут; у кода значение
 * живёт секунды и само сменяется, поэтому рядом с ним стоит отсчёт и кольцо, а
 * группировка 3+3 существует ради того, чтобы шесть цифр можно было перенести
 * в чужую форму, не сбившись.
 *
 * ЗАКОН №1: компонент — чистое представление. Он не ходит в ядро и ничего не
 * хранит: `code` приходит сверху ровно на время показа.
 */

const props = withDefaults(
  defineProps<{
    /** Подключён ли ключ — известно из метаданных, без чтения секрета. */
    present?: boolean
    /** Показанный код. `null` — закрыт. */
    code?: string | null
    secondsLeft?: number
    periodS?: number
    busy?: boolean
  }>(),
  { present: true, code: null, secondsLeft: 0, periodS: 30, busy: false },
)

const emit = defineEmits<{ toggle: [] }>()

const revealed = computed(() => props.code !== null)

/**
 * 3+3 с одним пробелом. Шесть цифр подряд человек читает хуже, чем две тройки,
 * — а вводить их приходится глядя то на экран, то в чужое поле.
 */
const grouped = computed(() => {
  const code = props.code ?? ''
  const half = Math.ceil(code.length / 2)
  return `${code.slice(0, half)} ${code.slice(half)}`.trim()
})

/** Доля окна, которую кольцо уже прошло. */
const sweep = computed(() => {
  const period = props.periodS > 0 ? props.periodS : 30
  const left = Math.min(Math.max(props.secondsLeft, 0), period)
  return `${((period - left) / period) * 360}deg`
})
</script>

<template>
  <div class="totp">
    <span class="totp__label">Код TOTP</span>

    <div v-if="!present" class="totp__empty">Не подключён</div>

    <!-- Весь бокс — переключатель: отдельная кнопка внутри 280px только мешала бы. -->
    <button
      v-else
      type="button"
      class="totp__box"
      :class="{ 'totp__box--open': revealed }"
      :disabled="busy"
      :aria-pressed="revealed"
      @click="emit('toggle')"
    >
      <template v-if="revealed">
        <span class="totp__code">{{ grouped }}</span>
        <span class="totp__timer">
          <span class="totp__seconds">{{ secondsLeft }} с</span>
          <span
            class="totp__ring"
            :style="{ '--totp-sweep': sweep }"
            :title="`Код обновится через ${secondsLeft} с`"
          />
        </span>
      </template>
      <template v-else>
        <span class="totp__mask" aria-hidden="true">••• •••</span>
        <span class="totp__chip">Показать</span>
      </template>
    </button>
  </div>
</template>

<style scoped>
.totp {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
  min-width: 0;
}

.totp__label {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-meta);
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.totp__empty {
  display: flex;
  align-items: center;
  height: 46px;
  padding: 0 var(--sy-space-6);
  border: 1px dashed var(--sy-border-strong);
  border-radius: var(--sy-radius-sm);
  font-size: var(--sy-text-note);
  color: var(--sy-text-3);
}

.totp__box {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  width: 100%;
  height: 46px;
  padding: 0 var(--sy-space-2) 0 var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.totp__box--open {
  padding-right: var(--sy-space-5);
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
}

.totp__box:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

.totp__box:disabled {
  cursor: progress;
}

.totp__code {
  font-family: var(--sy-font-mono);
  font-size: 17px;
  letter-spacing: 0.14em;
}

/* Маска тоже 3+3: закрытое поле не должно менять форму при открытии. */
.totp__mask {
  font-family: var(--sy-font-mono);
  font-size: 16px;
  letter-spacing: 0.22em;
  color: var(--sy-text-2);
}

.totp__timer {
  flex: none;
  display: flex;
  align-items: center;
  gap: 9px;
}

.totp__seconds {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  color: var(--sy-text-2);
}

/*
 * Кольцо отсчёта. Конический градиент под маской-кольцом: так дуга показывает
 * настоящую долю окна, а не просто «что-то крутится», и подложка под ней
 * остаётся прозрачной — заливка бокса сквозь дырку видна как есть.
 *
 * `--totp-sweep` намеренно БЕЗ префикса `--sy-`: это не токен дизайн-системы,
 * а переменная одного компонента, которую задаёт разметка.
 */
.totp__ring {
  flex: none;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: conic-gradient(var(--sy-accent) var(--totp-sweep, 0deg), var(--sy-accent-border) 0);
  mask: radial-gradient(closest-side, transparent 72%, #000 73%);
  -webkit-mask: radial-gradient(closest-side, transparent 72%, #000 73%);
}

.totp__chip {
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
</style>

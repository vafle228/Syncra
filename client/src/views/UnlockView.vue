<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'

import { SyButton, SyInput } from '@/components/ui'
import { PIN_LENGTH } from '@/core/contract'
import { useVaultStore } from '@/stores/useVaultStore'

/**
 * Экран разблокировки (F3, §3.8, F13).
 *
 * ЗАКОН №1: и мастер-пароль, и PIN живут ровно до отправки в ядро — сразу после
 * вызова ref очищается, в стор не попадает ни тот, ни другой.
 *
 * PIN — НЕ второй мастер-пароль. Настоящий ключ один; PIN отпирает его локальное
 * представление на этом устройстве. Поэтому мастер-пароль здесь не «запасной
 * вход», а единственный настоящий: он открывает хранилище и тогда, когда быстрый
 * вход выключен, и тогда, когда попытки исчерпаны.
 *
 * Какой способ показать, решает ЯДРО (`VaultStatus.pin.enrolled`), а не экран:
 * рисовать клавиатуру, которая ничего не отпирает, — обман.
 */

const router = useRouter()
const vault = useVaultStore()

/** Что сейчас на экране. `pin` — только если ядро подтвердило быстрый вход. */
type Mode = 'pin' | 'master'
const mode = ref<Mode>('master')

const masterPassword = ref('')
const capsLock = ref(false)

/** Набранный PIN. Транзит: уходит в ядро и стирается, не дожидаясь ответа. */
const digits = ref('')
/** Сообщение про неверный PIN. Это не ошибка ядра, а промах человека. */
const pinNote = ref<string | null>(null)

const lockNote = computed(() => {
  if (vault.lockReason === 'timeout') return 'Хранилище закрылось само — не было действий.'
  if (vault.lockReason === 'system') return 'Хранилище закрылось: компьютер уходил в сон.'
  return mode.value === 'pin' ? 'Быстрый вход на этом устройстве.' : 'Первый вход после запуска — только паролем.'
})

const canSubmit = computed(() => masterPassword.value.length > 0 && !vault.busy)

/** Клавиши пада: три ряда цифр, потом пусто-0-стереть. */
const keys = computed(() => ['1', '2', '3', '4', '5', '6', '7', '8', '9', '', '0', '⌫'])

/** Точки: заполненные слева, пустые справа. */
const cells = computed(() =>
  Array.from({ length: PIN_LENGTH }, (_, index) => index < digits.value.length),
)

onMounted(async () => {
  vault.clearError()
  // Спрашиваем ядро до первой отрисовки формы: иначе экран моргнёт с пароля
  // на клавиатуру.
  await vault.refresh()
  if (vault.pin.enrolled) mode.value = 'pin'
})

/**
 * Ядро выключило быстрый вход — уводим на пароль.
 *
 * Свою подпись ставим, только если её ещё нет: исчерпанные попытки объясняет
 * `sendPin()` подробнее (он знает, что именно произошло), и перебивать его
 * общей фразой значило бы стереть более точный ответ.
 */
watch(
  () => vault.pin.enrolled,
  (enrolled) => {
    if (enrolled || mode.value !== 'pin') return

    mode.value = 'master'
    pinNote.value ??= 'Быстрый вход отключён. Откройте хранилище мастер-паролем.'
  },
)

function onKeydown(event: KeyboardEvent): void {
  capsLock.value = event.getModifierState?.('CapsLock') ?? false
}

async function submit(): Promise<void> {
  if (!canSubmit.value) return

  const attempt = masterPassword.value
  // Отпускаем пароль до ожидания ответа: он не должен пережить эту функцию.
  masterPassword.value = ''

  try {
    await vault.unlock(attempt)
    await router.push({ name: 'home' })
  } catch {
    // Сообщение уже в `vault.error` — здесь ничего не логируем: в исключении
    // может лежать введённый пароль.
  }
}

/** Нажатие клавиши пада. Полный набор уходит в ядро сам — подтверждать нечего. */
async function press(key: string): Promise<void> {
  if (vault.busy) return

  if (key === '⌫') {
    digits.value = digits.value.slice(0, -1)
    pinNote.value = null
    return
  }
  if (key === '' || digits.value.length >= PIN_LENGTH) return

  pinNote.value = null
  digits.value += key
  if (digits.value.length === PIN_LENGTH) await sendPin()
}

async function sendPin(): Promise<void> {
  const attempt = digits.value
  digits.value = ''

  try {
    const response = await vault.unlockByPin(attempt)
    if (response.ok) {
      await router.push({ name: 'home' })
      return
    }

    // Неверный PIN — ожидаемый исход, а не сбой: говорим, сколько осталось.
    pinNote.value = response.pin_disabled
      ? 'Попытки кончились. Быстрый вход выключен — откройте хранилище мастер-паролем.'
      : `Неверный PIN. Осталось попыток: ${response.attempts_left}.`
  } catch {
    // Сообщение уже в `vault.error`.
  }
}

function toMaster(): void {
  mode.value = 'master'
  digits.value = ''
  vault.clearError()
}

function toPin(): void {
  mode.value = 'pin'
  masterPassword.value = ''
  pinNote.value = null
  vault.clearError()
}
</script>

<template>
  <main class="unlock" @keydown="onKeydown">
    <section class="unlock__panel">
      <div class="unlock__mark" aria-hidden="true"><span /></div>

      <header class="unlock__header">
        <h1 class="unlock__title">{{ mode === 'pin' ? 'Хранилище заперто' : 'Мастер-пароль' }}</h1>
        <p class="unlock__subtitle">{{ lockNote }}</p>
      </header>

      <!-- Быстрый вход: клавиатура. Показывается, только если ядро его знает. -->
      <div v-if="mode === 'pin'" class="unlock__pin" data-test="pin-pad">
        <div class="unlock__cells" role="status" :aria-label="`Введено цифр: ${digits.length}`">
          <span
            v-for="(filled, index) in cells"
            :key="index"
            class="unlock__cell"
            :class="{ 'unlock__cell--filled': filled }"
            aria-hidden="true"
          >
            <span />
          </span>
        </div>

        <div class="unlock__keys">
          <template v-for="(key, index) in keys" :key="index">
            <span v-if="key === ''" class="unlock__key-gap" aria-hidden="true" />
            <button
              v-else
              type="button"
              class="unlock__key"
              :aria-label="key === '⌫' ? 'Стереть цифру' : `Цифра ${key}`"
              :disabled="vault.busy"
              @click="press(key)"
            >
              {{ key }}
            </button>
          </template>
        </div>

        <p v-if="pinNote" class="unlock__pin-note" role="alert">{{ pinNote }}</p>
        <p v-else-if="vault.error" class="unlock__pin-note" role="alert">{{ vault.error }}</p>

        <button type="button" class="unlock__switch" @click="toMaster">Ввести мастер-пароль</button>
      </div>

      <!-- Мастер-пароль: единственный настоящий ключ. -->
      <template v-else>
        <form class="unlock__form" @submit.prevent="submit">
          <SyInput
            v-model="masterPassword"
            label="Пароль от хранилища"
            type="password"
            autocomplete="current-password"
            autofocus
            :error="vault.error"
            @submit="submit"
          />

          <p v-if="pinNote" class="unlock__pin-note" role="status">{{ pinNote }}</p>

          <p v-if="capsLock" class="unlock__caps">
            <span class="unlock__caps-dot" aria-hidden="true" />
            Включён Caps Lock
          </p>

          <SyButton
            type="submit"
            variant="primary"
            size="lg"
            block
            :disabled="!canSubmit"
            :loading="vault.busy"
          >
            Открыть хранилище
          </SyButton>
        </form>

        <button v-if="vault.pin.enrolled" type="button" class="unlock__switch" @click="toPin">
          Назад к PIN
        </button>

        <div v-else class="unlock__alt">
          <span class="unlock__alt-label">Быстрый вход</span>
          <p class="unlock__alt-text">
            PIN и биометрия — удобство поверх мастер-пароля, а не замена ему. Включаются на этом
            устройстве и с него же не уезжают.
          </p>
        </div>
      </template>

      <p class="unlock__footnote">
        Пароль нигде не хранится и не восстанавливается: он и есть ключ. Восстановления по почте не
        существует — сервера, который мог бы его прислать, нет.
      </p>
    </section>

    <p class="unlock__ground">данные лежат только на ваших устройствах · сервера нет</p>
  </main>
</template>

<style scoped>
/* Экран живёт внутри окна (F13): свою полосу заголовка он больше не рисует. */
.unlock {
  position: relative;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: auto;
  background: var(--sy-bg-0);
}

.unlock__panel {
  flex: 1;
  align-self: center;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-7);
  width: 380px;
  max-width: 100%;
  padding: var(--sy-space-10) var(--sy-space-6);
}

.unlock__mark {
  display: grid;
  place-items: center;
  width: 56px;
  height: 56px;
  border: 1.5px solid var(--sy-accent);
  border-radius: var(--sy-radius-lg);
}

.unlock__mark span {
  width: 8px;
  height: 8px;
  border-radius: 3px;
  background: var(--sy-accent);
}

.unlock__header {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.unlock__title {
  font-size: 18px;
  font-weight: var(--sy-weight-semibold);
}

.unlock__subtitle {
  font-size: 13px;
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

/* PIN */

.unlock__pin {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--sy-space-6);
}

.unlock__cells {
  display: flex;
  gap: var(--sy-space-5);
}

/*
 * Клетка, а не точка: длина PIN известна заранее и одинакова всегда, поэтому
 * показать её — не значит выдать что-то о самом PIN.
 */
.unlock__cell {
  display: grid;
  place-items: center;
  width: 46px;
  height: 56px;
  border: 1px solid var(--sy-border);
  border-radius: 9px;
  background: var(--sy-bg-1);
}

.unlock__cell span {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--sy-border-strong);
}

.unlock__cell--filled {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
}

.unlock__cell--filled span {
  width: 10px;
  height: 10px;
  background: var(--sy-accent);
}

.unlock__keys {
  display: grid;
  grid-template-columns: repeat(3, 72px);
  gap: var(--sy-space-4);
}

.unlock__key-gap {
  display: block;
}

.unlock__key {
  height: 56px;
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-md);
  background: var(--sy-surface);
  color: var(--sy-text);
  font-family: var(--sy-font-mono);
  font-size: 19px;
  cursor: pointer;
  transition:
    background var(--sy-transition),
    border-color var(--sy-transition);
}

.unlock__key:hover:not(:disabled) {
  border-color: var(--sy-border-strong);
  background: var(--sy-surface-2);
}

.unlock__key:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

.unlock__key:disabled {
  cursor: progress;
  opacity: 0.6;
}

.unlock__pin-note {
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--sy-warn);
  text-align: center;
  text-wrap: pretty;
}

.unlock__switch {
  align-self: center;
  height: 32px;
  padding: 0 var(--sy-space-5);
  border: 1px solid transparent;
  border-radius: var(--sy-radius-sm);
  background: transparent;
  color: var(--sy-accent);
  font-family: inherit;
  font-size: 13px;
  cursor: pointer;
}

.unlock__switch:hover {
  background: var(--sy-surface);
}

.unlock__switch:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

/* Мастер-пароль */

.unlock__form {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-6);
}

.unlock__caps {
  display: flex;
  align-items: center;
  gap: var(--sy-space-3);
  font-size: 12px;
  color: var(--sy-warn);
}

.unlock__caps-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--sy-warn);
}

.unlock__alt {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px dashed var(--sy-border-strong);
  border-radius: var(--sy-radius);
}

.unlock__alt-label {
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.unlock__alt-text {
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.unlock__footnote {
  padding-top: var(--sy-space-5);
  border-top: 1px solid var(--sy-border);
  font-size: 12px;
  line-height: 1.55;
  color: var(--sy-text-3);
  text-wrap: pretty;
}

/*
 * Строка у нижнего края окна — обещание продукта, а не подпись формы. Поэтому
 * она вне панели и не двигается вместе с ней.
 */
.unlock__ground {
  position: absolute;
  left: 0;
  right: 0;
  bottom: var(--sy-space-6);
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-3);
  text-align: center;
}
</style>

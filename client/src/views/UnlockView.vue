<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'

import { SyButton, SyInput } from '@/components/ui'
import { useVaultStore } from '@/stores/useVaultStore'

/**
 * Экран разблокировки (F3, §3.8).
 *
 * ЗАКОН №1: введённый мастер-пароль живёт ровно до отправки в ядро — сразу
 * после вызова `unlock` ref очищается, в стор он не попадает вовсе.
 * PIN и биометрия — платформенная часть, glue в MVP 2; здесь только плейсхолдер.
 */

const router = useRouter()
const vault = useVaultStore()

const masterPassword = ref('')
const capsLock = ref(false)

const lockNote = computed(() => {
  if (vault.lockReason === 'timeout') return 'Хранилище закрылось само — не было действий.'
  if (vault.lockReason === 'system') return 'Хранилище закрылось: компьютер уходил в сон.'
  return 'Первый вход после запуска — только паролем.'
})

const canSubmit = computed(() => masterPassword.value.length > 0 && !vault.busy)

onMounted(() => {
  vault.clearError()
})

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
</script>

<template>
  <main class="unlock" @keydown="onKeydown">
    <section class="unlock__panel">
      <div class="unlock__mark" aria-hidden="true"><span /></div>

      <header class="unlock__header">
        <h1 class="unlock__title">Мастер-пароль</h1>
        <p class="unlock__subtitle">{{ lockNote }}</p>
      </header>

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

      <div class="unlock__alt">
        <span class="unlock__alt-label">Быстрый вход</span>
        <p class="unlock__alt-text">
          PIN и биометрия — удобство поверх мастер-пароля, а не замена ему. Появятся в следующей
          версии.
        </p>
      </div>

      <p class="unlock__footnote">
        Пароль нигде не хранится и не восстанавливается: он и есть ключ. Восстановления по почте не
        существует — сервера, который мог бы его прислать, нет.
      </p>
    </section>
  </main>
</template>

<style scoped>
/* Экран живёт внутри окна (F13): свою полосу заголовка он больше не рисует. */
.unlock {
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
</style>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { SyButton, SyInput, SyModal } from '@/components/ui'
import { MASTER_PASSWORD_MIN_LENGTH } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useToastStore } from '@/stores/useToastStore'
import { useVaultStore } from '@/stores/useVaultStore'

/**
 * Смена мастер-пароля (F13, «Настройки → Безопасность»).
 *
 * ЗАКОН №1: оба пароля живут в области видимости этого компонента и уходят
 * аргументом в ядро. Перешифровывает хранилище ядро; здесь нет и не может быть
 * ни одной криптографической операции.
 *
 * Поле подтверждения нового пароля добавлено СВЕРХ прототипа намеренно. Пути
 * восстановления у продукта нет — его же текст об этом и говорит, — поэтому
 * опечатка в новом пароле означала бы безвозвратную потерю всего хранилища.
 * Это единственное место во всём F13, где расхождение с макетом обязательно.
 *
 * Порог длины берётся из контракта (`MASTER_PASSWORD_MIN_LENGTH`), а не из
 * «не короче 12» в макете: политику задаёт ядро, и второе число в UI стало бы
 * ложью при первом же расхождении.
 */

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()

const toast = useToastStore()
const vault = useVaultStore()

const current = ref('')
const next = ref('')
const repeat = ref('')

const busy = ref(false)
const error = ref<string | null>(null)

/** Закрыли диалог — черновики паролей не переживают его. */
watch(
  () => props.open,
  (open) => {
    if (!open) reset()
  },
)

function reset(): void {
  current.value = ''
  next.value = ''
  repeat.value = ''
  error.value = null
  busy.value = false
}

const tooShort = computed(
  () => next.value !== '' && next.value.length < MASTER_PASSWORD_MIN_LENGTH,
)
const mismatch = computed(() => repeat.value !== '' && repeat.value !== next.value)
const same = computed(() => next.value !== '' && next.value === current.value)

const ready = computed(
  () =>
    current.value !== '' &&
    next.value.length >= MASTER_PASSWORD_MIN_LENGTH &&
    repeat.value === next.value &&
    !same.value,
)

async function submit(): Promise<void> {
  if (!ready.value || busy.value) return

  busy.value = true
  error.value = null
  try {
    // Через стор, а не напрямую в ядро: перешифровка сбрасывает быстрый вход, и
    // стор гасит `pin.enrolled` сразу. Иначе экран блокировки после следующего
    // замка показал бы клавиатуру, которая больше ничего не отпирает.
    const result = await vault.changeMasterPassword(current.value, next.value)
    // Хранилище остаётся открытым: человек только что доказал знание старого
    // пароля, и запирать за это — наказывать за успех.
    toast.push(
      result.devices_to_update > 0
        ? `Мастер-пароль изменён · остальные устройства спросят новый при следующей встрече`
        : 'Мастер-пароль изменён',
      'success',
    )
    emit('close')
  } catch (cause) {
    error.value = isCoreError(cause) ? cause.message : 'Не удалось сменить мастер-пароль.'
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <SyModal
    :open="open"
    size="form"
    title="Сменить мастер-пароль"
    tone="warning"
    warning="Восстановления не существует: мастер-пароль нигде не хранится. Забытый новый пароль означает потерю всего хранилища."
    @close="emit('close')"
  >
    <div class="master" data-test="master-password-modal">
      <p class="master__lead">
        Хранилище перешифруется на этом устройстве. Остальные ваши устройства спросят новый пароль
        при следующей встрече — до тех пор они продолжают работать со своей копией.
      </p>

      <SyInput
        v-model="current"
        label="Текущий мастер-пароль"
        type="password"
        autocomplete="current-password"
      />

      <SyInput
        v-model="next"
        label="Новый мастер-пароль"
        type="password"
        autocomplete="new-password"
        :error="
          tooShort
            ? `Не короче ${MASTER_PASSWORD_MIN_LENGTH} символов.`
            : same
              ? 'Это тот же пароль, что и сейчас.'
              : null
        "
        hint="Длинная фраза надёжнее короткого набора символов."
      />

      <!--
        Поля подтверждения в прототипе нет. Оно здесь потому, что опечатку
        поймать больше негде: пароль не хранится, и «забыл» равно «потерял всё».
      -->
      <SyInput
        v-model="repeat"
        label="Новый пароль ещё раз"
        type="password"
        autocomplete="new-password"
        :error="mismatch ? 'Пароли не совпадают.' : null"
      />

      <p v-if="error" class="master__error" role="alert">{{ error }}</p>
    </div>

    <template #note>Хранилище останется открытым — запирать не придётся.</template>

    <template #actions>
      <SyButton size="sm" :disabled="busy" @click="emit('close')">Отмена</SyButton>
      <SyButton
        variant="primary"
        size="sm"
        :disabled="!ready"
        :loading="busy"
        data-test="master-password-submit"
        @click="submit"
      >
        Сменить пароль
      </SyButton>
    </template>
  </SyModal>
</template>

<style scoped>
.master {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
}

.master__lead {
  font-size: var(--sy-text-body);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.master__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}
</style>

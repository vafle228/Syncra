<script setup lang="ts">
import { SyButton, SyModal } from '@/components/ui'
import type { Device } from '@/core/contract'

/**
 * Подтверждение отзыва доступа (F9, §2.3) — модалка поверх окна (F13).
 *
 * Чего этот диалог НЕ должен обещать. Отзыв отрезает устройство от будущих
 * синхронизаций — и только. Полная реплика хранилища, которую украденный
 * ноутбук уже скачал, остаётся у него на диске, и вернуть её нельзя: настоящая
 * защита при краже — шифрование at-rest (§2.3). Поэтому текст говорит это
 * прямо и советует то, что правда помогает, а не изображает кнопку «стереть
 * удалённо».
 *
 * Имя устройства стоит В ЗАГОЛОВКЕ, а не только в списке за диалогом: у
 * подтверждения, вырванного из строки, единственная защита от ошибки строкой —
 * назвать вслух, у кого именно отбирают доступ.
 */

defineProps<{
  /** Устройство, у которого спрашивают. `null` — диалог закрыт. */
  device: Device | null
  busy: boolean
  error: string | null
}>()

const emit = defineEmits<{ close: []; confirm: [] }>()
</script>

<template>
  <SyModal
    :open="device !== null"
    size="confirm"
    tone="danger"
    :title="`Отозвать доступ у «${device?.name ?? ''}»?`"
    @close="emit('close')"
  >
    <div data-test="revoke-modal">
      <p class="revoke__text">
        Устройство перестанет получать обновления и не сможет сопрячься по старому ключу. Записи,
        которые оно уже скачало, остаются в его копии — если ноутбук потерян, смените мастер-пароль
        и пароли важных сервисов.
      </p>
      <p v-if="error" class="revoke__error" role="alert">{{ error }}</p>
    </div>

    <!--
      Сноска называет ту самую кнопку, которой подтверждают (`Прототип:2508`):
      подтверждение — это не «ещё раз кликнуть куда-нибудь», а нажать именно её.
    -->
    <template #note>Подтвердите повторным нажатием «Отозвать»</template>

    <template #actions>
      <SyButton size="sm" :disabled="busy" @click="emit('close')">Отмена</SyButton>
      <SyButton size="sm" variant="danger" :loading="busy" @click="emit('confirm')">
        Отозвать
      </SyButton>
    </template>
  </SyModal>
</template>

<style scoped>
.revoke__text {
  font-size: var(--sy-text-body);
  line-height: 1.6;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.revoke__error {
  margin-top: var(--sy-space-4);
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}
</style>

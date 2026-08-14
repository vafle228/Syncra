<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

import { iconHue, iconInitials, SyButton } from '@/components/ui'
import { pluralize } from '@/composables/plural'
import type { Device } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useDevicesStore } from '@/stores/useDevicesStore'

import {
  DEVICE_FORMS,
  devicePresence,
  deviceSubtitle,
  fingerprintLine,
  isStale,
} from './deviceFormat'
import RevokeDeviceModal from './RevokeDeviceModal.vue'

/**
 * Доверенные устройства и отзыв (F9, §2.3 — подвал экрана §3.6 макета).
 *
 * Чего этот экран НЕ должен обещать. Отзыв отрезает устройство от будущих
 * синхронизаций — и только. Полная реплика хранилища, которую украденный
 * ноутбук уже скачал, остаётся у него на диске, и вернуть её нельзя: настоящая
 * защита при краже — шифрование at-rest (§2.3). Поэтому подтверждение говорит
 * это прямым текстом и советует то, что правда помогает (сменить мастер-пароль
 * и пароли важных сервисов), а не изображает кнопку «стереть удалённо».
 *
 * Подтверждение — модалка (F13, как в прототипе). Риск «ошибиться строкой»
 * снимается тем, что имя устройства стоит в заголовке диалога: вопрос называет
 * вслух, у кого именно отбирают доступ.
 */

const emit = defineEmits<{
  /** Отозванное устройство добавляют заново — это новое сопряжение (§2.2). */
  (event: 'repair', device: Device): void
}>()

const store = useDevicesStore()

onMounted(() => {
  void store.ensure()
})

/** Устройство, у которого сейчас спрашивают подтверждение. Открыто одно. */
const asking = ref<string | null>(null)
const askingDevice = computed(
  () => store.devices.find((device) => device.device_id === asking.value) ?? null,
)
const busyId = ref<string | null>(null)
const actionError = ref<{ deviceId: string; message: string } | null>(null)

function errorFor(deviceId: string): string | null {
  return actionError.value?.deviceId === deviceId ? actionError.value.message : null
}

function ask(device: Device): void {
  actionError.value = null
  asking.value = device.device_id
}

async function revoke(device: Device): Promise<void> {
  busyId.value = device.device_id
  actionError.value = null
  try {
    await store.revoke(device.device_id)
    asking.value = null
  } catch (cause) {
    actionError.value = {
      deviceId: device.device_id,
      message: isCoreError(cause) ? cause.message : 'Не удалось отозвать доступ.',
    }
  } finally {
    busyId.value = null
  }
}

// ---------------------------------------------------------------------------
// Тексты
// ---------------------------------------------------------------------------

/** «2 устройства в доверенных · отозвано за всё время: 1». */
const summary = computed(() => {
  const line = `${pluralize(store.active.length, DEVICE_FORMS)} в доверенных`
  return store.revoked.length === 0
    ? line
    : `${line} · отозвано за всё время: ${store.revoked.length}`
})

/**
 * Подзаголовок списка. Обещание «список полный» держится в обоих случаях, но
 * говорить «хранилище живёт только здесь», когда рядом стоит iPhone, было бы
 * неправдой.
 */
const lead = computed(() =>
  store.isAlone
    ? 'Список полный: хранилище живёт только здесь и больше нигде.'
    : 'Список полный: других копий хранилища, кроме этих устройств, не существует.',
)

function tile(device: Device): { initials: string; hue: number } {
  return { initials: iconInitials(device.name), hue: iconHue(device.name) }
}

/** Обёртка ради шаблона: считать «сейчас» в разметке нельзя. */
function presence(device: Device): { text: string; live: boolean } {
  return devicePresence(device)
}
</script>

<template>
  <section class="trusted">
    <header class="trusted__head">
      <div class="trusted__head-text">
        <h3 class="trusted__title">Доверенные устройства</h3>
        <p class="trusted__lead">{{ lead }}</p>
      </div>
      <span class="trusted__summary">{{ summary }}</span>
    </header>

    <p v-if="store.error" class="trusted__error" role="alert">{{ store.error }}</p>

    <div class="trusted__list">
      <article
        v-for="device in store.devices"
        :key="device.device_id"
        class="trusted__card"
        :class="{ 'trusted__card--revoked': device.revoked_at !== null }"
      >
        <div class="trusted__row">
          <span
            class="trusted__tile"
            :style="{
              color:
                device.revoked_at === null
                  ? `oklch(var(--sy-icon-ink) ${tile(device).hue})`
                  : 'var(--sy-text-3)',
            }"
            aria-hidden="true"
          >
            {{ tile(device).initials }}
          </span>

          <div class="trusted__text">
            <div class="trusted__name-row">
              <span class="trusted__name">{{ device.name }}</span>
              <span v-if="device.is_this_device" class="trusted__tag">это устройство</span>
              <span v-if="device.revoked_at !== null" class="trusted__tag trusted__tag--revoked">
                доступ отозван
              </span>
            </div>
            <p class="trusted__sub">{{ deviceSubtitle(device) }}</p>
            <!--
              Отпечаток стоит под именем, а не в диалоге сопряжения: совет
              «сверьте отпечаток глазами» имеет смысл только там, где его видно.
            -->
            <p class="trusted__fingerprint">{{ fingerprintLine(device) }}</p>
            <p v-if="errorFor(device.device_id)" class="trusted__error" role="alert">
              {{ errorFor(device.device_id) }}
            </p>
          </div>

          <!--
            Присутствие: точка плюс два слова. Дата последнего сеанса тут не
            нужна — нужен ответ «рядом ли оно сейчас».
          -->
          <span
            class="trusted__presence"
            :class="{ 'trusted__presence--live': presence(device).live }"
          >
            <span class="trusted__presence-dot" aria-hidden="true" />
            {{ presence(device).text }}
          </span>

          <span v-if="isStale(device)" class="trusted__stale">
            <span class="trusted__stale-dot" aria-hidden="true" />
            давно не появлялся
          </span>

          <!--
            «Отозвать» в покое НЕЙТРАЛЬНА и краснеет только под курсором
            (`Прототип:2062`): красная кнопка в списке читалась бы как «здесь
            что-то не так с этим устройством», а с ним всё в порядке.
          -->
          <button
            v-if="device.revoked_at === null && !device.is_this_device"
            type="button"
            class="trusted__revoke"
            :disabled="asking === device.device_id"
            @click="ask(device)"
          >
            Отозвать
          </button>
          <SyButton
            v-else-if="device.revoked_at !== null"
            class="trusted__action"
            size="sm"
            variant="ghost"
            @click="emit('repair', device)"
          >
            Сопрячь заново
          </SyButton>
        </div>
      </article>

      <p v-if="store.loaded && store.devices.length === 0" class="trusted__empty">
        Ядро не вернуло ни одного устройства — даже этого. Похоже на сбой хранилища.
      </p>
    </div>

    <RevokeDeviceModal
      :device="askingDevice"
      :busy="busyId !== null"
      :error="asking === null ? null : errorFor(asking)"
      @close="asking = null"
      @confirm="askingDevice && revoke(askingDevice)"
    />
  </section>
</template>

<style scoped>
.trusted {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
}

.trusted__head {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: var(--sy-space-6);
}

.trusted__head-text {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
}

.trusted__title {
  font-size: var(--sy-text-body-strong);
  font-weight: var(--sy-weight-semibold);
}

.trusted__lead {
  font-size: var(--sy-text-small);
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.trusted__summary {
  flex: none;
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-3);
}

.trusted__list {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
}

.trusted__card {
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-surface);
  overflow: hidden;
}

.trusted__row {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
  padding: var(--sy-space-5) var(--sy-space-6);
}

.trusted__tile {
  flex: none;
  display: grid;
  place-items: center;
  width: 38px;
  height: 38px;
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-1);
  font-family: var(--sy-font-mono);
  font-size: 12px;
}

.trusted__text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.trusted__name-row {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
}

.trusted__name {
  font-size: var(--sy-text-item-title);
  font-weight: var(--sy-weight-medium);
}

.trusted__card--revoked .trusted__name,
.trusted__card--revoked .trusted__sub {
  color: var(--sy-text-3);
}

.trusted__tag {
  flex: none;
  padding: 2px 7px;
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius-tag);
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-tag);
  color: var(--sy-accent);
}

.trusted__tag--revoked {
  border-color: var(--sy-danger);
  background: transparent;
  color: var(--sy-danger);
}

.trusted__sub {
  font-size: var(--sy-text-small);
  color: var(--sy-text-2);
}

.trusted__fingerprint {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  color: var(--sy-text-3);
}

.trusted__card--revoked .trusted__fingerprint {
  color: var(--sy-text-3);
}

.trusted__presence {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--sy-space-3);
  font-size: var(--sy-text-small);
  color: var(--sy-text-2);
}

.trusted__presence-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--sy-text-3);
}

.trusted__presence--live .trusted__presence-dot {
  background: var(--sy-accent);
}

/* Нейтральная в покое, красная под курсором — и только так. */
.trusted__revoke {
  flex: none;
  height: 32px;
  padding: 0 var(--sy-space-5);
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius-inner);
  background: var(--sy-bg-1);
  color: var(--sy-text-2);
  font-family: inherit;
  font-size: var(--sy-text-small);
  cursor: pointer;
  transition:
    border-color var(--sy-transition),
    color var(--sy-transition);
}

.trusted__revoke:hover:not(:disabled) {
  border-color: var(--sy-danger);
  color: var(--sy-danger);
}

.trusted__revoke:disabled {
  color: var(--sy-text-3);
  cursor: not-allowed;
}

.trusted__revoke:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

.trusted__stale {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--sy-space-3);
  font-size: 11.5px;
  color: var(--sy-warn);
}

.trusted__stale-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--sy-warn);
}

.trusted__action {
  flex: none;
}

.trusted__empty,
.trusted__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}
</style>

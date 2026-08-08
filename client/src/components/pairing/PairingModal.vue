<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import PairingQr from '@/components/pairing/PairingQr.vue'
import PairingScanBox from '@/components/pairing/PairingScanBox.vue'
import { SyButton, SyModal } from '@/components/ui'
import { pluralize, RECORD_FORMS } from '@/composables/plural'
import { formatManualCode, usePairingOffer, usePairingScan } from '@/composables/usePairing'
import { useDevicesStore } from '@/stores/useDevicesStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { useToastStore } from '@/stores/useToastStore'

/**
 * Знакомство устройств (F8, §2.2) — модалка поверх окна (F13).
 *
 * Экран держит два обещания продукта:
 *  1. Аккаунта нет, входить некуда. Второе устройство добавляется тем, что два
 *     экрана смотрят друг на друга: одно показывает код, другое его читает.
 *  2. Слова-отпечаток сверяет ЧЕЛОВЕК, и до его подтверждения ничего не
 *     сопряжено. Это единственная защита от того, что между устройствами кто-то
 *     встал (§2.2), — поэтому шаг нельзя ни пропустить, ни «подтвердить за
 *     пользователя».
 *
 * ЗАКОН №1: код сопряжения живёт в области видимости этого компонента
 * (composable), а не в Pinia и не в localStorage: тот, кто его прочитал,
 * получает право забрать копию хранилища. Закрытие модалки и блокировка
 * хранилища его стирают — поэтому содержимое монтируется только с `open`.
 */

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()

const sections = useSectionsStore()
const devices = useDevicesStore()
const toast = useToastStore()

const offer = usePairingOffer()
const scan = usePairingScan()

/** Что делает ЭТО устройство: показывает код или читает чужой. */
type Mode = 'show' | 'scan'
const mode = ref<Mode>('show')

/**
 * Код просим при открытии и забываем при закрытии.
 *
 * Именно так, а не «один раз при монтировании»: модалка живёт всё время, пока
 * открыт экран устройств, и оставлять действующий код за закрытым диалогом —
 * значит держать наготове право забрать хранилище.
 */
watch(
  () => props.open,
  (open) => {
    if (open) {
      void sections.ensure()
      void devices.ensure()
      if (offer.offer.value === null && mode.value === 'show') void offer.request()
    } else {
      offer.forget()
      void scan.cancel()
      scan.reset()
      mode.value = 'show'
    }
  },
  { immediate: true },
)

/**
 * Второе устройство прочитало наш код и подтвердило сопряжение (обратная
 * сторона F8, событие `device_paired`).
 *
 * Это ровно тот случай, когда человек смотрит на экран и ничего не нажимает:
 * он держит код, а всё происходит на другом устройстве. Не сказать ему об этом
 * значило бы оставить его гадать, сработало ли.
 */
watch(
  () => devices.justPaired,
  (result) => {
    if (result === null) return
    toast.push(
      `«${result.device.name}» сопряжено · ${pluralize(result.records_transferred, RECORD_FORMS)} уехало`,
      'success',
    )
    // Код своё отработал: показывать его дальше — приглашать третьего.
    offer.forget()
  },
)

function showCode(): void {
  mode.value = 'show'
  void scan.cancel()
  if (offer.offer.value === null) void offer.request()
}

function startScan(): void {
  mode.value = 'scan'
  // Свой код с экрана убираем: он больше не нужен, а лежать ему незачем.
  offer.forget()
}

async function submitScanned(payload: string): Promise<void> {
  await scan.submit(payload)
}

async function confirm(): Promise<void> {
  const peer = scan.handshake.value?.peer_name ?? 'Устройство'
  if (await scan.confirm()) {
    toast.push(`«${peer}» теперь доверенное устройство`, 'success')
    // Список держит ядро, а не экран успеха: перечитываем, чтобы новое
    // устройство появилось в нём сразу.
    void devices.load()
  }
}

function pairAnother(): void {
  scan.reset()
  showCode()
}

/** Показать код заново — используется и снаружи, для «сопрячь заново» (§2.3). */
defineExpose({ pairAnother })

// ---------------------------------------------------------------------------
// Тексты
// ---------------------------------------------------------------------------

const steps = [
  'Откройте Syncra на втором устройстве и перейдите в «Устройства».',
  'Там выберите «Ввожу код» и наведите камеру на этот код или наберите шесть символов.',
  'Сверьте четыре слова на обоих экранах — и подтвердите.',
]

const manualCode = computed(() =>
  offer.offer.value === null ? '' : formatManualCode(offer.offer.value.manual_code),
)

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} мс`
  return `${(ms / 1000).toFixed(1).replace('.', ',')} с`
}
</script>

<template>
  <SyModal
    :open="open"
    size="wide"
    title="Знакомство устройств"
    @close="emit('close')"
  >
    <div class="pairing" data-test="pairing-modal">
      <p class="pairing__lead">
        Аккаунта нет, входить некуда. Второе устройство добавляется тем, что два экрана смотрят
        друг на друга: одно показывает код, другое его читает.
      </p>

      <div v-if="scan.result.value === null" class="pairing__modes">
        <SyButton size="sm" :variant="mode === 'show' ? 'primary' : 'secondary'" @click="showCode">
          Показываю код
        </SyButton>
        <SyButton size="sm" :variant="mode === 'scan' ? 'primary' : 'secondary'" @click="startScan">
          Ввожу код
        </SyButton>
      </div>

      <div class="pairing__panes">
        <!-- Левая панель: сам код, рамка ввода или итог. -->
        <div class="pairing__stage">
          <template v-if="scan.result.value !== null">
            <div class="pairing__pair">
              <span class="pairing__device">Этот компьютер</span>
              <span class="pairing__link" aria-hidden="true" />
              <span class="pairing__device pairing__device--new">
                {{ scan.result.value.device.name }}
              </span>
            </div>
            <p class="pairing__stage-note">
              ключи сверены · {{ formatDuration(scan.result.value.duration_ms) }}
            </p>
          </template>

          <template v-else-if="mode === 'show'">
            <PairingQr
              v-if="offer.offer.value"
              :matrix="offer.offer.value.qr"
              :stale="offer.isExpired.value"
            />
            <div v-else class="pairing__stage-empty">
              {{ offer.busy.value ? 'Ядро собирает код…' : 'Кода нет' }}
            </div>

            <p v-if="offer.offer.value" class="pairing__stage-note">
              <span v-if="offer.isExpired.value">код истёк — нужен новый</span>
              <span v-else>код живёт {{ offer.remainingLabel.value }}</span>
            </p>
          </template>

          <PairingScanBox v-else :busy="scan.busy.value" @submit="submitScanned" />
        </div>

        <!-- Правая панель: что сейчас происходит и что делать. -->
        <div class="pairing__pane">
          <template v-if="scan.result.value !== null">
            <div class="pairing__pane-head">
              <h3 class="pairing__pane-title">
                «{{ scan.result.value.device.name }}» теперь ваш второй ключ
              </h3>
              <p class="pairing__pane-text">
                Копия хранилища перенесена напрямую, минуя интернет. Дальше устройства будут
                догонять друг друга сами, когда окажутся в одной сети.
              </p>
            </div>

            <div class="pairing__stats">
              <div class="pairing__stat">
                <span class="pairing__stat-label">Перенесено</span>
                <span class="pairing__stat-value">
                  {{ pluralize(scan.result.value.records_transferred, RECORD_FORMS) }}
                </span>
                <span class="pairing__stat-note">
                  за {{ formatDuration(scan.result.value.duration_ms) }} по локальной сети
                </span>
              </div>
              <div class="pairing__stat">
                <span class="pairing__stat-label">Ушло в интернет</span>
                <span class="pairing__stat-value">0 байт</span>
                <span class="pairing__stat-note">сервера у Syncra нет</span>
              </div>
            </div>

            <p v-if="sections.hasLocal" class="pairing__pane-note">
              Записи локальных секций не поехали: выключенная синхронизация означает «никуда», в том
              числе на только что сопряжённое устройство. Их единственная копия — здесь.
            </p>

            <div class="pairing__actions">
              <SyButton variant="primary" @click="emit('close')">Готово</SyButton>
              <SyButton @click="pairAnother">Добавить ещё устройство</SyButton>
            </div>
          </template>

          <template v-else-if="mode === 'show'">
            <div class="pairing__pane-head">
              <h3 class="pairing__pane-title">Откройте Syncra на втором устройстве</h3>
              <p class="pairing__pane-text">
                В коде — одноразовый ключ этого сеанса. Он не отправляется в интернет: устройства
                договариваются напрямую, в пределах вашей сети.
              </p>
            </div>

            <ol class="pairing__steps">
              <li v-for="(step, index) in steps" :key="step" class="pairing__step">
                <span class="pairing__step-number">{{ index + 1 }}</span>
                <span>{{ step }}</span>
              </li>
            </ol>

            <p v-if="offer.error.value" class="pairing__error" role="alert">
              {{ offer.error.value }}
            </p>

            <div v-if="offer.offer.value" class="pairing__manual">
              <div class="pairing__manual-text">
                <span class="pairing__manual-title">Камеры нет?</span>
                <span class="pairing__manual-note">
                  Продиктуйте эти шесть символов — их можно набрать на втором устройстве руками.
                </span>
              </div>
              <span
                class="pairing__manual-code"
                :class="{ 'pairing__manual-code--stale': offer.isExpired.value }"
              >
                {{ manualCode }}
              </span>
            </div>

            <div class="pairing__actions">
              <SyButton
                :variant="offer.isExpired.value ? 'primary' : 'secondary'"
                :loading="offer.busy.value"
                @click="offer.request()"
              >
                Обновить код
              </SyButton>
              <SyButton @click="startScan">Прочитать код здесь</SyButton>
            </div>
          </template>

          <template v-else-if="scan.handshake.value !== null">
            <div class="pairing__pane-head">
              <h3 class="pairing__pane-title">Сверьте слова на обоих экранах</h3>
              <p class="pairing__pane-text">
                Одинаковые слова здесь и на «{{ scan.handshake.value.peer_name }}» означают, что
                устройства говорят напрямую. Если списки разные — между ними кто-то встал: отмените
                сопряжение.
              </p>
            </div>

            <div class="pairing__fingerprint">
              <span class="pairing__fingerprint-label">
                Отпечаток сеанса · должен совпасть на обоих экранах
              </span>
              <div class="pairing__words">
                <span
                  v-for="word in scan.handshake.value.fingerprint_words"
                  :key="word"
                  class="pairing__word"
                >
                  {{ word }}
                </span>
              </div>
            </div>

            <p v-if="scan.error.value" class="pairing__error" role="alert">
              {{ scan.error.value }}
            </p>

            <div class="pairing__actions">
              <SyButton variant="primary" :loading="scan.busy.value" @click="confirm">
                Слова совпадают
              </SyButton>
              <SyButton :disabled="scan.busy.value" @click="scan.cancel()">Отмена</SyButton>
            </div>
          </template>

          <template v-else>
            <div class="pairing__pane-head">
              <h3 class="pairing__pane-title">Введите код, который показывает второе устройство</h3>
              <p class="pairing__pane-text">
                Как только код доедет до ядра, оба устройства покажут по четыре слова — сверьте их
                глазами. До этого ничего не сопряжено и ничего не передано.
              </p>
            </div>

            <p v-if="scan.error.value" class="pairing__error" role="alert">
              {{ scan.error.value }}
            </p>
            <p v-if="scan.isExpired.value" class="pairing__pane-note">
              Коды живут несколько минут. Попросите второе устройство показать новый.
            </p>

            <div class="pairing__actions">
              <SyButton @click="showCode">Показать свой код</SyButton>
            </div>
          </template>
        </div>
      </div>
    </div>
  </SyModal>
</template>

<style scoped>
.pairing {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-6);
}

.pairing__lead {
  font-size: var(--sy-text-body);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.pairing__modes {
  display: flex;
  gap: var(--sy-space-3);
}

.pairing__panes {
  display: grid;
  grid-template-columns: 340px minmax(0, 1fr);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-lg);
  background: var(--sy-bg-0);
  overflow: hidden;
}

/* Узкая модалка: код встаёт над объяснением, а не рядом с ним. */
@media (max-width: 900px) {
  .pairing__panes {
    grid-template-columns: minmax(0, 1fr);
  }
}

.pairing__stage {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--sy-space-6);
  padding: var(--sy-space-7) var(--sy-space-6);
  border-right: 1px solid var(--sy-border);
}

.pairing__stage-empty {
  display: grid;
  place-items: center;
  width: 240px;
  height: 240px;
  border: 1px dashed var(--sy-border-strong);
  border-radius: var(--sy-radius-lg);
  font-size: var(--sy-text-small);
  color: var(--sy-text-3);
}

.pairing__stage-note {
  font-family: var(--sy-font-mono);
  font-size: 11.5px;
  color: var(--sy-text-2);
}

.pairing__pair {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
}

.pairing__device {
  display: grid;
  place-items: center;
  width: 96px;
  height: 88px;
  padding: var(--sy-space-4);
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius-lg);
  background: var(--sy-surface);
  font-size: var(--sy-text-small);
  color: var(--sy-text-2);
  text-align: center;
}

.pairing__device--new {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
  color: var(--sy-accent);
}

.pairing__link {
  width: 34px;
  height: 2px;
  background: var(--sy-accent);
}

.pairing__pane {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-6);
  padding: var(--sy-space-7);
}

.pairing__pane-head {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
}

.pairing__pane-title {
  font-size: var(--sy-text-h3);
  line-height: var(--sy-text-h3-lh);
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.01em;
}

.pairing__pane-text {
  font-size: var(--sy-text-body);
  line-height: 1.6;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.pairing__pane-note {
  font-size: var(--sy-text-small);
  line-height: 1.55;
  color: var(--sy-text-3);
  text-wrap: pretty;
}

.pairing__steps {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
  margin: 0;
  padding: 0;
  list-style: none;
}

.pairing__step {
  display: flex;
  align-items: flex-start;
  gap: var(--sy-space-5);
  font-size: var(--sy-text-body);
  line-height: 1.5;
  color: var(--sy-text-2);
}

.pairing__step-number {
  display: grid;
  place-items: center;
  flex: none;
  width: 22px;
  height: 22px;
  border: 1px solid var(--sy-border-strong);
  border-radius: 7px;
  background: var(--sy-surface);
  font-family: var(--sy-font-mono);
  font-size: 11px;
}

.pairing__manual {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-6);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-surface);
}

.pairing__manual-text {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
}

.pairing__manual-title {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-medium);
}

.pairing__manual-note {
  font-size: var(--sy-text-small);
  color: var(--sy-text-2);
}

.pairing__manual-code {
  flex: none;
  font-family: var(--sy-font-mono);
  font-size: 17px;
  letter-spacing: 0.18em;
  color: var(--sy-accent);
}

.pairing__manual-code--stale {
  color: var(--sy-text-3);
  text-decoration: line-through;
}

.pairing__fingerprint {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  padding: var(--sy-space-6);
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius);
  background: var(--sy-accent-quiet);
}

.pairing__fingerprint-label {
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-accent);
}

.pairing__words {
  display: flex;
  gap: var(--sy-space-3);
}

.pairing__word {
  flex: 1;
  display: grid;
  place-items: center;
  height: 40px;
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-bg-0);
  font-family: var(--sy-font-mono);
  font-size: 13.5px;
}

.pairing__stats {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--sy-space-5);
}

.pairing__stat {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-surface);
}

.pairing__stat-label {
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.pairing__stat-value {
  font-size: var(--sy-text-body-strong);
  font-weight: var(--sy-weight-semibold);
}

.pairing__stat-note {
  font-size: var(--sy-text-small);
  color: var(--sy-text-2);
}

.pairing__actions {
  display: flex;
  gap: var(--sy-space-4);
  margin-top: auto;
}

.pairing__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}
</style>

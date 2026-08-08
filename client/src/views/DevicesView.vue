<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'

import TrustedDevices from '@/components/devices/TrustedDevices.vue'
import PairingQr from '@/components/pairing/PairingQr.vue'
import PairingScanBox from '@/components/pairing/PairingScanBox.vue'
import SyncIndicator from '@/components/sync/SyncIndicator.vue'
import SyncPanel from '@/components/sync/SyncPanel.vue'
import { SyButton } from '@/components/ui'
import { pluralize, RECORD_FORMS } from '@/composables/plural'
import { formatManualCode, usePairingOffer, usePairingScan } from '@/composables/usePairing'
import type { Device } from '@/core/contract'
import { useDevicesStore } from '@/stores/useDevicesStore'
import { useSectionsStore } from '@/stores/useSectionsStore'
import { useToastStore } from '@/stores/useToastStore'

/**
 * Устройства: знакомство по QR (F8, §2.2 и §3.6 макета).
 *
 * Экран держит два обещания продукта:
 *  1. Аккаунта нет, входить некуда. Второе устройство добавляется тем, что два
 *     экрана смотрят друг на друга: одно показывает код, другое его читает.
 *  2. Слова-отпечаток сверяет ЧЕЛОВЕК, и до его подтверждения ничего не
 *     сопряжено. Это единственная защита от того, что между устройствами кто-то
 *     встал (§2.2), — поэтому шаг нельзя ни пропустить, ни «подтвердить за
 *     пользователя».
 *
 * В подвале — список доверенных устройств и отзыв (F9, §2.3): он стоит здесь же,
 * потому что «кто имеет доступ» и «дать доступ ещё одному» — один вопрос, и
 * разносить их по разным экранам значило бы прятать половину ответа.
 *
 * ЗАКОН №1: код сопряжения живёт в области видимости этого экрана (composable),
 * а не в Pinia и не в localStorage: тот, кто его прочитал, получает право
 * забрать копию хранилища. Уход с экрана и блокировка хранилища его стирают.
 */

const router = useRouter()
const sections = useSectionsStore()
const devices = useDevicesStore()
const toast = useToastStore()

const offer = usePairingOffer()
const scan = usePairingScan()

/** Что делает ЭТО устройство: показывает код или читает чужой. */
type Mode = 'show' | 'scan'
const mode = ref<Mode>('show')

onMounted(() => {
  void offer.request()
  // Локальные секции никуда не уезжают (§4.2) — про это нужно сказать до того,
  // как человек решит, что на втором устройстве будет всё.
  void sections.ensure()
  // Список нужен сразу: событие о сопряжении придёт в него же (F10).
  void devices.ensure()
})

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
    // Список в подвале держит ядро, а не экран успеха: перечитываем, чтобы
    // новое устройство появилось в нём сразу.
    void devices.load()
  }
}

function pairAnother(): void {
  scan.reset()
  showCode()
}

/**
 * «Сопрячь заново» у отозванного устройства (§2.3).
 *
 * Отменить отзыв нельзя, и команды для этого нет намеренно: устройство,
 * отрезанное по старому ключу, возвращается только через новое знакомство —
 * иначе отзыв не значил бы ничего. Поэтому кнопка ведёт ровно туда же, куда
 * ведёт добавление любого устройства: к показу кода.
 */
function repair(device: Device): void {
  pairAnother()
  toast.push(`Покажите этот код на «${device.name}»: старый ключ больше не действует`, 'neutral')
}

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
  <main class="devices">
    <header class="devices__header">
      <div class="devices__brand">
        <RouterLink class="devices__back" :to="{ name: 'home' }">← К паролям</RouterLink>
        <h1 class="devices__title">Устройства</h1>
      </div>
      <div class="devices__header-actions">
        <SyncIndicator />
      </div>
    </header>

    <div class="devices__body">
      <section class="devices__intro">
        <h2 class="devices__lead-title">Знакомство устройств</h2>
        <p class="devices__lead">
          Аккаунта нет, входить некуда. Второе устройство добавляется тем, что два экрана смотрят
          друг на друга: одно показывает код, другое его читает.
        </p>
      </section>

      <div v-if="scan.result.value === null" class="devices__modes">
        <SyButton size="sm" :variant="mode === 'show' ? 'primary' : 'secondary'" @click="showCode">
          Показываю код
        </SyButton>
        <SyButton size="sm" :variant="mode === 'scan' ? 'primary' : 'secondary'" @click="startScan">
          Ввожу код
        </SyButton>
      </div>

      <div class="devices__panes">
        <!-- Левая панель: сам код, рамка ввода или итог. -->
        <div class="devices__stage">
          <template v-if="scan.result.value !== null">
            <div class="devices__pair">
              <span class="devices__device">Этот компьютер</span>
              <span class="devices__link" aria-hidden="true" />
              <span class="devices__device devices__device--new">
                {{ scan.result.value.device.name }}
              </span>
            </div>
            <p class="devices__stage-note">
              ключи сверены · {{ formatDuration(scan.result.value.duration_ms) }}
            </p>
          </template>

          <template v-else-if="mode === 'show'">
            <PairingQr
              v-if="offer.offer.value"
              :matrix="offer.offer.value.qr"
              :stale="offer.isExpired.value"
            />
            <div v-else class="devices__stage-empty">
              {{ offer.busy.value ? 'Ядро собирает код…' : 'Кода нет' }}
            </div>

            <p v-if="offer.offer.value" class="devices__stage-note">
              <span v-if="offer.isExpired.value">код истёк — нужен новый</span>
              <span v-else>код живёт {{ offer.remainingLabel.value }}</span>
            </p>
          </template>

          <PairingScanBox v-else :busy="scan.busy.value" @submit="submitScanned" />
        </div>

        <!-- Правая панель: что сейчас происходит и что делать. -->
        <div class="devices__pane">
          <template v-if="scan.result.value !== null">
            <div class="devices__pane-head">
              <h3 class="devices__pane-title">
                «{{ scan.result.value.device.name }}» теперь ваш второй ключ
              </h3>
              <p class="devices__pane-text">
                Копия хранилища перенесена напрямую, минуя интернет. Дальше устройства будут
                догонять друг друга сами, когда окажутся в одной сети.
              </p>
            </div>

            <div class="devices__stats">
              <div class="devices__stat">
                <span class="devices__stat-label">Перенесено</span>
                <span class="devices__stat-value">
                  {{ pluralize(scan.result.value.records_transferred, RECORD_FORMS) }}
                </span>
                <span class="devices__stat-note">
                  за {{ formatDuration(scan.result.value.duration_ms) }} по локальной сети
                </span>
              </div>
              <div class="devices__stat">
                <span class="devices__stat-label">Ушло в интернет</span>
                <span class="devices__stat-value">0 байт</span>
                <span class="devices__stat-note">сервера у Syncra нет</span>
              </div>
            </div>

            <p v-if="sections.hasLocal" class="devices__pane-note">
              Записи локальных секций не поехали: выключенная синхронизация означает «никуда», в том
              числе на только что сопряжённое устройство. Их единственная копия — здесь.
            </p>

            <div class="devices__actions">
              <SyButton variant="primary" @click="router.push({ name: 'home' })">Готово</SyButton>
              <SyButton @click="pairAnother">Добавить ещё устройство</SyButton>
            </div>
          </template>

          <template v-else-if="mode === 'show'">
            <div class="devices__pane-head">
              <h3 class="devices__pane-title">Откройте Syncra на втором устройстве</h3>
              <p class="devices__pane-text">
                В коде — одноразовый ключ этого сеанса. Он не отправляется в интернет: устройства
                договариваются напрямую, в пределах вашей сети.
              </p>
            </div>

            <ol class="devices__steps">
              <li v-for="(step, index) in steps" :key="step" class="devices__step">
                <span class="devices__step-number">{{ index + 1 }}</span>
                <span>{{ step }}</span>
              </li>
            </ol>

            <p v-if="offer.error.value" class="devices__error" role="alert">
              {{ offer.error.value }}
            </p>

            <div v-if="offer.offer.value" class="devices__manual">
              <div class="devices__manual-text">
                <span class="devices__manual-title">Камеры нет?</span>
                <span class="devices__manual-note">
                  Продиктуйте эти шесть символов — их можно набрать на втором устройстве руками.
                </span>
              </div>
              <span
                class="devices__manual-code"
                :class="{ 'devices__manual-code--stale': offer.isExpired.value }"
              >
                {{ manualCode }}
              </span>
            </div>

            <div class="devices__actions">
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
            <div class="devices__pane-head">
              <h3 class="devices__pane-title">Сверьте слова на обоих экранах</h3>
              <p class="devices__pane-text">
                Одинаковые слова здесь и на «{{ scan.handshake.value.peer_name }}» означают, что
                устройства говорят напрямую. Если списки разные — между ними кто-то встал: отмените
                сопряжение.
              </p>
            </div>

            <div class="devices__fingerprint">
              <span class="devices__fingerprint-label">
                Отпечаток сеанса · должен совпасть на обоих экранах
              </span>
              <div class="devices__words">
                <span
                  v-for="word in scan.handshake.value.fingerprint_words"
                  :key="word"
                  class="devices__word"
                >
                  {{ word }}
                </span>
              </div>
            </div>

            <p v-if="scan.error.value" class="devices__error" role="alert">
              {{ scan.error.value }}
            </p>

            <div class="devices__actions">
              <SyButton variant="primary" :loading="scan.busy.value" @click="confirm">
                Слова совпадают
              </SyButton>
              <SyButton :disabled="scan.busy.value" @click="scan.cancel()">Отмена</SyButton>
            </div>
          </template>

          <template v-else>
            <div class="devices__pane-head">
              <h3 class="devices__pane-title">Введите код, который показывает второе устройство</h3>
              <p class="devices__pane-text">
                Как только код доедет до ядра, оба устройства покажут по четыре слова — сверьте их
                глазами. До этого ничего не сопряжено и ничего не передано.
              </p>
            </div>

            <p v-if="scan.error.value" class="devices__error" role="alert">
              {{ scan.error.value }}
            </p>
            <p v-if="scan.isExpired.value" class="devices__pane-note">
              Коды живут несколько минут. Попросите второе устройство показать новый.
            </p>

            <div class="devices__actions">
              <SyButton @click="showCode">Показать свой код</SyButton>
            </div>
          </template>
        </div>
      </div>

      <SyncPanel />

      <TrustedDevices @repair="repair" />
    </div>
  </main>
</template>

<style scoped>
.devices {
  display: flex;
  flex-direction: column;
  min-height: 100%;
  background: var(--sy-bg-1);
}

.devices__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  padding: var(--sy-space-5) var(--sy-space-6);
  border-bottom: 1px solid var(--sy-border);
  background: var(--sy-bg-0);
}

.devices__brand {
  display: flex;
  align-items: baseline;
  gap: var(--sy-space-6);
}

.devices__header-actions {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
}

.devices__back {
  font-size: var(--sy-text-body);
  color: var(--sy-accent);
  text-decoration: none;
}

.devices__back:hover {
  text-decoration: underline;
}

.devices__title {
  font-size: var(--sy-text-body-strong);
  font-weight: var(--sy-weight-semibold);
}

.devices__body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-7);
  max-width: 1040px;
  width: 100%;
  margin: 0 auto;
  padding: var(--sy-space-9) var(--sy-space-8);
}

.devices__intro {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
  max-width: 620px;
}

.devices__lead-title {
  font-size: var(--sy-text-h2);
  line-height: var(--sy-text-h2-lh);
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.01em;
}

.devices__lead {
  font-size: var(--sy-text-body);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.devices__modes {
  display: flex;
  gap: var(--sy-space-3);
}

.devices__panes {
  display: grid;
  grid-template-columns: 380px minmax(0, 1fr);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-lg);
  background: var(--sy-bg-0);
  overflow: hidden;
}

.devices__stage {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--sy-space-6);
  padding: var(--sy-space-9) var(--sy-space-7);
  border-right: 1px solid var(--sy-border);
  min-height: 420px;
}

.devices__stage-empty {
  display: grid;
  place-items: center;
  width: 240px;
  height: 240px;
  border: 1px dashed var(--sy-border-strong);
  border-radius: var(--sy-radius-lg);
  font-size: var(--sy-text-small);
  color: var(--sy-text-3);
}

.devices__stage-note {
  font-family: var(--sy-font-mono);
  font-size: 11.5px;
  color: var(--sy-text-2);
}

.devices__pair {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
}

.devices__device {
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

.devices__device--new {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
  color: var(--sy-accent);
}

.devices__link {
  width: 34px;
  height: 2px;
  background: var(--sy-accent);
}

.devices__pane {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-7);
  padding: var(--sy-space-9) var(--sy-space-8);
}

.devices__pane-head {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
}

.devices__pane-title {
  font-size: var(--sy-text-h3);
  line-height: var(--sy-text-h3-lh);
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.01em;
}

.devices__pane-text {
  font-size: var(--sy-text-body);
  line-height: 1.6;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.devices__pane-note {
  font-size: var(--sy-text-small);
  line-height: 1.55;
  color: var(--sy-text-3);
  text-wrap: pretty;
}

.devices__steps {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-4);
  margin: 0;
  padding: 0;
  list-style: none;
}

.devices__step {
  display: flex;
  align-items: flex-start;
  gap: var(--sy-space-5);
  font-size: var(--sy-text-body);
  line-height: 1.5;
  color: var(--sy-text-2);
}

.devices__step-number {
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

.devices__manual {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-6);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-surface);
}

.devices__manual-text {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
}

.devices__manual-title {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-medium);
}

.devices__manual-note {
  font-size: var(--sy-text-small);
  color: var(--sy-text-2);
}

.devices__manual-code {
  flex: none;
  font-family: var(--sy-font-mono);
  font-size: 17px;
  letter-spacing: 0.18em;
  color: var(--sy-accent);
}

.devices__manual-code--stale {
  color: var(--sy-text-3);
  text-decoration: line-through;
}

.devices__fingerprint {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  padding: var(--sy-space-6);
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius);
  background: var(--sy-accent-quiet);
}

.devices__fingerprint-label {
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-accent);
}

.devices__words {
  display: flex;
  gap: var(--sy-space-3);
}

.devices__word {
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

.devices__stats {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--sy-space-5);
}

.devices__stat {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-surface);
}

.devices__stat-label {
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  letter-spacing: var(--sy-tracking-label);
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.devices__stat-value {
  font-size: var(--sy-text-body-strong);
  font-weight: var(--sy-weight-semibold);
}

.devices__stat-note {
  font-size: var(--sy-text-small);
  color: var(--sy-text-2);
}

.devices__actions {
  display: flex;
  gap: var(--sy-space-4);
  margin-top: auto;
}

.devices__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}

@media (max-width: 900px) {
  .devices__panes {
    grid-template-columns: minmax(0, 1fr);
  }

  .devices__stage {
    border-right: none;
    border-bottom: 1px solid var(--sy-border);
  }
}
</style>

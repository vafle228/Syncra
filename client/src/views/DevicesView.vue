<script setup lang="ts">
import { onMounted, ref } from 'vue'

import TrustedDevices from '@/components/devices/TrustedDevices.vue'
import PairingModal from '@/components/pairing/PairingModal.vue'
import SyncPanel from '@/components/sync/SyncPanel.vue'
import { SyButton } from '@/components/ui'
import type { Device } from '@/core/contract'
import { useDevicesStore } from '@/stores/useDevicesStore'
import { useToastStore } from '@/stores/useToastStore'

/**
 * Устройства (F9, §2.3 и §3.6 макета): кто имеет доступ к хранилищу.
 *
 * С F13 экран отвечает ровно на этот вопрос и ни на какой другой. Знакомство
 * нового устройства — редкое действие, и оно уехало в модалку: держать полотно
 * с QR-кодом развёрнутым в тот момент, когда человек зашёл посмотреть список,
 * значило бы держать наготове право забрать копию хранилища.
 *
 * Панель синхронизации остаётся здесь: это единственная точка ручного повтора
 * обмена, и макет «Состояния» требует её для состояния сбоя.
 *
 * ЗАКОН №1: экран не касается ни секретов, ни кода сопряжения — код живёт
 * внутри модалки и умирает вместе с ней.
 */

const devices = useDevicesStore()
const toast = useToastStore()

const pairing = ref(false)

onMounted(() => {
  // Список нужен сразу: событие о сопряжении придёт в него же (F10).
  void devices.ensure()
})

/**
 * «Сопрячь заново» у отозванного устройства (§2.3).
 *
 * Отменить отзыв нельзя, и команды для этого нет намеренно: устройство,
 * отрезанное по старому ключу, возвращается только через новое знакомство —
 * иначе отзыв не значил бы ничего. Поэтому кнопка ведёт ровно туда же, куда
 * ведёт добавление любого устройства.
 */
function repair(device: Device): void {
  pairing.value = true
  toast.push(`Покажите этот код на «${device.name}»: старый ключ больше не действует`, 'neutral')
}
</script>

<template>
  <main class="devices">
    <!--
      Ни «назад», ни индикатора обмена: возвращаться некуда — сайдбар никуда не
      уходил (F13), а состояние обмена стоит в его подвале постоянно.
    -->
    <header class="devices__header">
      <div class="devices__brand">
        <h1 class="devices__title">Доверенные устройства</h1>
        <p class="devices__lead">
          Аккаунта нет: хранилище знают только те устройства, которые вы познакомили лично.
        </p>
      </div>
      <SyButton variant="primary" size="sm" data-test="pair-open" @click="pairing = true">
        Добавить устройство
      </SyButton>
    </header>

    <div class="devices__body">
      <SyncPanel />

      <TrustedDevices @repair="repair" />

      <div class="devices__notes">
        <section class="devices__note">
          <h2 class="devices__note-title">Отпечаток сверяется глазами</h2>
          <p class="devices__note-text">
            Символы под именем устройства — те же, что видны на его экране. Совпали — устройство то
            самое, и никакой сервер этого не подтверждал.
          </p>
        </section>
        <section class="devices__note">
          <h2 class="devices__note-title">Что делает отзыв</h2>
          <p class="devices__note-text">
            Устройство перестаёт получать обновления и не расшифрует новые записи. Копия, которая
            уже лежит на нём, останется — отсюда её не стереть.
          </p>
        </section>
      </div>
    </div>

    <PairingModal :open="pairing" @close="pairing = false" />
  </main>
</template>

<style scoped>
/*
 * Экран — правая панель окна (F13), а не отдельная страница: он занимает
 * остаток ширины и высоту окна, а прокручивается его тело, оставляя сайдбар и
 * полосу заголовка на месте.
 */
.devices {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  background: var(--sy-bg-1);
}

.devices__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-6);
  padding: var(--sy-space-6) var(--sy-space-8);
  border-bottom: 1px solid var(--sy-border);
  background: var(--sy-bg-0);
}

.devices__brand {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
  min-width: 0;
}

.devices__title {
  font-size: 22px;
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.01em;
}

.devices__lead {
  font-size: var(--sy-text-body);
  color: var(--sy-text-2);
  text-wrap: pretty;
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
  padding: var(--sy-space-8);
}

/*
 * Секции тела не сжимаются. Пока экран был страницей во всю высоту документа,
 * это ничего не значило; в панели окна высота стала определённой, и без запрета
 * flex сплющил бы список вместо того, чтобы дать телу прокрутиться.
 */
.devices__body > * {
  flex: none;
}

.devices__notes {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: var(--sy-space-5);
}

.devices__note {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
  padding: var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius);
  background: var(--sy-bg-0);
}

.devices__note-title {
  font-size: 13.5px;
  font-weight: var(--sy-weight-semibold);
}

.devices__note-text {
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}
</style>

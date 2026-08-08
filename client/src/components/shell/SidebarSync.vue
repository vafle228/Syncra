<script setup lang="ts">
import { computed } from 'vue'

import { describeSync, lastSyncLine } from '@/components/sync/syncFormat'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useSyncStore } from '@/stores/useSyncStore'

/**
 * Состояние синхронизации в подвале сайдбара (F10, F13).
 *
 * Прототип рисует здесь три вида точки — пульсирующую, сплошную и полую. Это
 * НЕ три состояния: `describeSync()` различает семь и раскрашивает их четырьмя
 * тонами, и схлопывать «конфликт» с «рядом никого» в одну серую точку значило бы
 * потерять единственное состояние, которое ждёт человека. Поэтому форма точки
 * берётся из прототипа, а цвет — из словаря состояний.
 *
 * Заголовок — короткая подпись (`chip`), а не разъяснение: в 210 пикселях
 * ширины предложению не поместиться. Разъяснение живёт в `SyncPanel` на экране
 * устройств, куда ведёт соседняя строка сайдбара.
 *
 * Данные поднимает `VaultShell` — здесь только чтение: подвал сайдбара не должен
 * слать команды в ядро при каждой перерисовке.
 */

const sync = useSyncStore()
const conflicts = useConflictsStore()

const view = computed(() =>
  sync.status === null ? null : describeSync(sync.status, conflicts.count),
)

/** Вторая строка: когда в последний раз что-то доехало. */
const line = computed(() => (sync.status === null ? '' : lastSyncLine(sync.status)))

/** Подробности — в подсказку: место есть только под две короткие строки. */
const details = computed(() => view.value?.title ?? '')
</script>

<template>
  <div v-if="view" class="sidebar-sync" :class="`sidebar-sync--${view.tone}`" role="status">
    <span
      class="sidebar-sync__dot"
      :class="{
        'sidebar-sync__dot--pulse': view.pulse,
        'sidebar-sync__dot--hollow': view.tone === 'calm',
      }"
      aria-hidden="true"
    />
    <span class="sidebar-sync__text">
      <span class="sidebar-sync__title" :title="details">{{ view.chip }}</span>
      <span class="sidebar-sync__line">{{ line }}</span>
    </span>
  </div>
</template>

<style scoped>
.sidebar-sync {
  display: flex;
  align-items: flex-start;
  gap: var(--sy-space-4);
  padding-top: var(--sy-space-5);
  border-top: 1px solid var(--sy-border);
}

.sidebar-sync__dot {
  flex: none;
  width: 8px;
  height: 8px;
  margin-top: 5px;
  border-radius: 50%;
  background: var(--sy-accent);
}

.sidebar-sync--warn .sidebar-sync__dot {
  background: var(--sy-warn);
}

.sidebar-sync--danger .sidebar-sync__dot {
  background: var(--sy-danger);
}

/* Покой рисуется пустой окружностью: нечему гореть. */
.sidebar-sync__dot--hollow {
  border: 1.5px solid var(--sy-text-3);
  background: transparent;
}

/* Пульс — только там, где прямо сейчас что-то происходит. */
.sidebar-sync__dot--pulse {
  animation: sy-pulse 1.4s ease-in-out infinite;
}

.sidebar-sync__text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.sidebar-sync__title {
  font-size: 12.5px;
  font-weight: var(--sy-weight-medium);
  color: var(--sy-text);
}

.sidebar-sync--danger .sidebar-sync__title {
  color: var(--sy-danger);
}

.sidebar-sync__line {
  font-size: 11px;
  line-height: 1.4;
  color: var(--sy-text-3);
}
</style>

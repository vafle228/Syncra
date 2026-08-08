<script setup lang="ts">
import { computed, onMounted } from 'vue'

import { useConflictsStore } from '@/stores/useConflictsStore'
import { useSyncStore } from '@/stores/useSyncStore'

import { describeSync, lastSyncLine } from './syncFormat'

/**
 * Индикатор синхронизации в шапке окна (F10, § «Состояния» макета).
 *
 * Правило макета, которое здесь и держится: индикатор НИЧЕГО не блокирует и
 * ничего не открывает. Он только отвечает на вопрос «что сейчас происходит» —
 * и молчит, пока ядро не ответило, вместо того чтобы показывать бодрое
 * «синхронизировано» до первого ответа.
 *
 * Состояние берётся из двух сторов: что делает синхронизация — из `sync`,
 * сколько записей ждёт выбора версии — из `conflicts`. Конфликты не
 * продублированы в статусе намеренно (см. контракт).
 */

const sync = useSyncStore()
const conflicts = useConflictsStore()

onMounted(() => {
  void sync.ensure()
  void conflicts.ensure()
})

const view = computed(() =>
  sync.status === null ? null : describeSync(sync.status, conflicts.count),
)

/** Подробности — в подсказку: в шапке для них нет места, а знать их полезно. */
const details = computed(() => {
  if (sync.status === null || view.value === null) return ''
  return `${view.value.title} · ${lastSyncLine(sync.status)}`
})
</script>

<template>
  <span
    v-if="view"
    class="sync-chip"
    :class="`sync-chip--${view.tone}`"
    role="status"
    :title="details"
  >
    <span
      class="sync-chip__dot"
      :class="{ 'sync-chip__dot--pulse': view.pulse }"
      aria-hidden="true"
    />
    <span class="sync-chip__text">{{ view.chip }}</span>
  </span>
</template>

<style scoped>
.sync-chip {
  display: inline-flex;
  align-items: center;
  gap: var(--sy-space-3);
  height: 26px;
  padding: 0 var(--sy-space-4);
  border: 1px solid var(--sy-border);
  border-radius: 7px;
  font-size: 11.5px;
  font-weight: var(--sy-weight-medium);
  color: var(--sy-text-2);
  white-space: nowrap;
}

.sync-chip__dot {
  flex: none;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: currentColor;
}

/* Пульс — только там, где прямо сейчас что-то происходит. */
.sync-chip__dot--pulse {
  animation: sync-pulse 1.4s ease-in-out infinite;
}

.sync-chip--accent {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
  color: var(--sy-accent);
}

.sync-chip--warn {
  border-color: var(--sy-warn);
  background: var(--sy-warn-quiet);
  color: var(--sy-warn);
}

.sync-chip--danger {
  border-color: var(--sy-danger);
  background: var(--sy-danger-quiet);
  color: var(--sy-danger);
}

/* Покой: рамка есть, заливки нет — состояние не просит внимания. */
.sync-chip--calm {
  border-color: var(--sy-border);
  background: transparent;
  color: var(--sy-text-2);
}

.sync-chip--calm .sync-chip__dot {
  background: var(--sy-text-3);
}

@keyframes sync-pulse {
  0%,
  100% {
    opacity: 1;
    transform: scale(1);
  }
  50% {
    opacity: 0.3;
    transform: scale(0.8);
  }
}

@media (prefers-reduced-motion: reduce) {
  .sync-chip__dot--pulse {
    animation: none;
  }
}
</style>

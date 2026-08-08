<script setup lang="ts">
import { computed, onMounted } from 'vue'

import { SyButton } from '@/components/ui'
import { pluralize, RECORD_FORMS } from '@/composables/plural'
import { useConflictsStore } from '@/stores/useConflictsStore'
import { useSyncStore } from '@/stores/useSyncStore'

import { describeSync, lastSyncLine } from './syncFormat'

/**
 * Развёрнутое состояние синхронизации — на экране устройств (F10).
 *
 * Индикатор в шапке отвечает «что сейчас», а здесь есть место объяснить, что
 * это значит и надо ли что-то делать. Кнопка «Попробовать сейчас» показывается
 * ТОЛЬКО после обрыва: у состояния «рядом никого» повторять нечего — там не
 * ошибка, а отсутствие второго устройства, и макет кнопку там запрещает прямо.
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

const lastSync = computed(() => (sync.status === null ? '' : lastSyncLine(sync.status)))
</script>

<template>
  <section class="sync-panel">
    <header class="sync-panel__head">
      <h2 class="sync-panel__caption">Синхронизация</h2>
      <span class="sync-panel__rule" aria-hidden="true" />
    </header>

    <p v-if="sync.error" class="sync-panel__error" role="alert">{{ sync.error }}</p>

    <p v-else-if="view === null" class="sync-panel__loading">Спрашиваю ядро…</p>

    <template v-else>
      <div class="sync-panel__row">
        <span
          class="sync-panel__dot"
          :class="[`sync-panel__dot--${view.tone}`, { 'sync-panel__dot--pulse': view.pulse }]"
          aria-hidden="true"
        />
        <div class="sync-panel__text">
          <h3 class="sync-panel__title">{{ view.title }}</h3>
          <p class="sync-panel__body">{{ view.body }}</p>
        </div>

        <SyButton
          v-if="view.look === 'error'"
          size="sm"
          :loading="sync.loading"
          @click="sync.retry()"
        >
          Попробовать сейчас
        </SyButton>
      </div>

      <div class="sync-panel__facts">
        <span>{{ lastSync }}</span>
        <span class="sync-panel__sep" aria-hidden="true" />
        <span>рядом устройств: {{ sync.peersOnline }}</span>
        <span class="sync-panel__sep" aria-hidden="true" />
        <span>
          {{
            sync.pendingCount === 0
              ? 'всё отправлено'
              : `${pluralize(sync.pendingCount, RECORD_FORMS)} ждёт отправки`
          }}
        </span>
      </div>

      <!--
        Про конфликт говорим и здесь: человек, зашедший посмотреть на
        устройства, должен узнать, что одна запись ждёт его решения, — но
        решается это в списке паролей, поэтому отсюда только ссылка.
      -->
      <p v-if="conflicts.hasConflicts" class="sync-panel__conflicts">
        <span>
          {{ pluralize(conflicts.count, RECORD_FORMS) }} правили на двух устройствах — нужно выбрать
          версию.
        </span>
        <RouterLink class="sync-panel__link" :to="{ name: 'home' }">К паролям</RouterLink>
      </p>
    </template>
  </section>
</template>

<style scoped>
.sync-panel {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-5);
  padding: var(--sy-space-7) var(--sy-space-8);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-lg);
  background: var(--sy-bg-0);
}

.sync-panel__head {
  display: flex;
  align-items: center;
  gap: var(--sy-space-5);
}

.sync-panel__caption {
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.sync-panel__rule {
  flex: 1;
  height: 1px;
  background: var(--sy-border);
}

.sync-panel__row {
  display: flex;
  align-items: flex-start;
  gap: var(--sy-space-5);
}

.sync-panel__dot {
  flex: none;
  width: 9px;
  height: 9px;
  margin-top: 6px;
  border-radius: 50%;
  background: var(--sy-text-3);
}

.sync-panel__dot--accent {
  background: var(--sy-accent);
}

.sync-panel__dot--warn {
  background: var(--sy-warn);
}

.sync-panel__dot--danger {
  background: var(--sy-danger);
}

.sync-panel__dot--pulse {
  animation: sync-panel-pulse 1.4s ease-in-out infinite;
}

.sync-panel__text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.sync-panel__title {
  font-size: var(--sy-text-body-strong);
  font-weight: var(--sy-weight-semibold);
}

.sync-panel__body {
  font-size: var(--sy-text-body);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.sync-panel__facts {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--sy-space-4);
  font-family: var(--sy-font-mono);
  font-size: 10.5px;
  color: var(--sy-text-3);
}

.sync-panel__sep {
  width: 1px;
  height: 11px;
  background: var(--sy-border);
}

.sync-panel__conflicts {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  padding: var(--sy-space-4) var(--sy-space-5);
  border: 1px solid var(--sy-danger);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-danger-quiet);
  font-size: var(--sy-text-small);
  line-height: 1.5;
  color: var(--sy-text);
}

.sync-panel__link {
  flex: none;
  color: var(--sy-danger);
  font-weight: var(--sy-weight-medium);
  text-decoration: none;
}

.sync-panel__link:hover {
  text-decoration: underline;
}

.sync-panel__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}

.sync-panel__loading {
  font-size: var(--sy-text-small);
  color: var(--sy-text-3);
}

@keyframes sync-panel-pulse {
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
  .sync-panel__dot--pulse {
    animation: none;
  }
}
</style>

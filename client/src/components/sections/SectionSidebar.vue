<script setup lang="ts">
import { vaultColorVar } from '@/components/ui'
import type { VaultId } from '@/core/contract'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'

/**
 * Сайдбар секций (F7, §3.1 макета).
 *
 * Секция — это ФИЛЬТР, а не отдельное хранилище: «Все записи» стоит первым и
 * выбрано по умолчанию, потому что по умолчанию всё живёт в одном пространстве
 * (§4.2). Пометка «локально» рядом с несинхронизируемой секцией — не
 * украшение: то, что записи этой секции не уедут на другие устройства, должно
 * быть видно там же, где их открывают, а не только в настройках.
 */

const list = useRecordsStore()
const sections = useSectionsStore()

function count(vaultId: VaultId): number {
  return list.countByVault.get(vaultId) ?? 0
}
</script>

<template>
  <nav class="sections" aria-label="Секции">
    <p class="sections__caption">Секции</p>

    <!--
      Скроллер: секций может быть много, а подвал сайдбара с состоянием обмена и
      замком должен оставаться на виду. «Управление секциями» стоит НИЖЕ него —
      выход из длинного списка не должен уезжать вместе со списком.
    -->
    <div class="sections__scroll">
      <button
        type="button"
        class="sections__item"
        :class="{ 'sections__item--active': list.vaultFilter === null }"
        :aria-current="list.vaultFilter === null ? 'true' : undefined"
        @click="list.setVaultFilter(null)"
      >
        <span class="sections__dot sections__dot--all" aria-hidden="true" />
        <span class="sections__name">Все записи</span>
        <span class="sections__count">{{ list.totalAll }}</span>
      </button>

      <button
        v-for="vault in sections.vaults"
        :key="vault.vault_id"
        type="button"
        class="sections__item"
        :class="{ 'sections__item--active': list.vaultFilter === vault.vault_id }"
        :aria-current="list.vaultFilter === vault.vault_id ? 'true' : undefined"
        @click="list.setVaultFilter(vault.vault_id)"
      >
        <span
          class="sections__dot"
          :style="{ background: vaultColorVar(vault.color) }"
          aria-hidden="true"
        />
        <span class="sections__name">{{ vault.name }}</span>
        <span v-if="!vault.sync" class="sections__local" title="Не уезжает с этого устройства">
          локально
        </span>
        <span class="sections__count">{{ count(vault.vault_id) }}</span>
      </button>
    </div>

    <p v-if="sections.error" class="sections__error" role="alert">{{ sections.error }}</p>

    <RouterLink class="sections__manage" :to="{ name: 'sections' }">
      Управление секциями
    </RouterLink>
  </nav>
</template>

<style scoped>
.sections {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: var(--sy-space-7) var(--sy-space-5) var(--sy-space-6);
}

.sections__caption {
  padding: 0 var(--sy-space-3) var(--sy-space-3);
  font-family: var(--sy-font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--sy-text-3);
}

.sections__scroll {
  display: flex;
  flex-direction: column;
  gap: 3px;
  max-height: 206px;
  overflow: auto;
}

.sections__item {
  display: flex;
  align-items: center;
  gap: var(--sy-space-4);
  width: 100%;
  height: 34px;
  padding: 0 var(--sy-space-4);
  border: 1px solid transparent;
  border-radius: var(--sy-radius-sm);
  background: transparent;
  font-family: inherit;
  font-size: var(--sy-text-body);
  color: var(--sy-text-2);
  text-align: left;
  cursor: pointer;
  transition:
    background var(--sy-transition),
    color var(--sy-transition);
}

.sections__item:hover {
  background: var(--sy-surface);
  color: var(--sy-text);
}

.sections__item:focus-visible {
  outline: none;
  box-shadow: var(--sy-focus-ring);
}

.sections__item--active {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
  color: var(--sy-text);
  font-weight: var(--sy-weight-medium);
}

.sections__dot {
  flex: none;
  width: 7px;
  height: 7px;
  border-radius: 2px;
}

.sections__dot--all {
  background: var(--sy-text-3);
}

.sections__name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sections__local {
  flex: none;
  padding: 1px var(--sy-space-2);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-xs);
  font-family: var(--sy-font-mono);
  font-size: 9.5px;
  letter-spacing: 0.06em;
  color: var(--sy-text-3);
}

.sections__count {
  flex: none;
  font-family: var(--sy-font-mono);
  font-size: 11px;
  color: var(--sy-text-3);
}

.sections__item--active .sections__count {
  color: var(--sy-accent);
}

.sections__error {
  padding: var(--sy-space-3) var(--sy-space-4);
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}

.sections__manage {
  margin-top: var(--sy-space-2);
  display: grid;
  place-items: center;
  height: 30px;
  border: 1px dashed var(--sy-border-strong);
  border-radius: var(--sy-radius-sm);
  font-size: var(--sy-text-small);
  color: var(--sy-text-3);
  text-decoration: none;
}

.sections__manage:hover {
  border-color: var(--sy-text-3);
  color: var(--sy-text-2);
}
</style>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import BackupCard from '@/components/data/BackupCard.vue'
import CsvExportCard from '@/components/data/CsvExportCard.vue'
import ImportWizard from '@/components/data/ImportWizard.vue'
import { SyThemeToggle } from '@/components/ui'
import type { VaultId } from '@/core/contract'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'

/**
 * Данные на входе и на выходе (F12, §3.10 макета, §6.2 спека).
 *
 * Экран собран вокруг одной мысли: бэкап и CSV делают почти одно и то же, но
 * CSV — это пароли открытым текстом в папке «Загрузки». Разница должна
 * читаться до клика, а не после, поэтому опасное не спрятано, но и не
 * соседствует с безобидным: у него своя рамка, своя фактура и свои
 * подтверждения.
 *
 * ЗАКОН №1: содержимого файлов на этом экране нет. Ни экспорт, ни импорт не
 * проносят через IPC ни одного пароля — файлы собирает и разбирает ядро
 * (см. `ExportFile` и `ImportPreview` в контракте).
 */

const route = useRoute()
const router = useRouter()
const records = useRecordsStore()
const sections = useSectionsStore()

type Tab = 'import' | 'export'

const tab = ref<Tab>(route.query.tab === 'export' ? 'export' : 'import')

/** Сколько уедет в файл: живые записи всех секций, включая локальные. */
const recordCount = computed(() => records.totalAll)
const vaultCount = computed(() => sections.vaults.length)

onMounted(() => {
  if (!records.loaded) void records.load()
  void sections.ensure()
})

/**
 * Импорт закончился — показываем, что приехало. Фильтр списка ставим на
 * свежую секцию: 300 чужих записей вперемешку со своими нельзя ни проверить,
 * ни разобрать.
 */
function showImported(vaultId: VaultId): void {
  records.setVaultFilter(vaultId)
  void records.load()
  void router.push({ name: 'home' })
}
</script>

<template>
  <main class="data">
    <header class="data__header">
      <div class="data__brand">
        <RouterLink class="data__back" :to="{ name: 'home' }">← К паролям</RouterLink>
        <h1 class="data__title">Данные</h1>
      </div>
      <SyThemeToggle />
    </header>

    <div class="data__body">
      <section class="data__pane">
        <div class="data__intro">
          <h2 class="data__pane-title">Занести пароли внутрь и вынести наружу</h2>
          <p class="data__pane-text">
            Импорт — первый шаг к тому, чтобы хранилищем начали пользоваться. Экспорт — два разных
            поступка с разной ценой ошибки, и выглядеть они должны по-разному.
          </p>
        </div>

        <div class="data__tabs" role="tablist" aria-label="Данные">
          <button
            v-for="item in [
              { key: 'import' as const, label: 'Импорт' },
              { key: 'export' as const, label: 'Экспорт' },
            ]"
            :key="item.key"
            type="button"
            role="tab"
            class="data__tab"
            :class="{ 'data__tab--on': tab === item.key }"
            :aria-selected="tab === item.key"
            @click="tab = item.key"
          >
            {{ item.label }}
          </button>
        </div>

        <ImportWizard v-if="tab === 'import'" @imported="showImported" />

        <template v-else>
          <p class="data__pane-text">
            Слева — копия на чёрный день, её можно спокойно положить на флешку. Справа — открытый
            текст для переезда в другой менеджер: короткая жизнь, быстро удалить.
          </p>

          <div class="data__exports">
            <BackupCard :records="recordCount" :vaults="vaultCount" />
            <CsvExportCard :records="recordCount" />
          </div>

          <p class="data__foot">
            Оба файла создаются локально и никуда не отправляются: ушло в интернет — 0 байт.
          </p>
        </template>
      </section>
    </div>
  </main>
</template>

<style scoped>
.data {
  display: flex;
  flex-direction: column;
  min-height: 100%;
  background: var(--sy-bg-1);
}

.data__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sy-space-5);
  padding: var(--sy-space-5) var(--sy-space-6);
  border-bottom: 1px solid var(--sy-border);
  background: var(--sy-bg-0);
}

.data__brand {
  display: flex;
  align-items: baseline;
  gap: var(--sy-space-6);
}

.data__back {
  font-size: var(--sy-text-body);
  color: var(--sy-accent);
  text-decoration: none;
}

.data__back:hover {
  text-decoration: underline;
}

.data__title {
  font-size: var(--sy-text-body-strong);
  font-weight: var(--sy-weight-semibold);
}

.data__body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: var(--sy-space-9) var(--sy-space-8);
}

.data__pane {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-7);
  max-width: 980px;
  margin: 0 auto;
}

.data__intro {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-2);
}

.data__pane-title {
  font-size: var(--sy-text-h2);
  line-height: var(--sy-text-h2-lh);
  font-weight: var(--sy-weight-semibold);
  letter-spacing: -0.015em;
}

.data__pane-text {
  font-size: var(--sy-text-body);
  line-height: 1.6;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.data__tabs {
  display: flex;
  gap: var(--sy-space-3);
}

.data__tab {
  height: var(--sy-control-height-sm);
  padding: 0 var(--sy-space-6);
  border: 1px solid var(--sy-border-strong);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
  font: inherit;
  font-size: var(--sy-text-body);
  color: var(--sy-text);
  cursor: pointer;
}

.data__tab--on {
  border-color: var(--sy-accent-border);
  background: var(--sy-accent-quiet);
  color: var(--sy-accent);
}

.data__exports {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--sy-space-6);
  align-items: start;
}

.data__foot {
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--sy-text-3);
}

@media (max-width: 900px) {
  .data__exports {
    grid-template-columns: minmax(0, 1fr);
  }

  .data__body {
    padding: var(--sy-space-8) var(--sy-space-6);
  }
}
</style>

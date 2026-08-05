<script setup lang="ts">
import { onMounted, ref } from 'vue'

import type { RecordMeta } from '@/core/contract'
import { isCoreError } from '@/core/errors'
import { useCore } from '@/core/ipc'

/**
 * Временный экран-заглушка: проверяет сквозной путь UI → IPC → мок-ядро.
 * Настоящий список (поиск, группировка, состояния) — задача F4,
 * экран разблокировки — F3. Стилей нет намеренно: токены приходят в F2.
 *
 * В шаблоне ниже — только метаданные. Секретов на этом экране нет и не будет.
 */

const core = useCore()

const records = ref<RecordMeta[]>([])
const error = ref<string | null>(null)
const loading = ref(true)

onMounted(async () => {
  try {
    // Заглушка вместо экрана входа (F3): мок-ядро стартует заблокированным.
    // Пароль дев-мока (MOCK_MASTER_PASSWORD из src/core/mock/seed.ts) вписан
    // строкой, чтобы не тянуть мок-модуль в прод-бандл.
    await core.unlock('syncra-dev')
    records.value = await core.listRecords()
  } catch (cause) {
    error.value = isCoreError(cause) ? cause.message : 'Не удалось связаться с ядром.'
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <main>
    <h1>Syncra</h1>

    <p v-if="loading">Загрузка…</p>
    <p v-else-if="error">{{ error }}</p>
    <ul v-else>
      <li v-for="record in records" :key="record.record_id">
        {{ record.service_name }} — {{ record.login }}
        <span v-if="record.account_label">({{ record.account_label }})</span>
      </li>
    </ul>
  </main>
</template>

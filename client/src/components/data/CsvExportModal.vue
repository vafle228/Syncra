<script setup lang="ts">
import { computed } from 'vue'

import { SyModal } from '@/components/ui'
import { useRecordsStore } from '@/stores/useRecordsStore'

import CsvExportCard from './CsvExportCard.vue'

/**
 * Экспорт в CSV (F12, §6.2) — модалка поверх окна (F13).
 *
 * Тон `danger` и формулировки риска переносятся ДОСЛОВНО: `CLAUDE.md` прямо
 * запрещает смягчать тексты безопасности, а этот файл кладёт пароли на диск
 * открытым текстом. Слово-подтверждение и поле мастер-пароля остаются на месте.
 */

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()

const records = useRecordsStore()

/** Сколько строк ляжет в файл открытым текстом — это надо назвать вслух. */
const recordCount = computed(() => records.totalAll)
</script>

<template>
  <SyModal
    :open="props.open"
    size="form"
    tone="danger"
    title="Экспорт в CSV"
    warning="Файл не шифруется: пароли внутри читаются как обычный текст."
    @close="emit('close')"
  >
    <div v-if="props.open" data-test="csv-modal">
      <CsvExportCard :records="recordCount" />
    </div>
  </SyModal>
</template>

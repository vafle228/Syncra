<script setup lang="ts">
import { computed } from 'vue'

import { SyModal } from '@/components/ui'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'

import BackupCard from './BackupCard.vue'

/**
 * Зашифрованный бэкап (F12, §6.2) — модалка поверх окна (F13).
 *
 * Поле мастер-пароля остаётся, хотя в диалоге прототипа его нет: `export_backup`
 * требует его по контракту, и без него команда просто не уйдёт.
 */

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()

const records = useRecordsStore()
const sections = useSectionsStore()

/** Сколько уедет в файл: живые записи всех секций, включая локальные. */
const recordCount = computed(() => records.totalAll)
const vaultCount = computed(() => sections.vaults.length)
</script>

<template>
  <SyModal :open="props.open" size="form" title="Зашифрованный бэкап" @close="emit('close')">
    <div v-if="props.open" data-test="backup-modal">
      <BackupCard :records="recordCount" :vaults="vaultCount" />
    </div>
  </SyModal>
</template>

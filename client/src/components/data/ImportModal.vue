<script setup lang="ts">
import { SyModal } from '@/components/ui'
import type { VaultId } from '@/core/contract'

import ImportWizard from './ImportWizard.vue'

/**
 * Импорт из другого менеджера (F12, §6.2) — модалка поверх окна (F13).
 *
 * Обёртка намеренно тонкая: весь мастер живёт в `ImportWizard`, у него свой
 * спек, и переезд в модалку не должен был его переписывать.
 *
 * `v-if` на содержимом обязателен: мастер держит сеанс импорта в ядре, и
 * оставлять его смонтированным за закрытым диалогом значило бы держать открытым
 * разобранный чужой файл.
 */

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: []; imported: [vaultId: VaultId, count: number] }>()

function onImported(vaultId: string, count: number): void {
  emit('imported', vaultId, count)
}
</script>

<template>
  <SyModal :open="props.open" size="wizard" title="Импорт из другого менеджера" @close="emit('close')">
    <div v-if="props.open" class="import-modal" data-test="import-modal">
      <ImportWizard @imported="onImported" />
    </div>
  </SyModal>
</template>

<style scoped>
.import-modal {
  display: flex;
  flex-direction: column;
}
</style>

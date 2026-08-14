<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { SyButton, SyInput, SyModal } from '@/components/ui'
import { useVaultExport } from '@/composables/useDataTransfer'
import type { ExportFile } from '@/core/contract'
import { useRecordsStore } from '@/stores/useRecordsStore'
import { useSectionsStore } from '@/stores/useSectionsStore'

import { formatFileSize } from './importSources'

/**
 * Зашифрованный бэкап (F12, §6.2) — модалка поверх окна (F13).
 *
 * Диалога для бэкапа в макетах нет: там экспорт срабатывает сразу. Он остаётся
 * осознанным отступлением — `export_backup` требует мастер-пароль по контракту,
 * и спросить его негде, кроме диалога. Но всё лишнее из него убрано: заголовок
 * один (свой у `SyModal`), состав файла — двухколоночная сводка вместо четырёх
 * рамок подряд, действие — в подвале диалога, как у остальных.
 *
 * Мастер-пароль уходит транзитом: файл собирает и шифрует ЯДРО, сюда
 * возвращается только путь и размер.
 */

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{
  close: []
  /** Файл создан — карточка настроек показывает след. */
  done: [file: ExportFile]
}>()

const records = useRecordsStore()
const sections = useSectionsStore()

const backup = useVaultExport('backup')
const masterPassword = ref('')

/** Что уедет в файл: живые записи всех секций, включая локальные. */
const specs = computed(() => [
  { label: 'Формат', value: '.syncra' },
  { label: 'Чем закрыт', value: 'мастер-пароль' },
  { label: 'Записей', value: String(records.totalAll) },
  { label: 'Секций', value: String(sections.vaults.length) },
])

async function save(): Promise<void> {
  const attempt = masterPassword.value
  // Пароль ушёл в ядро — здесь он больше не нужен.
  masterPassword.value = ''
  if (await backup.run(attempt)) {
    const file = backup.file.value
    if (file) emit('done', file)
  }
}

watch(
  () => props.open,
  (open) => {
    if (!open) {
      masterPassword.value = ''
      backup.forget()
    }
  },
)
</script>

<template>
  <SyModal :open="props.open" size="form" title="Зашифрованный бэкап" @close="emit('close')">
    <div v-if="props.open" class="backup" data-test="backup-modal">
      <p class="backup__lead">
        Хранилище целиком, закрытое тем же мастер-паролем. Без него файл — бесполезный набор байтов,
        поэтому его не страшно держать на флешке или на внешнем диске.
      </p>

      <!-- Сводка, а не четыре рамки подряд: это одна мысль о составе файла. -->
      <dl class="backup__specs">
        <div v-for="spec in specs" :key="spec.label" class="backup__spec">
          <dt class="backup__spec-label">{{ spec.label }}</dt>
          <dd class="backup__spec-value">{{ spec.value }}</dd>
        </div>
      </dl>

      <div v-if="backup.file.value" class="backup__done">
        <span class="backup__done-title">Бэкап сохранён</span>
        <span class="backup__done-path">
          {{ backup.file.value.path }} · {{ formatFileSize(backup.file.value.size_bytes) }}
        </span>
      </div>

      <SyInput
        v-model="masterPassword"
        label="Мастер-пароль"
        type="password"
        :revealable="false"
        autocomplete="current-password"
        hint="Экспорт всегда требует мастер-пароль — даже если хранилище уже открыто."
        :error="backup.error.value"
        @submit="save"
      />

      <p class="backup__note">
        Если сменить мастер-пароль, старый бэкап продолжит открываться старым — держите это в
        голове. Восстанавливают из такого файла на чистом устройстве: если Syncra на нём уже есть,
        хранилища придётся не сливать, а знакомить устройства по QR.
      </p>
    </div>

    <template #actions>
      <SyButton size="sm" :disabled="backup.busy.value" @click="emit('close')">Закрыть</SyButton>
      <SyButton
        variant="primary"
        size="sm"
        :disabled="masterPassword === ''"
        :loading="backup.busy.value"
        @click="save"
      >
        {{ backup.file.value ? 'Сохранить ещё раз' : 'Сохранить бэкап' }}
      </SyButton>
    </template>
  </SyModal>
</template>

<style scoped>
.backup {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-6);
}

.backup__lead {
  font-size: var(--sy-text-body);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

/* Две колонки: подпись и значение читаются парами, а не столбиком рамок. */
.backup__specs {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--sy-space-3) var(--sy-space-6);
  margin: 0;
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-surface);
}

.backup__spec {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--sy-space-4);
  min-width: 0;
}

.backup__spec-label {
  font-size: var(--sy-text-small);
  color: var(--sy-text-3);
}

.backup__spec-value {
  margin: 0;
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-caption);
}

.backup__done {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-1);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-accent-border);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-accent-quiet);
}

.backup__done-title {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-semibold);
}

.backup__done-path {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  color: var(--sy-text-2);
  word-break: break-all;
}

.backup__note {
  font-size: var(--sy-text-small);
  line-height: 1.55;
  color: var(--sy-text-3);
  text-wrap: pretty;
}
</style>

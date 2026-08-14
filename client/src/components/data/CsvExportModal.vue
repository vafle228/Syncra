<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { SyButton, SyInput, SyModal } from '@/components/ui'
import { pluralize } from '@/composables/plural'
import { useVaultExport } from '@/composables/useDataTransfer'
import type { ExportFile } from '@/core/contract'
import { useRecordsStore } from '@/stores/useRecordsStore'

import { formatFileSize } from './importSources'

/**
 * Экспорт в CSV (F12, §6.2) — модалка поверх окна (F13).
 *
 * Три уровня риска разведены цветом ровно так, как в макете
 * (`Прототип:2485-2500`): рамка диалога НЕЙТРАЛЬНАЯ, предупреждение янтарное,
 * красная только кнопка, которая правда кладёт пароли на диск. Раньше красным
 * было всё сразу — и тогда красный переставал что-либо значить.
 *
 * Формулировки §6.2 при этом не смягчены ни на слово: файл не шифруется, и
 * текст говорит это прямо. Смягчать их запрещено (`CLAUDE.md`).
 *
 * Ворот один — мастер-пароль, потому что его требует контракт `export_csv`.
 * Галочка и слово «ЭКСПОРТ» из спек-страницы (`Data and Settings:275-320`)
 * сюда не переехали: три подтверждения подряд не делают решение осознаннее,
 * они учат нажимать не глядя.
 */

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{
  close: []
  /** Файл создан — карточка настроек показывает след. */
  done: [file: ExportFile]
  /** Файл удалён с диска: следа больше нет. */
  forgotten: []
}>()

const csv = useVaultExport('csv')
const masterPassword = ref('')

/** Сколько строк ляжет в файл открытым текстом — это надо назвать вслух. */
const recordCount = computed(() => useRecordsStore().totalAll)

async function exportCsv(): Promise<void> {
  const attempt = masterPassword.value
  // Пароль ушёл в ядро — здесь он больше не нужен.
  masterPassword.value = ''
  if (await csv.run(attempt)) {
    const file = csv.file.value
    if (file) emit('done', file)
  }
}

async function removeFile(): Promise<void> {
  if (await csv.remove()) emit('forgotten')
}

/** Закрыли диалог — пароль и сообщение об ошибке с ним же и уходят. */
watch(
  () => props.open,
  (open) => {
    if (!open) {
      masterPassword.value = ''
      csv.forget()
    }
  },
)
</script>

<template>
  <SyModal
    :open="props.open"
    size="form"
    warning-tone="warning"
    warning="Файл не будет зашифрован: пароли внутри читаются как обычный текст. Удалите его сразу после переноса."
    title="Экспорт в CSV"
    @close="emit('close')"
  >
    <div v-if="props.open" class="csv" data-test="csv-modal">
      <p class="csv__lead">
        Для резервной копии лучше подойдёт зашифрованный бэкап — он открывается только вашим
        мастер-паролем.
      </p>

      <!-- Файл уже создан: диалог не закрывается, пока он лежит на диске. -->
      <div v-if="csv.file.value" class="csv__done">
        <span class="csv__done-title">Открытый файл лежит на диске</span>
        <span class="csv__done-path">
          {{ csv.file.value.path }} · {{ formatFileSize(csv.file.value.size_bytes) }}
        </span>
        <span class="csv__done-body">
          Файл не зашифрован. Удалите его сразу после использования — пока он на диске, ваши пароли
          читает любая программа на этом компьютере.
        </span>
      </div>

      <SyInput
        v-else
        v-model="masterPassword"
        label="Мастер-пароль"
        type="password"
        :revealable="false"
        autocomplete="current-password"
        hint="Экспорт всегда требует мастер-пароль — даже если хранилище уже открыто."
        :error="csv.error.value"
        @submit="exportCsv"
      />

      <p v-if="csv.file.value && csv.error.value" class="csv__error" role="alert">
        {{ csv.error.value }}
      </p>
    </div>

    <template #note>
      {{ pluralize(recordCount, ['пароль', 'пароля', 'паролей']) }} лягут в файл открытым текстом.
    </template>

    <template #actions>
      <template v-if="csv.file.value">
        <SyButton size="sm" @click="emit('close')">Закрыть</SyButton>
        <SyButton variant="danger" size="sm" :loading="csv.busy.value" @click="removeFile">
          Удалить файл сейчас
        </SyButton>
      </template>

      <template v-else>
        <SyButton size="sm" :disabled="csv.busy.value" @click="emit('close')">Отмена</SyButton>
        <SyButton
          variant="danger"
          size="sm"
          :disabled="masterPassword === ''"
          :loading="csv.busy.value"
          @click="exportCsv"
        >
          Сохранить CSV
        </SyButton>
      </template>
    </template>
  </SyModal>
</template>

<style scoped>
.csv {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-6);
}

.csv__lead {
  font-size: var(--sy-text-body);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

/* След файла — красный: он единственное здесь, что правда опасно. */
.csv__done {
  display: flex;
  flex-direction: column;
  gap: var(--sy-space-3);
  padding: var(--sy-space-5) var(--sy-space-6);
  border: 1px solid var(--sy-danger);
  border-radius: var(--sy-radius-sm);
  background: var(--sy-bg-0);
}

.csv__done-title {
  font-size: var(--sy-text-body);
  font-weight: var(--sy-weight-semibold);
  color: var(--sy-danger);
}

.csv__done-path {
  font-family: var(--sy-font-mono);
  font-size: var(--sy-text-label);
  color: var(--sy-text-2);
  word-break: break-all;
}

.csv__done-body {
  font-size: var(--sy-text-small);
  line-height: 1.55;
  color: var(--sy-text-2);
  text-wrap: pretty;
}

.csv__error {
  font-size: var(--sy-text-small);
  color: var(--sy-danger);
}
</style>
